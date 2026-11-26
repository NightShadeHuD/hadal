//! The SONAR console: resource band, scope, pulse, carrier, generators, chain.

use hadal_core::BuyQuantity;
use hadal_core::content::ids::{GeneratorId, UpgradeId};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::glyphs::{self, Glyphs};
use crate::model;
use crate::render::{
    Focus, HitMap, UiState, amount, bar, coach, cost, count, rate_text, render_rule,
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
fn bold_accent(p: P) -> Style {
    Style::new().fg(p.accent).add_modifier(Modifier::BOLD)
}
fn amber(p: P) -> Style {
    Style::new().fg(p.amber)
}
fn bold_amber(p: P) -> Style {
    Style::new().fg(p.amber).add_modifier(Modifier::BOLD)
}

fn flash_style(flash: bool, normal: Style, p: P) -> Style {
    if flash {
        Style::new().fg(p.amber).add_modifier(Modifier::BOLD)
    } else {
        normal
    }
}

/// Draw the SONAR screen into the full frame area.
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
        Span::styled("RELAY K-9 ", text(p)),
    ]);
    let carrier_style = if ui.anim.carrier_flash {
        bold_amber(p)
    } else {
        amber(p)
    };
    let right = Line::from(vec![
        Span::styled(format!(" DEPTH {} m", count(ui.view.depth_m())), text(p)),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled(
            format!("CARRIER {:02} ", ui.view.carrier_band() + 1),
            carrier_style,
        ),
    ]);

    let block = titled_frame(ui.ascii, &p, left, right);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 9 {
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
    let mut content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let mut hits = HitMap {
        tabs,
        ..HitMap::default()
    };
    if ui.demo && content.height >= 2 {
        let banner = Rect::new(content.x, content.y, content.width, 2);
        render_demo_banner(frame, banner, ui);
        content = Rect::new(content.x, content.y + 2, content.width, content.height - 2);
    }
    if ui.coach_active() {
        render_coach_compact(frame, content, ui, g, &mut hits);
    } else if area.width >= 100 && area.height >= 30 {
        render_wide(frame, content, ui, g, &mut hits);
    } else {
        render_narrow(frame, content, ui, g, &mut hits);
    }
    hits
}

fn render_coach_compact(
    frame: &mut Frame,
    inner: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
) {
    let rows = Layout::vertical([
        Constraint::Length(3),                            // resource band
        Constraint::Length(1),                            // pulse
        Constraint::Length(1),                            // carrier
        Constraint::Length(1),                            // generator header
        Constraint::Length(4),                            // generators
        Constraint::Min(0),                               // signal chain
        Constraint::Length(1),                            // rule above coach
        Constraint::Length(crate::render::coach::HEIGHT), // reserved coach
        Constraint::Length(1),                            // footer
    ])
    .areas::<9>(inner);

    render_band(frame, rows[0], ui, g);
    render_capacitor(frame, rows[1], ui, g);
    render_carrier(frame, rows[2], ui);
    render_generator_header(frame, rows[3], &ui.palette);
    render_generators(frame, rows[4], ui, hits);
    render_chain(frame, rows[5], ui, g, hits, false);
    render_rule(frame, rows[6], &ui.palette, g);
    render_footer(frame, rows[8], ui);

    let target = ui.coach_target();
    hits.tutorial_target = match target {
        Some(TutorialTarget::PulseBar) => Some(rows[1]),
        Some(TutorialTarget::TuneStrip) => Some(rows[2]),
        Some(TutorialTarget::PeakReadout) => Some(rows[0]),
        Some(TutorialTarget::ChainPanel) => Some(Rect::new(rows[5].x, rows[5].y, rows[5].width, 1)),
        Some(TutorialTarget::Generator(id)) => Some(Rect::new(
            rows[4].x,
            rows[4].y.saturating_add(id.index() as u16),
            rows[4].width,
            1,
        )),
        Some(TutorialTarget::HelpKey) => Some(rows[8]),
        _ => hits.tabs.iter().find_map(|(rect, tab)| {
            (Some(*tab) == crate::render::coach::target_tab(ui)).then_some(*rect)
        }),
    };
}

// ---------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------

fn render_narrow(frame: &mut Frame, inner: Rect, ui: &UiState<'_>, g: &Glyphs, hits: &mut HitMap) {
    let selection = if selection_line(ui).is_some() { 1 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(3),         // resource band
        Constraint::Length(1),         // rule
        Constraint::Length(3),         // scope
        Constraint::Length(1),         // capacitor
        Constraint::Length(1),         // carrier
        Constraint::Length(1),         // generator header
        Constraint::Length(4),         // generators
        Constraint::Length(1),         // objective
        Constraint::Length(selection), // selection explainer
        Constraint::Min(0),            // signal chain
        Constraint::Length(1),         // rule
        Constraint::Length(1),         // footer
    ])
    .areas::<12>(inner);

    render_band(frame, rows[0], ui, g);
    render_rule(frame, rows[1], &ui.palette, g);
    render_scope(frame, rows[2], ui, g);
    render_capacitor(frame, rows[3], ui, g);
    render_carrier(frame, rows[4], ui);
    render_generator_header(frame, rows[5], &ui.palette);
    render_generators(frame, rows[6], ui, hits);
    render_hint(frame, rows[7], ui, g);
    render_selection(frame, rows[8], ui);
    render_chain(frame, rows[9], ui, g, hits, false);
    render_rule(frame, rows[10], &ui.palette, g);
    render_footer(frame, rows[11], ui);
}

fn render_wide(frame: &mut Frame, inner: Rect, ui: &UiState<'_>, g: &Glyphs, hits: &mut HitMap) {
    let selection = if selection_line(ui).is_some() { 1 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(3),         // resource band
        Constraint::Length(1),         // rule
        Constraint::Min(0),            // body
        Constraint::Length(selection), // selection explainer
        Constraint::Length(1),         // objective
        Constraint::Length(1),         // rule
        Constraint::Length(1),         // footer
    ])
    .areas::<7>(inner);

    render_band(frame, rows[0], ui, g);
    render_rule(frame, rows[1], &ui.palette, g);

    let cols = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas::<3>(rows[2]);
    let (left, mid, right) = (cols[0], cols[1], cols[2]);

    let divider: Vec<Line> = (0..mid.height)
        .map(|_| Line::from(Span::styled(g.v_line.to_string(), dim(ui.palette))))
        .collect();
    frame.render_widget(Paragraph::new(Text::from(divider)), mid);

    let left_rows = Layout::vertical([
        Constraint::Min(0),    // scope
        Constraint::Length(1), // capacitor
        Constraint::Length(1), // carrier
        Constraint::Length(1), // generator header
        Constraint::Length(1), // generator caption
        Constraint::Length(4), // generators
    ])
    .areas::<6>(left);
    render_scope(frame, left_rows[0], ui, g);
    render_capacitor(frame, left_rows[1], ui, g);
    render_carrier(frame, left_rows[2], ui);
    render_generator_header(frame, left_rows[3], &ui.palette);
    render_caption(
        frame,
        left_rows[4],
        "   Costs rise with each unit.",
        &ui.palette,
    );
    render_generators(frame, left_rows[5], ui, hits);

    let right_rows =
        Layout::vertical([Constraint::Min(0), Constraint::Length(11)]).areas::<2>(right);
    render_chain(frame, right_rows[0], ui, g, hits, true);
    render_station(frame, right_rows[1], ui);

    render_selection(frame, rows[3], ui);
    render_hint(frame, rows[4], ui, g);
    render_rule(frame, rows[5], &ui.palette, g);
    render_footer(frame, rows[6], ui);
}

fn render_demo_banner(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    if area.height < 2 || area.width == 0 {
        return;
    }
    let p = ui.palette;
    let rows = Layout::vertical([Constraint::Length(1); 2]).areas::<2>(area);
    let descents = ui.view.descents();
    let hulls = ui.view.hull_count();
    let compact = area.width < 90;
    let first = if compact {
        format!("DEMO // NOTHING IS SAVED   late game: {descents} descents, {hulls} hulls")
    } else {
        format!(
            "DEMO // NOTHING IS SAVED   late game: {descents} descents, {hulls} hulls, automation is on"
        )
    };
    let second = if compact {
        "automation is on; keys additive   ? manual   1-4 screens"
    } else {
        "the station plays itself; keys are additive   ? manual   1-4 screens"
    };
    let prefix = "DEMO // NOTHING IS SAVED";
    let rest = first.strip_prefix(prefix).unwrap_or("");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                prefix.to_owned(),
                Style::new().fg(p.alert).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_owned(), dim(p)),
        ]))
        .style(Style::new().bg(p.panel)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(second, dim(p)))).style(Style::new().bg(p.panel)),
        rows[1],
    );
}

fn render_caption(frame: &mut Frame, area: Rect, text: &str, p: &Palette) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_owned(), dim(*p)))),
        area,
    );
}

// ---------------------------------------------------------------------
// Resource band
// ---------------------------------------------------------------------

fn render_band(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    if !ui.ascii && area.width >= 70 {
        let value = amount(ui.anim.echo_display);
        let (numeric, suffix) = split_numeric(&value);
        let digits_width = (numeric.chars().count() * 4).saturating_sub(1) as u16;
        let left_width = (digits_width + suffix.chars().count() as u16 + 3 + 5)
            .max(23)
            .min(area.width.saturating_sub(24));
        let cols = Layout::horizontal([Constraint::Length(left_width), Constraint::Fill(1)])
            .areas::<2>(area);
        render_block_echo(frame, cols[0], ui, numeric, suffix);
        render_band_right(frame, cols[1], ui, g);
    } else {
        let rows = Layout::vertical([Constraint::Length(1); 3]).areas::<3>(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("ECHO ", dim(p)),
                Span::styled(amount(ui.anim.echo_display), bold_accent(p)),
                Span::styled(format!("  +{}/s", rate_text(ui.view.rate())), accent(p)),
            ])),
            rows[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if coach::target_is(ui, TutorialTarget::PeakReadout) {
                        format!("{} PEAK ", coach::marker(ui.ascii))
                    } else {
                        "PEAK ".to_owned()
                    },
                    if coach::target_is(ui, TutorialTarget::PeakReadout) {
                        bold_amber(p)
                    } else {
                        dim(p)
                    },
                ),
                Span::styled(amount(ui.view.peak()), text(p)),
                Span::styled("   FATHOM ", dim(p)),
                Span::styled(count(ui.view.fathoms()), amber(p)),
            ])),
            rows[1],
        );
        render_next_row(frame, rows[2], ui, g);
    }
}

fn split_numeric(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(value.len());
    (&value[..end], &value[end..])
}

fn digit_glyph(c: char) -> [&'static str; 2] {
    const DIGITS: [[&str; 2]; 10] = [
        ["█▀█", "█▄█"],
        [" █ ", " █ "],
        ["▀▀█", "█▄▄"],
        ["▀▀█", "▄▄█"],
        ["█ █", "▀▀█"],
        ["█▀▀", "▄▄█"],
        ["█▀▀", "█▄█"],
        ["▀▀█", "  █"],
        ["█▀█", "█▄█"],
        ["█▀█", "▀▀█"],
    ];
    match c {
        '0'..='9' => DIGITS[(c as u8 - b'0') as usize],
        '.' => ["   ", " ▄ "],
        _ => ["   ", "   "],
    }
}

fn render_block_echo(frame: &mut Frame, area: Rect, ui: &UiState<'_>, numeric: &str, suffix: &str) {
    let p = ui.palette;
    let style = bold_accent(p);
    let rows = Layout::vertical([Constraint::Length(1); 3]).areas::<3>(area);

    let mut top = String::new();
    let mut bottom = String::new();
    for (i, c) in numeric.chars().enumerate() {
        if i > 0 {
            top.push(' ');
            bottom.push(' ');
        }
        let glyph = digit_glyph(c);
        top.push_str(glyph[0]);
        bottom.push_str(glyph[1]);
    }
    let top_line = Line::from(vec![
        Span::styled("ECHO ", dim(p)),
        Span::styled(top, style),
        Span::styled(
            if suffix.is_empty() {
                String::new()
            } else {
                format!(" {suffix}")
            },
            style,
        ),
    ]);
    frame.render_widget(Paragraph::new(top_line), rows[0]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("     ", dim(p)),
            Span::styled(bottom, style),
        ])),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("RATE ", dim(p)),
            Span::styled(format!("+{}/s", rate_text(ui.view.rate())), accent(p)),
        ])),
        rows[2],
    );
}

fn render_band_right(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    let rows = Layout::vertical([Constraint::Length(1); 3]).areas::<3>(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if coach::target_is(ui, TutorialTarget::PeakReadout) {
                    format!("{} PEAK ", coach::marker(ui.ascii))
                } else {
                    "PEAK   ".to_owned()
                },
                if coach::target_is(ui, TutorialTarget::PeakReadout) {
                    bold_amber(p)
                } else {
                    dim(p)
                },
            ),
            Span::styled(amount(ui.view.peak()), text(p)),
        ])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("FATHOM ", dim(p)),
            Span::styled(count(ui.view.fathoms()), amber(p)),
        ])),
        rows[1],
    );
    render_next_row(frame, rows[2], ui, g);
}

fn render_next_row(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    let progress = model::descent_progress(ui.view);
    let pct = (progress.fraction * 100.0).round() as u32;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("NEXT   ", dim(p)),
            Span::styled(amount(progress.next), text(p)),
            Span::styled("  ", dim(p)),
            Span::styled(bar(progress.fraction, 10, g), accent(p)),
            Span::styled(format!(" {pct:>3}%"), dim(p)),
        ])),
        area,
    );
}

// ---------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------

/// Fixed deterministic contacts as `(angle_radians, radius_fraction)`.
const CONTACTS: [(f64, f64); 4] = [(0.45, 0.62), (2.25, 0.82), (4.05, 0.48), (5.35, 0.88)];

fn render_scope(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = ui.palette;

    if area.height <= 3 {
        let rows = Layout::vertical([Constraint::Length(1); 3]).areas::<3>(area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("SONAR", dim(p)))),
            rows[0],
        );
        let mut spans = Vec::new();
        let sequence = [
            g.dot,
            g.scope_ring,
            g.scope_dot,
            g.scope_cross,
            g.scope_ring,
            g.scope_dot,
            g.dot,
        ];
        for (i, glyph) in sequence.iter().enumerate() {
            let style = if i == 3 {
                bold_amber(p)
            } else if i % 2 == 0 {
                dim(p)
            } else {
                accent(p)
            };
            spans.push(Span::styled(format!("{glyph} "), style));
        }
        if ui.anim.pulse_phase.is_some() {
            spans.push(Span::styled(" SWEEP", bold_amber(p)));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(ratatui::layout::Alignment::Center),
            rows[1],
        );
        return;
    }

    let w = area.width as usize;
    let h = area.height as usize;
    let cx = (w as f64 - 1.0) / 2.0;
    let cy = (h as f64 - 1.0) / 2.0;
    let max_r = (w as f64 / 2.0).min(h as f64).max(1.0) - 1.0;

    // class: 0 blank, 1 dim, 2 accent
    let mut cells = vec![vec![(' ', 0u8); w]; h];

    for (y, row) in cells.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let dx = x as f64 - cx;
            let dy = (y as f64 - cy) * 1.9;
            let distance = (dx * dx + dy * dy).sqrt();
            let mut glyph = ' ';
            let mut class = 0u8;

            if distance <= max_r {
                if (x as i32) == cx.round() as i32 {
                    glyph = g.dot;
                    class = 1;
                }
                if (y as i32) == cy.round() as i32 {
                    glyph = g.dash;
                    class = 1;
                }
            }
            for ring in 1..=3 {
                let radius = max_r * ring as f64 / 3.0;
                if (distance - radius).abs() < 0.6 {
                    glyph = g.scope_ring;
                    class = 1;
                }
            }
            if distance < 0.8 {
                glyph = g.scope_cross;
                class = 2;
            }
            *cell = (glyph, class);
        }
    }

    for (angle, fraction) in CONTACTS {
        let x = cx + angle.cos() * max_r * fraction;
        let y = cy + angle.sin() * max_r * fraction / 1.9;
        let xi = x.round() as i32;
        let yi = y.round() as i32;
        if xi >= 0 && yi >= 0 && (xi as usize) < w && (yi as usize) < h {
            cells[yi as usize][xi as usize] = (g.scope_dot, 2);
        }
    }

    if let Some(phase) = ui.anim.pulse_phase {
        let radius = max_r * phase.clamp(0.0, 1.0);
        for (y, row) in cells.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let dx = x as f64 - cx;
                let dy = (y as f64 - cy) * 1.9;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance <= max_r && (distance - radius).abs() < 0.7 {
                    *cell = (g.scope_cross, 2);
                }
            }
        }
    }

    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for row in cells {
        let mut spans: Vec<Span> = Vec::new();
        let mut run = String::new();
        let mut run_class = row[0].1;
        for (glyph, class) in row {
            if class != run_class {
                if !run.is_empty() {
                    let style = if run_class == 2 { accent(p) } else { dim(p) };
                    spans.push(Span::styled(std::mem::take(&mut run), style));
                }
                run_class = class;
            }
            run.push(glyph);
        }
        if !run.is_empty() {
            let style = if run_class == 2 { accent(p) } else { dim(p) };
            spans.push(Span::styled(run, style));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("SONAR", dim(p)))),
        Rect::new(area.x, area.y, area.width.min(5), 1),
    );
}

// ---------------------------------------------------------------------
// Pulse and carrier
// ---------------------------------------------------------------------

fn render_capacitor(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    let p = ui.palette;
    let capacity = u32::from(ui.view.pulse_capacity());
    let charge = u32::from(ui.view.pulse_charge_ticks());
    let fraction = if capacity == 0 {
        0.0
    } else {
        f64::from(charge) / f64::from(capacity)
    };
    let ready = ui.view.can_pulse();
    let remaining = capacity.saturating_sub(charge).div_ceil(10);
    let width = (area.width.saturating_sub(34) as usize).clamp(8, 32);

    let pulse_target = coach::target_is(ui, TutorialTarget::PulseBar);
    let pulse_label = if pulse_target {
        format!("{} PULSE ", coach::marker(ui.ascii))
    } else {
        "PULSE   ".to_owned()
    };
    let mut spans = vec![
        Span::styled(
            pulse_label,
            if pulse_target { bold_amber(p) } else { dim(p) },
        ),
        Span::styled(
            bar(fraction, width, g),
            if ready { accent(p) } else { text(p) },
        ),
        Span::styled(
            format!(" {:>3}%", (fraction * 100.0).round() as u32),
            text(p),
        ),
    ];
    if ready {
        spans.push(Span::styled("  READY", bold_accent(p)));
    } else {
        spans.push(Span::styled(format!("  CHARGING {remaining}s"), amber(p)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_carrier(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let carrier = ui.view.carrier_band();
    let tuned = ui.view.tuned_band();
    let matched = ui.view.carrier_matched();
    let flash = ui.anim.carrier_flash;

    let tune_target = coach::target_is(ui, TutorialTarget::TuneStrip);
    let tune_label = if tune_target {
        format!("{} TUNE  ", coach::marker(ui.ascii))
    } else {
        "TUNE    ".to_owned()
    };
    let mut spans = vec![Span::styled(
        tune_label,
        if tune_target { bold_amber(p) } else { dim(p) },
    )];
    for band in 0..6u8 {
        let number = band + 1;
        let label = if band == tuned {
            format!("[{number}*]")
        } else {
            format!("[{number}]")
        };
        let style = if flash {
            bold_amber(p)
        } else if band == tuned {
            if matched { bold_amber(p) } else { text(p) }
        } else if band == carrier {
            bold_accent(p)
        } else {
            dim(p)
        };
        spans.push(Span::styled(label, style));
    }
    spans.push(Span::styled(
        if matched { "  TUNED" } else { "  TUNING" },
        if matched { bold_amber(p) } else { dim(p) },
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------

fn render_generator_header(frame: &mut Frame, area: Rect, p: &Palette) {
    let p = *p;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "  {:<4}{:<10}{:>6} {:>7} {:>13} STATE",
                "", "GENERATORS", "OWNED", "COST", "OUTPUT"
            ),
            dim(p),
        ))),
        area,
    );
}

fn render_generators(frame: &mut Frame, area: Rect, ui: &UiState<'_>, hits: &mut HitMap) {
    let rows = Layout::vertical([Constraint::Length(1); 4]).areas::<4>(area);
    let show_requirement = area.width >= 60;
    for (index, id) in GeneratorId::ALL.iter().enumerate() {
        render_generator(frame, rows[index], *id, ui, show_requirement, hits);
    }
}

fn render_generator(
    frame: &mut Frame,
    area: Rect,
    id: GeneratorId,
    ui: &UiState<'_>,
    show_requirement: bool,
    hits: &mut HitMap,
) {
    hits.generators.push((area, id));
    let p = ui.palette;
    let view = ui.view;
    let owned = view.generator_owned(id);
    let unlocked = view.generator_unlocked(id);
    let affordable = view.generator_affordable(id, BuyQuantity::One);
    let max_affordable = view.generator_affordable(id, BuyQuantity::Max);
    let cost_text = cost(view.generator_cost(id, BuyQuantity::One));
    let flash = ui.anim.row_flash[id.index()];

    let (state, state_style) = if !unlocked {
        ("LOCK", dim(p))
    } else if max_affordable {
        ("MAX", bold_accent(p))
    } else if affordable {
        ("READY", accent(p))
    } else {
        ("NEED", dim(p))
    };

    let key = model::generator_key(id);
    let key_hint = format!("{key}/{}", key.to_ascii_uppercase());
    let targeted = coach::target_is(ui, TutorialTarget::Generator(id));
    let gutter = if targeted {
        format!("{} ", coach::marker(ui.ascii))
    } else {
        "  ".to_owned()
    };
    let output = format!("+{}/s", rate_text(model::generator_contribution(view, id)));
    let mut spans = vec![
        Span::styled(gutter, if targeted { bold_amber(p) } else { dim(p) }),
        Span::styled(
            format!("{key_hint:<4}"),
            flash_style(flash, if targeted { bold_amber(p) } else { accent(p) }, p),
        ),
        Span::styled(
            format!("{:<10}", model::generator_name(id)),
            flash_style(flash, if targeted { bold_amber(p) } else { text(p) }, p),
        ),
        Span::styled(format!("{owned:>6}"), flash_style(flash, text(p), p)),
        Span::styled(
            format!(" {cost_text:>7}"),
            flash_style(
                flash,
                if affordable || max_affordable {
                    accent(p)
                } else {
                    text(p)
                },
                p,
            ),
        ),
        Span::styled(format!(" {output:>13}"), flash_style(flash, accent(p), p)),
        Span::styled(format!(" {state}"), flash_style(flash, state_style, p)),
    ];
    if show_requirement
        && !unlocked
        && let Some(requirement) = model::generator_requirement_text(id)
    {
        spans.push(Span::styled(
            format!("  need {requirement}"),
            flash_style(flash, dim(p), p),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------
// Signal Chain
// ---------------------------------------------------------------------

fn render_chain(
    frame: &mut Frame,
    area: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
    explainer: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = ui.palette;
    let focused = matches!(ui.focus, Focus::Upgrade(_));
    let hint = if focused {
        format!(
            "   {}{} select  Enter buy  Esc exit",
            g.arrow_up, g.arrow_down
        )
    } else {
        "   u focus".to_owned()
    };
    let chain_title = if coach::target_is(ui, TutorialTarget::ChainPanel) {
        format!("{} SIGNAL CHAIN", coach::marker(ui.ascii))
    } else {
        "SIGNAL CHAIN".to_owned()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(chain_title, bold_amber(p)),
            Span::styled(hint, dim(p)),
        ])),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let header = if explainer { 2 } else { 1 };
    if explainer && area.height >= 2 {
        render_caption(
            frame,
            Rect::new(area.x, area.y + 1, area.width, 1),
            "   One-time this run; Descent resets them.",
            &p,
        );
    }

    let visible = area.height.saturating_sub(header) as usize;
    if visible == 0 {
        return;
    }
    let focus = match ui.focus {
        Focus::Upgrade(index) => Some(index),
        _ => None,
    };
    let start = window_start(UpgradeId::ALL.len(), visible, focus);
    for (offset, index) in (start..UpgradeId::ALL.len()).take(visible).enumerate() {
        let rect = Rect::new(area.x, area.y + header + offset as u16, area.width, 1);
        hits.upgrades.push((rect, index));
        render_upgrade_row(frame, rect, index, ui);
    }
}

fn render_upgrade_row(frame: &mut Frame, area: Rect, index: usize, ui: &UiState<'_>) {
    let p = ui.palette;
    let view = ui.view;
    let id = UpgradeId::ALL[index];
    let owned = view.upgrade_owned(id);
    let unlocked = view.upgrade_unlocked(id);
    let affordable = view.upgrade_affordable(id);
    let focused = matches!(ui.focus, Focus::Upgrade(focus) if focus == index);

    let marker = if focused { ">" } else { " " };
    let state = if owned {
        "OWNED"
    } else if !unlocked {
        "LOCK"
    } else if affordable {
        "READY"
    } else {
        "NEED"
    };

    let compact = area.width < 60;
    let effect = if compact {
        model::upgrade_effect_compact(id, ui.ascii)
    } else {
        model::upgrade_effect_text(id, ui.ascii)
    };
    let detail = if owned || unlocked {
        effect
    } else {
        match model::upgrade_requirement(id) {
            Some(requirement) => {
                let sep = if ui.ascii { " . " } else { " · " };
                if compact {
                    format!("{requirement}{sep}{effect}")
                } else {
                    format!("need {requirement}{sep}{effect}")
                }
            }
            None => effect,
        }
    };

    let name_style = if focused { bold_amber(p) } else { text(p) };
    let state_style = if owned {
        accent(p)
    } else if !unlocked {
        dim(p)
    } else if affordable {
        bold_accent(p)
    } else {
        dim(p)
    };

    let line = Line::from(vec![
        Span::styled(
            format!("{marker} "),
            if focused { bold_amber(p) } else { dim(p) },
        ),
        Span::styled(format!("{:<17}", model::upgrade_name(id)), name_style),
        Span::styled(
            format!(" {:>7}", cost(view.upgrade_cost(id))),
            if affordable { accent(p) } else { text(p) },
        ),
        Span::styled(format!(" {state:<5}"), state_style),
        Span::styled(format!(" {detail}"), dim(p)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------
// Station, hint, footer
// ---------------------------------------------------------------------

fn render_station(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let view = ui.view;
    let tuned = if view.carrier_matched() {
        "TUNED"
    } else {
        "MISMATCH"
    };
    let charge_fraction = view.pulse_charge_fraction();
    let pulse_state = if view.can_pulse() {
        "READY".to_owned()
    } else {
        format!("CHARGING {}%", (charge_fraction * 100.0).round() as u32)
    };
    let lines = vec![
        Line::from(Span::styled("STATION", bold_amber(p))),
        kv("DEPTH", format!("{} m", count(view.depth_m())), p),
        kv("CARRIER", format!("{:02}", view.carrier_band() + 1), p),
        kv("TUNED", format!("{:02}  {tuned}", view.tuned_band() + 1), p),
        kv("RATE", format!("{}/s", rate_text(view.rate())), p),
        kv("PEAK", amount(view.peak()), p),
        kv("BEST PEAK", amount(view.best_peak()), p),
        kv("OFFLINE CAP", format!("{} h", view.offline_cap_hours()), p),
        kv("DESCENTS", count(view.descents()), p),
        kv("ACHIEVEMENTS", count(view.achievement_count() as u32), p),
        kv("PULSE", pulse_state, p),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn kv(label: &str, value: String, p: P) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), dim(p)),
        Span::styled(value, text(p)),
    ])
}

fn render_hint(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    if ui.coach_active() {
        return;
    }
    let p = ui.palette;
    let hint = model::objective_line(ui.view, ui.ascii);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{} ", g.chevron_right), amber(p)),
            Span::styled(hint, text(p)),
        ])),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("SPACE pulse  f tune  u chain  2 descent  ", dim(p)),
            coach::help_span(ui, p),
            Span::styled("  Ctrl+S save  q quit", dim(p)),
        ])),
        area,
    );
}

/// Expose the state used by the resource band for unit tests.
#[cfg(test)]
pub(crate) fn numeric_split(value: &str) -> (&str, &str) {
    split_numeric(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_split_extracts_suffix() {
        assert_eq!(numeric_split("0"), ("0", ""));
        assert_eq!(numeric_split("12.0B"), ("12.0", "B"));
        assert_eq!(numeric_split("1.00e36"), ("1.00", "e36"));
        assert_eq!(numeric_split("999"), ("999", ""));
    }

    #[test]
    fn block_digits_support_period() {
        assert_eq!(digit_glyph('.'), ["   ", " ▄ "]);
        assert_eq!(digit_glyph('0'), ["█▀█", "█▄█"]);
    }

    #[test]
    fn contributions_sum_below_total_rate() {
        use hadal_core::Decimal;

        let engine = hadal_core::Engine::demo();
        let view = engine.view();
        let mut sum = Decimal::ZERO;
        let mut any = false;
        for id in GeneratorId::ALL {
            let contribution = model::generator_contribution(view, *id);
            assert!(contribution.mantissa().is_finite());
            assert!(contribution >= Decimal::ZERO);
            if view.generator_owned(*id) > 0 {
                assert!(contribution > Decimal::ZERO);
                any = true;
            }
            sum = sum.add(contribution);
        }
        assert!(any);
        assert!(
            sum < view.rate(),
            "sum {sum} should be below total rate {}",
            view.rate()
        );
    }

    #[test]
    fn zero_owned_generators_contribute_zero() {
        use hadal_core::Decimal;

        let engine = hadal_core::Engine::new();
        for id in GeneratorId::ALL {
            assert_eq!(
                model::generator_contribution(engine.view(), *id),
                Decimal::ZERO
            );
        }
    }
}
