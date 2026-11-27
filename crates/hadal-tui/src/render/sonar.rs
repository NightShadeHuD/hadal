//! The SONAR console: graphical radar/wave top, generators, chain, ticker.

use hadal_core::BuyQuantity;
use hadal_core::content::ids::{GeneratorId, UpgradeId};
use hadal_core::content::tutorial::TutorialTarget;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use super::{big, radar, spark, wave};
use crate::glyphs::{self, Glyphs};
use crate::model;
use crate::render::{
    Focus, HitMap, UiState, amount, bar, coach, cost, count, rate_text, render_selection,
    render_tabs, selection_line, titled_frame, window_start,
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

/// `(top_height, left_column_width, wave_height)` for the screen width.
///
/// At 80 columns the coach band costs a row, so the hero disc is 4 rows while
/// the tutorial is active and 5 rows once it is done.
fn top_metrics(screen_w: u16, coach_active: bool) -> (u16, u16, u16) {
    if screen_w < 100 {
        if coach_active { (7, 34, 3) } else { (8, 34, 3) }
    } else if screen_w < 140 {
        (12, 48, 3)
    } else {
        (18, 68, 4)
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
        coach::target_tab(ui),
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

    let screen_w = area.width;
    if ui.coach_active() {
        layout_with_coach(frame, content, ui, g, &mut hits, screen_w);
    } else {
        layout_full(frame, content, ui, g, &mut hits, screen_w);
    }
    hits
}

/// Post-tutorial layout: body (top + generators + chain), then objective,
/// optional selection, ticker, and footer.
fn layout_full(
    frame: &mut Frame,
    content: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
    screen_w: u16,
) {
    let selection = usize::from(screen_w >= 100 && selection_line(ui).is_some());
    let reserved = 3 + selection as u16; // footer + ticker + objective (+ selection)
    let body_h = content.height.saturating_sub(reserved);
    let rows = Layout::vertical([
        Constraint::Length(body_h),
        Constraint::Length(1),                // objective
        Constraint::Length(selection as u16), // selection explainer
        Constraint::Length(1),                // ticker
        Constraint::Length(1),                // footer
    ])
    .areas::<5>(content);

    let (top_h, _, _) = top_metrics(screen_w, false);
    let body = Layout::vertical([
        Constraint::Length(top_h),
        Constraint::Length(5), // generator header + 4 rows
        Constraint::Min(0),    // signal chain
    ])
    .areas::<3>(rows[0]);

    let top = render_top(frame, body[0], ui, g, hits, screen_w, false);
    let genrows =
        Layout::vertical([Constraint::Length(1), Constraint::Length(4)]).areas::<2>(body[1]);
    render_generator_header(frame, genrows[0], &ui.palette);
    render_generators(frame, genrows[1], ui, hits);
    let chain = body[2];
    render_chain(frame, chain, ui, g, hits);

    render_hint(frame, rows[1], ui, g);
    render_selection(frame, rows[2], ui);
    render_ticker(frame, rows[3], ui);
    render_footer(frame, rows[4], ui);

    set_tutorial_target(hits, ui, top, chain, genrows[1], rows[4]);
}

/// Tutorial layout: body (top + generators + chain + ticker), reserved coach
/// band, footer.
fn layout_with_coach(
    frame: &mut Frame,
    content: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
    screen_w: u16,
) {
    let body_h = content.height.saturating_sub(coach::HEIGHT + 1);
    let rows = Layout::vertical([
        Constraint::Length(body_h),
        Constraint::Length(coach::HEIGHT), // reserved for coach::render
        Constraint::Length(1),             // footer
    ])
    .areas::<3>(content);

    let (top_h, _, _) = top_metrics(screen_w, true);
    let body = Layout::vertical([
        Constraint::Length(top_h),
        Constraint::Length(5),
        Constraint::Min(0),    // signal chain
        Constraint::Length(1), // ticker
    ])
    .areas::<4>(rows[0]);

    let top = render_top(frame, body[0], ui, g, hits, screen_w, true);
    let genrows =
        Layout::vertical([Constraint::Length(1), Constraint::Length(4)]).areas::<2>(body[1]);
    render_generator_header(frame, genrows[0], &ui.palette);
    render_generators(frame, genrows[1], ui, hits);
    let chain = body[2];
    render_chain(frame, chain, ui, g, hits);
    render_ticker(frame, body[3], ui);
    render_footer(frame, rows[2], ui);

    set_tutorial_target(hits, ui, top, chain, genrows[1], rows[2]);
}

fn render_top(
    frame: &mut Frame,
    area: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    hits: &mut HitMap,
    screen_w: u16,
    coach_active: bool,
) -> RightHits {
    let p = ui.palette;
    let (_, left_w, wave_h) = top_metrics(screen_w, coach_active);
    let cols = Layout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(1), // divider
        Constraint::Length(1), // gap after the divider
        Constraint::Fill(1),   // right content
    ])
    .areas::<4>(area);
    let (left, divider, _gap, right) = (cols[0], cols[1], cols[2], cols[3]);

    // Vertical divider between the disc and the readouts.
    let lines: Vec<Line> = (0..divider.height)
        .map(|_| Line::from(Span::styled(g.v_line.to_string(), dim(p))))
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), divider);

    // Left column: radar disc over the waveform.
    let left_rows = Layout::vertical([
        Constraint::Length(area.height.saturating_sub(wave_h)),
        Constraint::Length(wave_h),
    ])
    .areas::<2>(left);
    radar::render(frame, left_rows[0], ui);
    wave::render(frame, left_rows[1], ui);
    hits.radar = Some(left_rows[0]);
    hits.wave = Some(left_rows[1]);

    // Right column: echo block, rate, spark/pulse, stats.
    render_right(frame, right, ui, g, screen_w)
}

struct RightHits {
    pulse: Rect,
    peak: Rect,
}

fn render_right(
    frame: &mut Frame,
    area: Rect,
    ui: &UiState<'_>,
    g: &Glyphs,
    screen_w: u16,
) -> RightHits {
    if screen_w < 100 {
        let rows = Layout::vertical([
            Constraint::Length(3), // echo
            Constraint::Length(1), // rate
            Constraint::Length(1), // pulse
            Constraint::Length(1), // NEXT
            Constraint::Length(1), // peak + fathom
        ])
        .areas::<5>(area);
        big::render(frame, rows[0], ui, 3);
        render_rate(frame, rows[1], ui);
        render_capacitor(frame, rows[2], ui, g);
        render_next(frame, rows[3], ui, g);
        render_peak_fathom(frame, rows[4], ui);
        // Post-tutorial the column is one row taller; use the spare row.
        if area.height > 7 {
            let status = Rect::new(area.x, area.y.saturating_add(7), area.width, 1);
            let label = if ui.view.carrier_matched() {
                "SIGNAL TUNED"
            } else {
                "SIGNAL LOCKED"
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    label.to_owned(),
                    if ui.view.carrier_matched() {
                        Style::new().fg(ui.palette.accent)
                    } else {
                        dim(ui.palette)
                    },
                ))),
                status,
            );
        }
        RightHits {
            pulse: rows[2],
            peak: rows[4],
        }
    } else if screen_w < 140 {
        let rows = Layout::vertical([
            Constraint::Length(5), // echo
            Constraint::Length(1), // rate
            Constraint::Length(1), // spark
            Constraint::Length(1), // pulse
            Constraint::Length(4), // stats
        ])
        .areas::<5>(area);
        big::render(frame, rows[0], ui, 5);
        render_rate(frame, rows[1], ui);
        spark::render(frame, rows[2], ui);
        render_capacitor(frame, rows[3], ui, g);
        render_stats(frame, rows[4], ui, g, false);
        RightHits {
            pulse: rows[3],
            peak: Rect::new(rows[4].x, rows[4].y, rows[4].width, 1),
        }
    } else {
        let rows = Layout::vertical([
            Constraint::Length(5), // echo
            Constraint::Length(1), // rate
            Constraint::Length(1), // spark
            Constraint::Length(1), // pulse
            Constraint::Length(5), // stats
            Constraint::Length(4), // breakdown
            Constraint::Length(1), // spacer
        ])
        .areas::<7>(area);
        big::render(frame, rows[0], ui, 5);
        render_rate(frame, rows[1], ui);
        spark::render(frame, rows[2], ui);
        render_capacitor(frame, rows[3], ui, g);
        render_stats(frame, rows[4], ui, g, true);
        render_breakdown(frame, rows[5], ui);
        RightHits {
            pulse: rows[3],
            peak: Rect::new(rows[4].x, rows[4].y, rows[4].width, 1),
        }
    }
}

fn render_rate(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("RATE ".to_owned(), dim(p)),
            Span::styled(format!("+{}/s", rate_text(ui.view.rate())), accent(p)),
        ])),
        area,
    );
}

fn next_line(ui: &UiState<'_>, g: &Glyphs) -> Line<'static> {
    let p = ui.palette;
    let progress = model::descent_progress(ui.view);
    let pct = (progress.fraction * 100.0).round() as u32;
    Line::from(vec![
        Span::styled("NEXT    ".to_owned(), dim(p)),
        Span::styled(amount(progress.next), text(p)),
        Span::styled("  ".to_owned(), dim(p)),
        Span::styled(bar(progress.fraction, 10, g), accent(p)),
        Span::styled(format!(" {pct:>3}%"), dim(p)),
    ])
}

fn render_next(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs) {
    frame.render_widget(Paragraph::new(next_line(ui, g)), area);
}

fn render_peak_fathom(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let target = coach::target_is(ui, TutorialTarget::PeakReadout);
    let label = if target {
        format!("{} PEAK ", coach::marker(ui.ascii))
    } else {
        "PEAK ".to_owned()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, if target { bold_amber(p) } else { dim(p) }),
            Span::styled(amount(ui.view.peak()), text(p)),
            Span::styled("  FATHOM ".to_owned(), dim(p)),
            Span::styled(count(ui.view.fathoms()), amber(p)),
        ])),
        area,
    );
}

fn render_stats(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs, with_best: bool) {
    let p = ui.palette;
    let mut lines = vec![
        kv("PEAK", amount(ui.view.peak()), p),
        kv("FATHOM", count(ui.view.fathoms()), p),
        next_line(ui, g),
        kv("DESCENT GAIN", format!("+{}", ui.view.descent_gain()), p),
    ];
    if with_best {
        lines.push(kv("BEST PEAK", amount(ui.view.best_peak()), p));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_breakdown(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let bottom = area.y.saturating_add(area.height);
    for (i, id) in GeneratorId::ALL.iter().enumerate() {
        let y = area.y.saturating_add(i as u16);
        if y >= bottom {
            break;
        }
        let contribution = model::generator_contribution(ui.view, *id);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{:<12}", model::generator_name(*id)), dim(p)),
                Span::styled(format!("+{}/s", rate_text(contribution)), accent(p)),
            ])),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

fn kv(label: &str, value: String, p: P) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), dim(p)),
        Span::styled(value, text(p)),
    ])
}

/// Record the coach target rect for the current step (None when inactive).
fn set_tutorial_target(
    hits: &mut HitMap,
    ui: &UiState<'_>,
    top: RightHits,
    chain: Rect,
    gen_rect: Rect,
    footer: Rect,
) {
    let wave_rect = hits.wave;
    let chain_title = Rect::new(chain.x, chain.y, chain.width, 1);
    hits.tutorial_target = match ui.coach_target() {
        Some(TutorialTarget::PulseBar) => Some(top.pulse),
        Some(TutorialTarget::TuneStrip) => wave_rect,
        Some(TutorialTarget::PeakReadout) => Some(top.peak),
        Some(TutorialTarget::ChainPanel) => Some(Rect::new(
            chain_title.x,
            chain_title.y,
            chain_title.width,
            chain.height.min(2),
        )),
        Some(TutorialTarget::Generator(id)) => Some(Rect::new(
            gen_rect.x,
            gen_rect.y.saturating_add(id.index() as u16),
            gen_rect.width,
            1,
        )),
        Some(TutorialTarget::HelpKey) => Some(footer),
        _ => hits
            .tabs
            .iter()
            .find_map(|(rect, tab)| (Some(*tab) == coach::target_tab(ui)).then_some(*rect)),
    };
}

fn render_ticker(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = ui.palette;
    let text = ui.toasts.last().map(String::as_str).unwrap_or("LISTENING");
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_owned(), dim(p)))),
        area,
    );
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
        " automation is on; keys additive   ? manual   1-4 screens"
    } else {
        " the station plays itself; keys are additive   ? manual   1-4 screens"
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

fn render_chain(frame: &mut Frame, area: Rect, ui: &UiState<'_>, g: &Glyphs, hits: &mut HitMap) {
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

    let visible = area.height.saturating_sub(1) as usize;
    if visible == 0 {
        return;
    }
    let focus = match ui.focus {
        Focus::Upgrade(index) => Some(index),
        _ => None,
    };
    let start = window_start(UpgradeId::ALL.len(), visible, focus);
    for (offset, index) in (start..UpgradeId::ALL.len()).take(visible).enumerate() {
        let rect = Rect::new(area.x, area.y + 1 + offset as u16, area.width, 1);
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
// Hint and footer
// ---------------------------------------------------------------------

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

/// Split a numeric string into the numeric part and suffix (e.g. "1,234M" → ("1,234", "M")).
pub(crate) fn split_numeric(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(value.len());
    (&value[..end], &value[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

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
