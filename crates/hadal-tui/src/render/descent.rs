//! The DESCENT screen: depth gauge, descent card, hull tree, governor, confirm.

use hadal_core::Decimal;
use hadal_core::StateView;
use hadal_core::content::ids::HullId;
use hadal_core::content::tables;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use super::canvas::Canvas;
use crate::glyphs::{self, Glyphs};
use crate::model;
use crate::render::{
    Focus, HitMap, UiState, amount, bar, coach, cost, count, overlay_box, rate_text, render_rule,
    render_selection, render_tabs, selection_line, titled_frame, window_start,
};
use crate::theme::Palette;
use hadal_core::content::tutorial::TutorialTarget;

type P = Palette;

fn dim(p: P) -> Style {
    Style::new().fg(p.dim)
}
fn text(p: P) -> Style {
    Style::new().fg(p.text)
}
fn accent(p: P) -> Style {
    Style::new().fg(p.accent)
}
fn amber(p: P) -> Style {
    Style::new().fg(p.amber)
}
fn bold_amber(p: P) -> Style {
    Style::new().fg(p.amber).add_modifier(Modifier::BOLD)
}
fn bold_accent(p: P) -> Style {
    Style::new().fg(p.accent).add_modifier(Modifier::BOLD)
}

/// Draw the DESCENT screen.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) -> HitMap {
    let p = ui.palette;
    let g = glyphs::set(ui.ascii);
    let area = frame.area();

    let left = Line::from(vec![
        Span::styled(
            " HADAL",
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled("DESCENT ", text(p)),
    ]);
    let right = Line::from(vec![
        Span::styled(format!(" DEPTH {} m", count(ui.view.depth_m())), text(p)),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled(format!("DESCENTS {} ", count(ui.view.descents())), amber(p)),
    ]);
    let block = titled_frame(ui.ascii, &p, left, right);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 20 || inner.height < 15 {
        return HitMap::default();
    }

    let tabs = render_tabs(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        &p,
        ui.screen.tab(),
        crate::render::coach::target_tab(ui),
        ui.ascii,
    );
    let content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let mut hits = HitMap {
        tabs,
        ..HitMap::default()
    };

    let top_height = (content.height / 3).clamp(9, 10);
    let coach = if ui.coach_active() {
        crate::render::coach::HEIGHT
    } else {
        0
    };
    let selection = if !ui.coach_active() && selection_line(ui).is_some() {
        1
    } else {
        0
    };
    // Show the schematic when the terminal is at least 100 columns wide
    // (content width is the terminal width minus the two frame border cells).
    let schematic_height = if area.width >= 100 { 4u16 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(schematic_height), // hull schematic (0 if width < 100)
        Constraint::Length(1),                // rule
        Constraint::Min(0),                   // hull tree
        Constraint::Length(1),                // rule
        Constraint::Length(selection),        // selection explainer
        Constraint::Length(1),                // governor
        Constraint::Length(1),                // notice
        Constraint::Length(coach),            // reserved coach
        Constraint::Length(1),                // footer
    ])
    .areas::<10>(content);

    render_top(frame, rows[0], ui, g);
    if schematic_height > 0 {
        render_hull_schematic(frame, rows[1], ui);
    }
    render_rule(frame, rows[2], &p, g);
    render_hull_tree(frame, rows[3], ui, g, &mut hits);
    render_rule(frame, rows[4], &p, g);
    render_selection(frame, rows[5], ui);
    render_governor(frame, rows[6], ui);
    render_notice(frame, rows[7], ui);
    render_footer(frame, rows[9], ui, g);

    hits.tutorial_target = match ui.coach_target() {
        Some(TutorialTarget::Hull(id)) => hits.hulls.iter().find_map(|(rect, index)| {
            let target = model::flattened_hulls().iter().position(|hull| *hull == id);
            (Some(*index) == target).then_some(*rect)
        }),
        Some(TutorialTarget::HelpKey) => Some(rows[9]),
        _ => hits
            .tabs
            .iter()
            .find_map(|(rect, tab)| (Some(*tab) == coach::target_tab(ui)).then_some(*rect)),
    };

    if ui.confirm_descent {
        render_confirmation(frame, ui);
    }
    hits
}

// ---------------------------------------------------------------------
// Hull schematic (width >= 100 only)
// ---------------------------------------------------------------------

/// Draw the hull schematic as a legible text slot grid (width >= 100 only).
fn render_hull_schematic(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let view = ui.view;
    let ascii = ui.ascii;
    let owned = view.hull_count();

    // Row 0: title and ownership count.
    let header = Line::from(vec![
        Span::styled("HULL SCHEMATIC", bold_amber(p)),
        Span::styled(format!("  {}/17 owned", count(owned as u32)), dim(p)),
    ]);
    frame.render_widget(
        Paragraph::new(header),
        Rect::new(area.x, area.y, area.width, 1),
    );

    let branches = &model::HULL_BRANCHES;
    let label = |name: &'static str| Span::styled(format!("{name}  "), dim(p));

    // Row 1: AUTOMATION hulls.
    let mut line = vec![label(branches[0].0)];
    line.extend(schematic_slots(view, branches[0].1, p, ascii));
    frame.render_widget(
        Paragraph::new(Line::from(line)),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );

    // Row 2: SIGNAL, ENDURANCE and DESCENT hulls on one row.
    let mut line = vec![label(branches[1].0)];
    line.extend(schematic_slots(view, branches[1].1, p, ascii));
    line.push(Span::styled("   ", dim(p)));
    line.push(label(branches[2].0));
    line.extend(schematic_slots(view, branches[2].1, p, ascii));
    line.push(Span::styled("   ", dim(p)));
    line.push(label(branches[3].0));
    line.extend(schematic_slots(view, branches[3].1, p, ascii));
    frame.render_widget(
        Paragraph::new(Line::from(line)),
        Rect::new(area.x, area.y + 2, area.width, 1),
    );

    // Row 3: dim keel line, K-9 mark, and the shape legend.
    let (keel_glyph, legend) = if ascii {
        ("=", "# owned  : empty")
    } else {
        ("━", "█ owned  ░ empty")
    };
    let keel = keel_glyph.repeat(20);
    let line = vec![
        Span::styled(keel, dim(p)),
        Span::styled("  K-9", dim(p)),
        Span::styled("   ", dim(p)),
        Span::styled(legend, dim(p)),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(line)),
        Rect::new(area.x, area.y + 3, area.width, 1),
    );
}

/// One `██`/`░░` slot per hull, in table order, separated by single spaces.
fn schematic_slots(
    view: StateView<'_>,
    ids: &[HullId],
    p: Palette,
    ascii: bool,
) -> Vec<Span<'static>> {
    let (own, empty) = if ascii {
        ("##", "::")
    } else {
        ("██", "░░")
    };
    let mut spans = Vec::with_capacity(ids.len() * 2);
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", dim(p)));
        }
        if view.hull_owned(*id) {
            spans.push(Span::styled(own.to_owned(), accent(p)));
        } else {
            spans.push(Span::styled(empty.to_owned(), dim(p)));
        }
    }
    spans
}

// ---------------------------------------------------------------------
// Top: gauge + card
// ---------------------------------------------------------------------

fn render_top(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let cols = Layout::horizontal([
        Constraint::Length(22),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas::<3>(area);

    render_trench(frame, cols[0], ui);

    let divider: Vec<Line> = (0..cols[1].height)
        .map(|_| Line::from(Span::styled(g.v_line.to_string(), dim(ui.palette))))
        .collect();
    frame.render_widget(Paragraph::new(Text::from(divider)), cols[1]);

    render_card(frame, cols[2], ui, g);
}

/// Trench viewport: `top_m ..= bottom_m` centred on `depth`.
const TRENCH_TOP_OFFSET: u32 = 1200;
const TRENCH_BOTTOM_OFFSET: u32 = 1800;
const TRENCH_GHOST_OFFSET: u32 = 500; // metres to the next Descent
const TRENCH_RAIL_X: u16 = 3; // rail column (panel col 3)
const TRENCH_TRACE_X: u16 = 2; // trace column (panel col 2)
const TRENCH_SECTION_X: u16 = 4; // strata start after the rail
const TRENCH_STATION_X: u16 = 6; // silhouette left edge (panel cols 6-10)
const TRENCH_STATION_W: u16 = 5;
const TRENCH_BANDS: u16 = 4; // up to 4 silt bands (height permitting)
const TRENCH_PING_MAX_PX: i32 = 6; // ping ring max radius in pixels

/// CP437 shade glyph for the silt ramp, with an ASCII fallback.
fn shade_glyph(level: u8, ascii: bool) -> char {
    if ascii {
        match level {
            0 => ':',
            1 => '+',
            _ => '#',
        }
    } else {
        match level {
            0 => '░',
            1 => '▒',
            _ => '▓',
        }
    }
}

/// Write one styled cell into a buffer, ignoring out-of-bounds coordinates.
fn put_cell(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

/// A fresh 5×3-cell K-9 station silhouette canvas (`▄███▄` / `█████` / `▀███▀`).
fn station_canvas(color: Color) -> Canvas {
    let mut canvas = Canvas::new(5, 3);
    // row 0: ▄███▄
    canvas.put(0, 1, color);
    for x in 1..=3 {
        canvas.put(x, 0, color);
        canvas.put(x, 1, color);
    }
    canvas.put(4, 1, color);
    // row 1: █████
    for x in 0..=4 {
        canvas.put(x, 2, color);
        canvas.put(x, 3, color);
    }
    // row 2: ▀███▀
    canvas.put(0, 4, color);
    for x in 1..=3 {
        canvas.put(x, 4, color);
        canvas.put(x, 5, color);
    }
    canvas.put(4, 4, color);
    canvas
}

/// Draw the trench cross-section into `area` (22 cols wide).
fn render_trench(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let ascii = ui.ascii;
    if area.height < 5 || area.width < 12 {
        return;
    }
    let depth = ui.view.depth_m();
    let next_depth = depth.saturating_add(TRENCH_GHOST_OFFSET);

    // Row 0: header. Last row: depth labels. The middle rows are strata.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("TRENCH", dim(p)))),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let strata_top = area.y + 1;
    let strata_rows = area.height - 2;
    let depth_row = area.y + area.height - 1;
    let rail_x = area.x + TRENCH_RAIL_X;
    let trace_x = area.x + TRENCH_TRACE_X;
    let section_x = area.x + TRENCH_SECTION_X;
    let panel_right = area.x + area.width;
    let station_x = area.x + TRENCH_STATION_X;
    let dim_style = Style::new().fg(p.dim).bg(p.bg);
    let accent_style = Style::new().fg(p.accent).bg(p.bg);
    let amber_style = Style::new().fg(p.amber).bg(p.bg);

    // Silt bands: even shade rows split by single dim boundary rows. The rail
    // is drawn for every section row so cols 0-3 stay continuous.
    let bands = TRENCH_BANDS.min(strata_rows / 2).max(1);
    let boundaries = bands - 1;
    let shade_rows = strata_rows - boundaries;
    let base = shade_rows / bands;
    let rem = shade_rows % bands;
    let ramp = [0u8, 1, 2, 1, 2];
    let rail_glyph = if ascii { '|' } else { '│' };
    let tick_glyph = if ascii { '+' } else { '├' };
    let run_glyph = if ascii { '_' } else { '▁' };
    let mut y = strata_top;
    for b in 0..bands {
        let rows = base + if b < rem { 1 } else { 0 };
        let glyph = shade_glyph(ramp[(b as usize).min(ramp.len() - 1)], ascii);
        for _ in 0..rows {
            for x in section_x..panel_right {
                put_cell(frame.buffer_mut(), x, y, glyph, dim_style);
            }
            put_cell(frame.buffer_mut(), rail_x, y, rail_glyph, dim_style);
            y += 1;
        }
        if b + 1 < bands {
            for x in section_x..panel_right {
                put_cell(frame.buffer_mut(), x, y, run_glyph, dim_style);
            }
            put_cell(frame.buffer_mut(), rail_x, y, tick_glyph, dim_style);
            y += 1;
        }
    }

    // Map absolute metres to a strata row (integer arithmetic only).
    let pixel_h = strata_rows as i32 * 2;
    let top_m = depth.saturating_sub(TRENCH_TOP_OFFSET);
    let span = (TRENCH_TOP_OFFSET + TRENCH_BOTTOM_OFFSET).max(1) as i64;
    let m_to_pixel = |m: u32| -> i32 {
        let py = ((m as i64 - top_m as i64) * (pixel_h as i64 - 1)) / span;
        (py as i32).clamp(0, pixel_h - 1)
    };
    let centre_rel = (m_to_pixel(depth) / 2).clamp(1, (strata_rows - 2) as i32) as u16;
    let station_row = strata_top + centre_rel;

    // Dim trace in the rail (col 2) from the top down to the station row.
    let trace_glyph = if ascii { '.' } else { '·' };
    for rel in 0..=centre_rel {
        put_cell(
            frame.buffer_mut(),
            trace_x,
            strata_top + rel,
            trace_glyph,
            dim_style,
        );
    }

    // Next Descent: a single dim diamond on the rail, unmistakably pending.
    let mut next_rel = (m_to_pixel(next_depth) / 2).clamp(0, (strata_rows - 1) as i32) as u16;
    if next_rel == centre_rel {
        next_rel = if centre_rel + 1 < strata_rows {
            centre_rel + 1
        } else {
            centre_rel.saturating_sub(1)
        };
    }
    let muted_glyph = if ascii { 'o' } else { '◇' };
    put_cell(
        frame.buffer_mut(),
        rail_x,
        strata_top + next_rel,
        muted_glyph,
        dim_style,
    );

    // The current station: an amber diamond on the rail plus the 5x3 amber K-9
    // silhouette at panel cols 6-10 (off the rail, on top of the shade).
    let tuned_glyph = if ascii { '*' } else { '◆' };
    put_cell(
        frame.buffer_mut(),
        rail_x,
        station_row,
        tuned_glyph,
        amber_style,
    );
    let marker_top = station_row - 1;
    station_canvas(p.amber).blit(
        frame.buffer_mut(),
        Rect::new(station_x, marker_top, TRENCH_STATION_W, 3),
        ui.settings.color_depth,
        ascii,
        p.bg,
    );

    // Ping: one accent ring every 2 s, centred on the station, clipped to the
    // section so the rail and trace are never overwritten.
    let radius = if ui.anim.reduce_motion {
        TRENCH_PING_MAX_PX / 2
    } else {
        ((ui.anim.frame_ms % 2000) as i32) * TRENCH_PING_MAX_PX / 2000
    };
    if radius > 0 {
        let ring_glyph = if ascii { '.' } else { '·' };
        let cx = station_x as i32 + TRENCH_STATION_W as i32 / 2;
        let cy = centre_rel as i32;
        let lower = (radius - 1).max(0);
        for ry in 0..strata_rows {
            let abs_y = strata_top + ry;
            for rx in 0..area.width {
                let abs_x = area.x + rx;
                // Never paint over the rail/trace (panel cols 0-3).
                if abs_x < section_x {
                    continue;
                }
                // Keep the silhouette opaque.
                let over_station = abs_x >= station_x
                    && abs_x < station_x + TRENCH_STATION_W
                    && abs_y >= marker_top
                    && abs_y <= marker_top + 2;
                if over_station {
                    continue;
                }
                let dx = abs_x as i32 - cx;
                let dy = (ry as i32 - cy) * 2;
                let d2 = dx * dx + dy * dy;
                if d2 <= radius * radius && d2 > lower * lower {
                    put_cell(frame.buffer_mut(), abs_x, abs_y, ring_glyph, accent_style);
                }
            }
        }
    }

    // Bottom row: clear to bg, then write the depth (and next depth if it fits).
    for x in area.x..panel_right {
        put_cell(
            frame.buffer_mut(),
            x,
            depth_row,
            ' ',
            Style::new().fg(p.bg).bg(p.bg),
        );
    }
    let depth_text = format!("{} m", count(depth));
    let next_text = format!("next {next_depth}");
    let avail = area.width.saturating_sub(TRENCH_SECTION_X) as usize;
    let label = if depth_text.chars().count() + 2 + next_text.chars().count() <= avail {
        format!("{depth_text}  {next_text}")
    } else {
        depth_text
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, dim(p)))),
        Rect::new(
            section_x,
            depth_row,
            area.width.saturating_sub(TRENCH_SECTION_X),
            1,
        ),
    );
}

fn render_card(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    let view = ui.view;
    let progress = model::descent_progress(view);
    let gain = view.descent_gain();
    let sep = if ui.ascii { " . " } else { " · " };
    let card_width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("RUN    ", dim(p)),
        Span::styled(
            format!(
                "peak {}   rate {}/s",
                amount(view.peak()),
                rate_text(view.rate())
            ),
            text(p),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ", dim(p)),
        Span::styled(
            format!(
                "descents {}  best {}  era F {}  life F {}",
                count(view.descents()),
                amount(view.best_peak()),
                count(view.era_fathoms()),
                count(view.lifetime_fathoms())
            ),
            text(p),
        ),
    ]));

    let gain_style = if view.can_descend() {
        bold_amber(p)
    } else {
        dim(p)
    };
    lines.push(Line::from(vec![
        Span::styled("DESCENT ", dim(p)),
        Span::styled(format!("+{gain} FATHOMS"), gain_style),
        Span::styled(
            if view.can_descend() {
                "   READY"
            } else {
                "   NEED 1.00M PEAK"
            },
            dim(p),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("NEXT    ", dim(p)),
        Span::styled(amount(progress.next), text(p)),
        Span::styled("  ", dim(p)),
        Span::styled(bar(progress.fraction, 10, g), accent(p)),
        Span::styled(
            format!(" {:>3}%", (progress.fraction * 100.0).round() as u32),
            dim(p),
        ),
    ]));

    let reset_items: Vec<&str> = view.reset_scope().to_vec();
    let reset = pack_items(&reset_items, card_width.saturating_sub(7), sep);
    let persist = pack_items(model::PERSIST_SCOPE, card_width.saturating_sub(9), sep);
    push_packed_lines(&mut lines, "RESETS", &reset, p);
    push_packed_lines(&mut lines, "PERSISTS", &persist, p);

    frame.render_widget(Paragraph::new(lines), area);
}

fn push_packed_lines(lines: &mut Vec<Line<'static>>, prefix: &str, packed: &[String], p: P) {
    for (index, line) in packed.iter().enumerate() {
        if index == 0 {
            lines.push(Line::from(vec![
                Span::styled(format!("{prefix} "), dim(p)),
                Span::styled(line.clone(), text(p)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}{line}", " ".repeat(prefix.len() + 1)),
                text(p),
            )));
        }
    }
}

/// Greedily pack items into lines joined by `sep`, each at most `width` wide.
pub fn pack_items(items: &[&str], width: usize, sep: &str) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for item in items {
        let piece = if current.is_empty() {
            (*item).to_owned()
        } else {
            format!("{sep}{item}")
        };
        if !current.is_empty() && current.chars().count() + piece.chars().count() > width {
            lines.push(current);
            current = (*item).to_owned();
        } else {
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The exact reset and persist item lines used by the card and confirmation.
pub fn reset_persist_lines(
    view: StateView<'_>,
    width: usize,
    sep: &str,
) -> (Vec<String>, Vec<String>) {
    let reset_items: Vec<&str> = view.reset_scope().to_vec();
    (
        pack_items(&reset_items, width, sep),
        pack_items(model::PERSIST_SCOPE, width, sep),
    )
}

/// The confirmation's plain-text content (used by the renderer and tests).
pub fn confirmation_lines(
    view: StateView<'_>,
    width: usize,
    sep: &str,
    ascii: bool,
) -> Vec<String> {
    let progress = model::descent_progress(view);
    let arrow = if ascii { "->" } else { "→" };
    let (reset, persist) = reset_persist_lines(view, width, sep);
    let mut lines = vec![
        format!("Descent grants +{} Fathoms.", view.descent_gain()),
        format!(
            "Depth {} m {arrow} {} m.",
            count(view.depth_m()),
            count(view.depth_m().saturating_add(tables::DEPTH_PER_DESCENT_M))
        ),
        format!("Next bucket at {}.", amount(progress.next)),
        String::new(),
        "RESETS".to_owned(),
    ];
    for line in reset {
        lines.push(format!("  {line}"));
    }
    lines.push("PERSISTS".to_owned());
    for line in persist {
        lines.push(format!("  {line}"));
    }
    lines.push(String::new());
    lines.push("[y] confirm descent     [Esc] cancel".to_owned());
    lines
}

// ---------------------------------------------------------------------
// Hull tree
// ---------------------------------------------------------------------

enum HullRow {
    Branch(&'static str),
    Hull(HullId, usize),
}

fn hull_rows() -> Vec<HullRow> {
    let mut rows = Vec::new();
    let mut index = 0usize;
    for (branch, hulls) in model::HULL_BRANCHES {
        rows.push(HullRow::Branch(branch));
        for hull in hulls {
            rows.push(HullRow::Hull(*hull, index));
            index += 1;
        }
    }
    rows
}

fn hull_focus_row(focus: Focus) -> Option<usize> {
    let Focus::Hull(target) = focus else {
        return None;
    };
    hull_rows()
        .iter()
        .position(|row| matches!(row, HullRow::Hull(_, index) if *index == target))
}

fn render_hull_tree(
    frame: &mut Frame,
    area: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
) {
    if area.height == 0 {
        return;
    }
    let p = ui.palette;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("HULL TREE", bold_amber(p)),
            Span::styled(
                format!(
                    "   {}/17 owned   {}{} focus   Enter buy",
                    ui.view.hull_count(),
                    g.arrow_up,
                    g.arrow_down
                ),
                dim(p),
            ),
        ])),
        Rect::new(area.x, area.y, area.width, 1),
    );

    let visible = area.height.saturating_sub(1) as usize;
    if visible == 0 {
        return;
    }
    let rows = hull_rows();
    let target_index = match ui.coach_target() {
        Some(TutorialTarget::Hull(id)) => rows
            .iter()
            .position(|row| matches!(row, HullRow::Hull(hull, _) if *hull == id)),
        _ => None,
    };
    let start = window_start(
        rows.len(),
        visible,
        target_index.or_else(|| hull_focus_row(ui.focus)),
    );
    for (offset, row) in rows.iter().skip(start).take(visible).enumerate() {
        let rect = Rect::new(area.x, area.y + 1 + offset as u16, area.width, 1);
        match row {
            HullRow::Branch(name) => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(*name, bold_amber(p)))),
                    rect,
                );
            }
            HullRow::Hull(id, index) => {
                hits.hulls.push((rect, *index));
                render_hull_row(frame, rect, *id, *index, ui);
            }
        }
    }
}

fn render_hull_row(frame: &mut Frame, area: Rect, id: HullId, index: usize, ui: &UiState<'_>) {
    let p = ui.palette;
    let view = ui.view;
    let owned = view.hull_owned(id);
    let locked = !owned && !view.hull_prereq_met(id);
    let affordable = view.hull_affordable(id);
    let focused = matches!(ui.focus, Focus::Hull(focus) if focus == index);
    let targeted = coach::target_is(ui, TutorialTarget::Hull(id));

    let marker = if targeted {
        coach::marker(ui.ascii)
    } else if focused {
        ">"
    } else {
        " "
    };
    let state = if owned {
        "OWNED"
    } else if locked {
        "LOCK"
    } else if affordable {
        "READY"
    } else {
        "NEED"
    };
    let state_style = if owned {
        accent(p)
    } else if locked {
        dim(p)
    } else if affordable {
        bold_accent(p)
    } else {
        dim(p)
    };
    let effect = model::hull_effect(id, ui.ascii);
    let detail = if locked {
        match model::hull_requirement(id) {
            Some(requirement) => {
                let sep = if ui.ascii { " . " } else { " · " };
                format!("needs {requirement}{sep}{effect}")
            }
            None => effect,
        }
    } else {
        effect
    };
    let cost_text = format!(
        "{}F",
        cost(Decimal::from_u64(u64::from(view.hull_cost(id))))
    );
    let sep_detail = model::ascii_text(&detail, ui.ascii);

    let line = Line::from(vec![
        Span::styled(
            format!("{marker} "),
            if focused { bold_amber(p) } else { dim(p) },
        ),
        Span::styled(
            format!("{:<17}", model::hull_name(id)),
            if focused || targeted {
                bold_amber(p)
            } else {
                text(p)
            },
        ),
        Span::styled(format!("{cost_text:>6}"), text(p)),
        Span::styled(format!(" {state:<6} "), state_style),
        Span::styled(sep_detail, dim(p)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------
// Governor, notice, footer
// ---------------------------------------------------------------------

fn render_governor(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let view = ui.view;
    let target = view.auto_descend_at();
    let line = if view.hull_owned(HullId::BallastGovernor) {
        let value = if target == 0 {
            "OFF".to_owned()
        } else {
            format!("{}F", count(target))
        };
        Line::from(vec![
            Span::styled("BALLAST GOVERNOR ", dim(p)),
            Span::styled(value, bold_amber(p)),
            Span::styled(
                "   -/+ adjust (Shift 10)   valid OFF or 3-1,000,000",
                dim(p),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("BALLAST GOVERNOR ", dim(p)),
            Span::styled("LOCKED", amber(p)),
            Span::styled(
                format!(
                    "   buy Ballast Governor ({}F) to automate descents",
                    count(view.hull_cost(HullId::BallastGovernor))
                ),
                dim(p),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_notice(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let Some(notice) = ui.notice else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            notice.to_owned(),
            Style::new().fg(ui.palette.alert),
        ))),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{}{} focus  Enter buy  p+y descend  +/- target  Esc back  ",
                    g.arrow_up, g.arrow_down
                ),
                dim(p),
            ),
            coach::help_span(ui, p),
            Span::styled("  1 sonar", dim(p)),
        ])),
        area,
    );
}

fn render_confirmation(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let width = area.width.saturating_sub(8).clamp(50, 80);
    let sep = if ui.ascii { " . " } else { " · " };
    let lines = confirmation_lines(ui.view, width.saturating_sub(6) as usize, sep, ui.ascii);
    let height = (lines.len() as u16).saturating_add(2).min(area.height);
    let inner = overlay_box(
        frame,
        area,
        width,
        height,
        ui.palette,
        ui.ascii,
        "CONFIRM DESCENT",
    );
    let text_lines: Vec<Line> = lines
        .into_iter()
        .map(|line| {
            let style = if line.starts_with("RESETS") || line.starts_with("PERSISTS") {
                Style::new()
                    .fg(ui.palette.amber)
                    .add_modifier(Modifier::BOLD)
            } else if line.starts_with("[y]") {
                Style::new().fg(ui.palette.accent)
            } else {
                Style::new().fg(ui.palette.text)
            };
            Line::from(Span::styled(line, style))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(text_lines).style(Style::new().bg(ui.palette.panel)),
        inner,
    );
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_lists_exact_scope_and_gain() {
        let engine = hadal_core::Engine::demo();
        let view = engine.view();
        let lines = confirmation_lines(view, 66, " · ", false);
        let joined = lines.join("\n");

        assert!(joined.contains(&format!("+{} Fathoms", view.descent_gain())));
        assert!(joined.contains(&amount(model::descent_progress(view).next)));
        for item in view.reset_scope() {
            assert!(joined.contains(item), "confirmation missing reset {item}");
        }
        for item in model::PERSIST_SCOPE {
            assert!(joined.contains(item), "confirmation missing persist {item}");
        }
        assert!(joined.contains("[y] confirm descent"));
        assert!(joined.contains("[Esc] cancel"));
    }

    #[test]
    fn pack_items_respects_width_and_keeps_items() {
        let items = ["Alpha", "Beta", "Gamma Delta", "Epsilon"];
        let packed = pack_items(&items, 18, " · ");
        assert!(packed.iter().all(|line| line.chars().count() <= 18));
        let joined = packed.join(" · ");
        for item in items {
            assert!(joined.contains(item), "missing {item}");
        }
    }

    #[test]
    fn hull_rows_include_all_hulls_once() {
        let rows = hull_rows();
        let hulls: Vec<HullId> = rows
            .iter()
            .filter_map(|row| match row {
                HullRow::Hull(id, _) => Some(*id),
                HullRow::Branch(_) => None,
            })
            .collect();
        assert_eq!(hulls.len(), 17);
        for id in HullId::ALL {
            assert_eq!(hulls.iter().filter(|h| *h == id).count(), 1, "{id}");
        }
        let branches = rows
            .iter()
            .filter(|row| matches!(row, HullRow::Branch(_)))
            .count();
        assert_eq!(branches, 4);
    }

    #[test]
    fn trench_section_has_lit_cells() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let engine = hadal_core::Engine::demo();
        let view = engine.view();
        let anim = crate::render::Anim::settled(view.echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );
        let ui = crate::render::UiState::settled(
            view,
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                let _ = render(frame, &ui);
            })
            .expect("draw frame");
        let buffer = terminal.backend().buffer().clone();

        // The trench gauge is in cols 0-21, rows 2-10 (approx).
        // Count non-space cells in that region.
        let mut lit = 0usize;
        for y in 2..=10 {
            for x in 0..22 {
                let sym = buffer[(x, y)].symbol();
                if sym != " " {
                    lit += 1;
                }
            }
        }
        assert!(
            lit >= 30,
            "trench gauge should have >= 30 lit cells, got {}",
            lit
        );
    }

    #[test]
    fn trench_marker_moves_with_depth() {
        let engine1 = hadal_core::Engine::demo();
        let engine2 = hadal_core::Engine::new();
        let anim = crate::render::Anim::settled(engine1.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );

        let ui1 = crate::render::UiState::settled(
            engine1.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );
        let ui2 = crate::render::UiState::settled(
            engine2.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );

        fn gauge_text(ui: &crate::render::UiState<'_>) -> String {
            use ratatui::Terminal;
            use ratatui::backend::TestBackend;
            let backend = TestBackend::new(80, 24);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 2..=10 {
                for x in 0..22 {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        }

        let text1 = gauge_text(&ui1);
        let text2 = gauge_text(&ui2);
        assert_ne!(
            text1, text2,
            "different depths should produce different buffers"
        );
    }

    #[test]
    fn trench_art_deterministic_at_fixed_frame() {
        let engine = hadal_core::Engine::demo();
        let view = engine.view();
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );

        fn render_frame(ui: &crate::render::UiState<'_>) -> String {
            use ratatui::Terminal;
            use ratatui::backend::TestBackend;
            let backend = TestBackend::new(80, 24);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        }

        let anim1 = crate::render::Anim {
            frame_ms: 1234,
            ..crate::render::Anim::settled(view.echo())
        };
        let ui1 = crate::render::UiState::settled(
            view,
            pal,
            false,
            crate::render::Screen::Descent,
            &anim1,
        );
        let text1 = render_frame(&ui1);

        let anim2 = crate::render::Anim {
            frame_ms: 1234,
            ..crate::render::Anim::settled(view.echo())
        };
        let ui2 = crate::render::UiState::settled(
            view,
            pal,
            false,
            crate::render::Screen::Descent,
            &anim2,
        );
        let text2 = render_frame(&ui2);

        assert_eq!(text1, text2, "same frame_ms must produce identical output");
    }

    #[test]
    fn trench_is_not_a_grid() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let engine = hadal_core::Engine::demo();
        let depth = engine.view().depth_m();
        let amber_color = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        )
        .amber;
        let anim = crate::render::Anim::settled(engine.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );
        let ui = crate::render::UiState::settled(
            engine.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                let _ = render(frame, &ui);
            })
            .expect("draw frame");
        let buf = terminal.backend().buffer();

        // Locate the gauge header, its depth label row, and the panel origin.
        let depth_label = format!("{} m", count(depth));
        let mut header_row = None;
        let mut panel_x = 0u16;
        let mut depth_row = None;
        for y in 0..buf.area().height {
            let mut line = String::new();
            for x in 0..buf.area().width {
                line.push_str(buf[(x, y)].symbol());
            }
            if let Some(byte_pos) = line.find("TRENCH") {
                header_row = Some(y);
                panel_x = line[..byte_pos].chars().count() as u16;
            } else if header_row.is_some() && line.contains(&depth_label) {
                depth_row = Some(y);
            }
        }
        let header_row = header_row.expect("trench header present");
        let depth_row = depth_row.expect("trench depth label present");
        let rail_x = panel_x + 3;
        let station_x = panel_x + 6;

        // Structural check: no strata row is more than 80% solid-block cells,
        // and the rail glyph survives on every section row.
        let blocky = |sym: &str| matches!(sym, "█" | "▀" | "▄");
        for y in (header_row + 1)..depth_row {
            let lit = (panel_x..panel_x + 22)
                .filter(|&x| blocky(buf[(x, y)].symbol()))
                .count();
            assert!(
                lit * 5 <= 22 * 4,
                "strata row {y} is a grid: {lit}/22 block cells"
            );
            let rail = buf[(rail_x, y)].symbol();
            assert!(
                matches!(rail, "│" | "├" | "◆" | "◇"),
                "rail glyph missing on section row {y}: {rail:?}"
            );
        }

        // Marker check: the amber silhouette sits at cols 6-10, and the rail
        // carries an amber ◆ at the mapped depth row.
        let strata_rows = depth_row - header_row - 1;
        let pixel_h = strata_rows as i32 * 2;
        let py = (1200i64 * (pixel_h as i64 - 1) / 3000) as i32;
        let marker_rel = (py / 2).clamp(1, strata_rows as i32 - 2) as u16;
        let marker_row = header_row + 1 + marker_rel;
        let mut amber = 0usize;
        for y in marker_row.saturating_sub(1)..=(marker_row + 1) {
            for x in station_x..(station_x + 5) {
                if buf[(x, y)].fg == amber_color {
                    amber += 1;
                }
            }
        }
        assert!(
            amber >= 3,
            "marker row {marker_row} should have >= 3 amber cells at cols 6-10, got {amber}"
        );
        assert_eq!(
            buf[(rail_x, marker_row)].symbol(),
            "◆",
            "current station must mark the rail with ◆ at the mapped row"
        );
        assert_eq!(
            buf[(rail_x, marker_row)].fg,
            amber_color,
            "the rail diamond must be amber"
        );
    }

    #[test]
    fn hull_schematic_slot_pairs_match_owned() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        fn slot_pairs(ui: &crate::render::UiState<'_>) -> usize {
            let backend = TestBackend::new(100, 30);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut header_row = None;
            for y in 0..buf.area().height {
                let mut line = String::new();
                for x in 0..buf.area().width {
                    line.push_str(buf[(x, y)].symbol());
                }
                if line.contains("HULL SCHEMATIC") {
                    header_row = Some(y);
                    break;
                }
            }
            let header_row = header_row.expect("schematic header present at 100x30");
            let mut pairs = 0usize;
            for y in (header_row + 1)..=(header_row + 2) {
                let mut line = String::new();
                for x in 0..buf.area().width {
                    line.push_str(buf[(x, y)].symbol());
                }
                pairs += line.matches("██").count();
            }
            pairs
        }

        let engine_demo = hadal_core::Engine::demo();
        let engine_new = hadal_core::Engine::new();
        let anim = crate::render::Anim::settled(engine_demo.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );

        let ui_demo = crate::render::UiState::settled(
            engine_demo.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );
        let ui_new = crate::render::UiState::settled(
            engine_new.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );

        let pairs_demo = slot_pairs(&ui_demo);
        let pairs_new = slot_pairs(&ui_new);
        assert_eq!(
            pairs_demo,
            engine_demo.view().hull_count(),
            "owned slot pairs must equal the owned hull count"
        );
        assert_eq!(
            pairs_new,
            engine_new.view().hull_count(),
            "owned slot pairs must equal the owned hull count"
        );
        assert!(
            pairs_demo > pairs_new,
            "demo should light more slots ({pairs_demo}) than new ({pairs_new})"
        );
    }

    #[test]
    fn hull_schematic_hidden_below_100() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        fn screen_text(width: u16, height: u16, ui: &crate::render::UiState<'_>) -> String {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        }

        let engine = hadal_core::Engine::demo();
        let anim = crate::render::Anim::settled(engine.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );
        let ui = crate::render::UiState::settled(
            engine.view(),
            pal,
            false,
            crate::render::Screen::Descent,
            &anim,
        );

        let text_80 = screen_text(80, 24, &ui);
        let text_100 = screen_text(100, 30, &ui);

        assert!(
            !text_80.contains("HULL SCHEMATIC"),
            "80x24 should not contain HULL SCHEMATIC"
        );
        assert!(
            text_100.contains("HULL SCHEMATIC"),
            "100x30 should contain HULL SCHEMATIC"
        );
    }
}
