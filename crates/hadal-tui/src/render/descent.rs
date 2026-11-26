//! The DESCENT screen: depth gauge, descent card, hull tree, governor, confirm.

use hadal_core::Decimal;
use hadal_core::StateView;
use hadal_core::content::ids::HullId;
use hadal_core::content::tables;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::glyphs::{self, Glyphs};
use crate::model;
use crate::render::{
    Focus, HitMap, UiState, amount, bar, cost, count, overlay_box, rate_text, render_rule,
    render_tabs, titled_frame, window_start,
};
use crate::theme::Palette;

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
    );
    let content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let mut hits = HitMap {
        tabs,
        ..HitMap::default()
    };

    let top_height = (content.height / 3).clamp(9, 10);
    let rows = Layout::vertical([
        Constraint::Length(top_height),
        Constraint::Length(1), // rule
        Constraint::Min(0),    // hull tree
        Constraint::Length(1), // rule
        Constraint::Length(1), // governor
        Constraint::Length(1), // notice
        Constraint::Length(1), // footer
    ])
    .areas::<7>(content);

    render_top(frame, rows[0], ui, g);
    render_rule(frame, rows[1], &p, g);
    render_hull_tree(frame, rows[2], ui, g, &mut hits);
    render_rule(frame, rows[3], &p, g);
    render_governor(frame, rows[4], ui);
    render_notice(frame, rows[5], ui);
    render_footer(frame, rows[6], &p, g);

    if ui.confirm_descent {
        render_confirmation(frame, ui);
    }
    hits
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

    render_gauge(frame, cols[0], ui, g);

    let divider: Vec<Line> = (0..cols[1].height)
        .map(|_| Line::from(Span::styled(g.v_line.to_string(), dim(ui.palette))))
        .collect();
    frame.render_widget(Paragraph::new(Text::from(divider)), cols[1]);

    render_card(frame, cols[2], ui, g);
}

fn render_gauge(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    if area.height < 3 {
        return;
    }
    let p = ui.palette;
    let depth = ui.view.depth_m();
    let min_depth = tables::BASE_DEPTH_M;
    let max_depth = 20_000u32;

    let rows = area.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    lines.push(Line::from(Span::styled("DEPTH GAUGE", dim(p))));

    let track_top = 1usize;
    let track_bottom = rows.saturating_sub(1);
    let span = track_bottom.saturating_sub(track_top);
    let fraction = if max_depth > min_depth {
        (f64::from(depth.saturating_sub(min_depth)) / f64::from(max_depth - min_depth))
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let marker = track_bottom.saturating_sub((fraction * span as f64).round() as usize);

    for row in track_top..=track_bottom {
        let is_marker = row == marker;
        let mark = if is_marker { g.depth_gauge } else { g.v_line };
        let label = if row == track_top {
            format!("{:>6}", count(max_depth))
        } else if row == track_bottom {
            format!("{:>6}", count(min_depth))
        } else if is_marker {
            format!("{:>6}", count(depth))
        } else {
            "      ".to_owned()
        };
        let style = if is_marker { bold_amber(p) } else { dim(p) };
        lines.push(Line::from(vec![
            Span::styled(format!(" {mark} "), style),
            Span::styled(label, style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
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
    let start = window_start(rows.len(), visible, hull_focus_row(ui.focus));
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

    let marker = if focused { ">" } else { " " };
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
            if focused { bold_amber(p) } else { text(p) },
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

fn render_footer(frame: &mut Frame, area: Rect, p: &Palette, g: &Glyphs) {
    let p = *p;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "{}{} focus   Enter buy   p confirm descent   +/- target   Esc back   1 sonar",
                g.arrow_up, g.arrow_down
            ),
            dim(p),
        ))),
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
}
