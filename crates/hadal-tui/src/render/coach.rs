//! The interactive tutorial coach: a five-row band above the screen footer.

/// Reserved coach band height.
pub const HEIGHT: u16 = 5;

use crate::render::HitMap;
use hadal_core::Decimal;
use hadal_core::content::tutorial::{TutorialStepId, TutorialTarget};
use hadal_core::engine::TutorialProgress;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::glyphs;
use crate::model;
use crate::render::{Tab, UiState, amount, bar, border_set, overlay_box};
use crate::theme::Palette;

/// The reserved coach band inside the frame, above the footer row.
pub fn band_rect(ui: &UiState<'_>, area: ratatui::layout::Rect) -> Option<Rect> {
    if !ui.coach_active() || area.width < 20 || area.height < 12 {
        return None;
    }
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.height <= HEIGHT {
        return None;
    }
    let y = inner
        .y
        .saturating_add(inner.height)
        .saturating_sub(1)
        .saturating_sub(HEIGHT);
    Some(Rect::new(inner.x, y, inner.width, HEIGHT))
}

/// Gutter marker in the active glyph set.
pub fn marker(ascii: bool) -> &'static str {
    if ascii { ">" } else { "▶" }
}

/// True when the current coach target equals `target`.
pub fn target_is(ui: &UiState<'_>, target: TutorialTarget) -> bool {
    ui.coach_target() == Some(target)
}

/// The tab that owns a tutorial target's surface.
///
/// `None` for targets that live in the current screen's footer rather than a
/// tab (`HelpKey`) and for `None`.
fn owner_tab(target: TutorialTarget) -> Option<Tab> {
    match target {
        TutorialTarget::PulseBar
        | TutorialTarget::Generator(_)
        | TutorialTarget::ChainPanel
        | TutorialTarget::TuneStrip
        | TutorialTarget::PeakReadout => Some(Tab::Sonar),
        TutorialTarget::Hull(_) | TutorialTarget::DescentTab => Some(Tab::Descent),
        TutorialTarget::SystemTab => Some(Tab::System),
        TutorialTarget::HelpKey | TutorialTarget::None => None,
    }
}

/// Short tab label used in the `TARGET <TAB> TAB` header.
fn tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Sonar => "SONAR",
        Tab::Descent => "DESCENT",
        Tab::Archive => "ARCHIVE",
        Tab::System => "SYSTEM",
    }
}

/// True when the current screen is not the target's owner tab.
fn target_offscreen(ui: &UiState<'_>, target: TutorialTarget) -> bool {
    match owner_tab(target) {
        Some(tab) => ui.screen.tab() != Some(tab),
        None => false,
    }
}

/// The tab that should receive the coach marker.
///
/// Tab targets always mark their tab; item targets mark their owner tab only
/// when the player is on a different screen (so the tab is the way to get
/// there).
pub fn target_tab(ui: &UiState<'_>) -> Option<Tab> {
    match ui.coach_target()? {
        TutorialTarget::DescentTab => Some(Tab::Descent),
        TutorialTarget::SystemTab => Some(Tab::System),
        TutorialTarget::Hull(_) => {
            if matches!(ui.screen, crate::render::Screen::Descent) {
                None
            } else {
                Some(Tab::Descent)
            }
        }
        TutorialTarget::PulseBar
        | TutorialTarget::Generator(_)
        | TutorialTarget::ChainPanel
        | TutorialTarget::TuneStrip
        | TutorialTarget::PeakReadout => {
            if matches!(ui.screen, crate::render::Screen::Sonar) {
                None
            } else {
                Some(Tab::Sonar)
            }
        }
        TutorialTarget::HelpKey | TutorialTarget::None => None,
    }
}

/// Display name for the coach TARGET label.
pub fn target_name(target: TutorialTarget) -> &'static str {
    match target {
        TutorialTarget::PulseBar => "PULSE",
        TutorialTarget::Generator(hadal_core::content::ids::GeneratorId::Hydrophone) => {
            "HYDROPHONE"
        }
        TutorialTarget::Generator(hadal_core::content::ids::GeneratorId::Drifter) => "DRIFTER",
        TutorialTarget::Generator(hadal_core::content::ids::GeneratorId::SonarMast) => "SONAR MAST",
        TutorialTarget::Generator(hadal_core::content::ids::GeneratorId::TidalCore) => "TIDAL CORE",
        TutorialTarget::ChainPanel => "SIGNAL CHAIN",
        TutorialTarget::TuneStrip => "TUNE",
        TutorialTarget::PeakReadout => "PEAK",
        TutorialTarget::DescentTab => "DESCENT TAB",
        TutorialTarget::Hull(_) => "HULL AUTO-PULSE",
        TutorialTarget::SystemTab => "SYSTEM TAB",
        TutorialTarget::HelpKey => "HELP",
        TutorialTarget::None => "",
    }
}

/// A `? manual` footer span, marked when HELP is the coach target.
pub fn help_span(ui: &UiState<'_>, p: Palette) -> Span<'static> {
    if target_is(ui, TutorialTarget::HelpKey) {
        Span::styled(
            format!("{} ? manual", marker(ui.ascii)),
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("? manual", Style::new().fg(p.dim))
    }
}

fn chapter_name(chapter: u8) -> &'static str {
    hadal_core::content::tutorial::CHAPTERS
        .iter()
        .find(|entry| entry.number == chapter)
        .map_or(hadal_core::content::tutorial::CHAPTERS[0].title, |entry| {
            entry.title
        })
}

fn key_words() -> &'static [&'static str] {
    &[
        "SPACE", "h", "u", "Enter", "d", "f", "2", "p", "y", "4", "?", "Esc",
    ]
}

fn instruction_line(text: &str, p: Palette) -> Line<'static> {
    let badge = Style::new()
        .fg(p.bg)
        .bg(p.amber)
        .add_modifier(Modifier::BOLD);
    let plain = Style::new().fg(p.text);
    // Leading gutter matches the why rows.
    let mut spans = vec![Span::styled("  ", plain)];
    for (index, word) in text.split_whitespace().enumerate() {
        // Exactly one separator space before each token; punctuation attached
        // to a key word stays outside the badge.
        if index > 0 {
            spans.push(Span::styled(" ", plain));
        }
        let core = word.trim_matches(|c: char| c == ',' || c == '.' || c == ';' || c == ':');
        // Prefix is the leading punctuation only (keys never start with ':').
        let prefix: String = word
            .chars()
            .take_while(|c| matches!(c, ',' | '.' | ';' | ':'))
            .collect();
        let suffix_start = prefix.len() + core.len();
        let suffix = word.get(suffix_start..).unwrap_or("");
        if key_words().contains(&core) && !core.is_empty() {
            if !prefix.is_empty() {
                spans.push(Span::styled(prefix, plain));
            }
            spans.push(Span::styled(core.to_owned(), badge));
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix.to_owned(), plain));
            }
        } else {
            spans.push(Span::styled(word.to_owned(), plain));
        }
    }
    Line::from(spans)
}

fn wrap_two(text: &str, width: usize, ascii: bool) -> [String; 2] {
    let width = width.max(8);
    let mut lines = [String::new(), String::new()];
    let mut line = 0usize;
    let mut overflow = false;
    for word in text.split_whitespace() {
        let len = word.chars().count();
        let current = lines[line].chars().count();
        if current == 0 {
            lines[line].push_str(word);
        } else if current + 1 + len <= width {
            lines[line].push(' ');
            lines[line].push_str(word);
        } else if line == 0 {
            line = 1;
            lines[1].push_str(word);
        } else {
            overflow = true;
            break;
        }
    }
    if overflow && !lines[1].is_empty() {
        let marker = if ascii { "..." } else { "…" };
        let keep = width.saturating_sub(marker.chars().count());
        lines[1] = lines[1].chars().take(keep).collect::<String>();
        lines[1].push_str(marker);
    }
    lines
}

fn chapter_line(view: &TutorialProgress) -> String {
    format!(
        "{} {} / {}",
        view.label,
        amount(view.current),
        amount(view.target)
    )
}

fn progress_fraction(view: &TutorialProgress) -> f64 {
    model::fraction(view.current, Decimal::ZERO, view.target)
}

/// Evaluate the current step's status.
///
/// Returns `(ready, text, show_bar)`. `show_bar` is false when a progress bar
/// would be misleading (owned, or blocked on an unlock unrelated to the bar).
fn status(ui: &UiState<'_>) -> (bool, String, bool) {
    let view = ui.view;
    let step = ui.tutorial.map(|t| t.step.id);
    match step {
        Some(TutorialStepId::Pulse) => (
            view.can_pulse(),
            if view.can_pulse() {
                "SPACE pulse the capacitor".to_owned()
            } else {
                "capacitor charging".to_owned()
            },
            false,
        ),
        Some(TutorialStepId::FirstGenerator | TutorialStepId::SecondGenerator) => {
            let affordable = view.generator_affordable(
                hadal_core::content::ids::GeneratorId::Hydrophone,
                hadal_core::BuyQuantity::One,
            );
            (
                affordable,
                if affordable {
                    "h buy a Hydrophone".to_owned()
                } else {
                    format!(
                        "need {} Echo",
                        amount(view.generator_cost(
                            hadal_core::content::ids::GeneratorId::Hydrophone,
                            hadal_core::BuyQuantity::One
                        ))
                    )
                },
                false,
            )
        }
        Some(TutorialStepId::Drifter) => {
            let id = hadal_core::content::ids::GeneratorId::Drifter;
            let unlocked = view.generator_unlocked(id);
            let affordable = view.generator_affordable(id, hadal_core::BuyQuantity::One);
            (
                affordable,
                if affordable {
                    "d buy a Drifter".to_owned()
                } else if unlocked {
                    format!(
                        "need {} Echo",
                        amount(view.generator_cost(id, hadal_core::BuyQuantity::One))
                    )
                } else {
                    "reach 10 Hydrophones".to_owned()
                },
                false,
            )
        }
        Some(TutorialStepId::Upgrade) => {
            let id = hadal_core::content::ids::UpgradeId::ImpedanceMatch;
            let owned = view.upgrade_owned(id);
            let unlocked = view.upgrade_unlocked(id);
            let affordable = view.upgrade_affordable(id);
            if owned {
                (true, "owned".to_owned(), false)
            } else if !unlocked {
                (false, "build a Drifter first".to_owned(), false)
            } else if affordable {
                (true, "u then Enter buy Impedance Match".to_owned(), true)
            } else {
                (
                    false,
                    format!("need {} Echo", amount(view.upgrade_cost(id))),
                    true,
                )
            }
        }
        Some(TutorialStepId::Tune) => {
            let matched = view.carrier_matched();
            (
                !matched,
                if matched {
                    "wait for the carrier to drift".to_owned()
                } else {
                    format!("f tune to CARRIER {:02}", view.carrier_band() + 1)
                },
                false,
            )
        }
        Some(TutorialStepId::Peak) => (
            view.can_descend(),
            if view.can_descend() {
                "2 open DESCENT".to_owned()
            } else {
                "need 1.00M Peak Echo".to_owned()
            },
            true,
        ),
        Some(TutorialStepId::Descend | TutorialStepId::SecondDescent) => (
            view.can_descend(),
            if view.can_descend() {
                "2 then p then y descend".to_owned()
            } else {
                "need 1.00M Peak Echo".to_owned()
            },
            true,
        ),
        Some(TutorialStepId::Hull) => {
            let id = hadal_core::content::ids::HullId::AutoPulse;
            let owned = view.hull_owned(id);
            let prereq = view.hull_prereq_met(id);
            let affordable = view.hull_affordable(id);
            if owned {
                (true, "owned".to_owned(), false)
            } else if !prereq {
                (false, "prerequisite hull required".to_owned(), false)
            } else if affordable {
                (true, "2, then Auto-Pulse, then Enter".to_owned(), true)
            } else {
                (false, format!("need {} Fathoms", view.hull_cost(id)), true)
            }
        }
        Some(TutorialStepId::System) => (true, "4 open SYSTEM".to_owned(), false),
        Some(TutorialStepId::Manual) => (true, "? open, Esc close".to_owned(), false),
        Some(TutorialStepId::Offline) => (true, "Enter to finish the tutorial".to_owned(), false),
        None => (true, "free play".to_owned(), false),
    }
}

fn coach_lines<'a>(ui: &'a UiState<'a>, width: usize, target_name: String) -> Vec<Line<'a>> {
    let p = ui.palette;
    let Some(tutorial) = ui.tutorial else {
        return Vec::new();
    };
    let step = tutorial.step;
    let index = tutorial.index.saturating_add(1);
    let sep = if ui.ascii { " - " } else { " · " };
    let header_left = format!(
        "  TUTORIAL {:02}/13{sep}{}",
        index,
        chapter_name(tutorial.chapter)
    );
    let header_target = if target_name.is_empty() {
        String::new()
    } else {
        format!("TARGET {target_name}  ")
    };
    let header_right = "[S] SKIP ".to_owned();
    let total_right = header_target.chars().count() + header_right.chars().count();
    let pad = width
        .saturating_sub(header_left.chars().count() + total_right)
        .max(1);
    let header = Line::from(vec![
        Span::styled(
            header_left,
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad), Style::new().fg(p.text)),
        Span::styled(
            header_target,
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            header_right,
            Style::new().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
    ]);

    let instruction = instruction_line(step.instruction, p);
    let why = model::ascii_text(step.why, ui.ascii);
    let [why1, why2] = wrap_two(&why, width.saturating_sub(2), ui.ascii);

    // Row 4: status or nudge
    let status_line = if let Some(nudge) = ui.tutorial_nudge {
        Line::from(Span::styled(
            nudge,
            Style::new().fg(p.alert).add_modifier(Modifier::BOLD),
        ))
    } else {
        let (ready, action, show_bar) = status(ui);
        let mut spans = vec![
            Span::styled(
                if ready { "[READY] " } else { "WAITING " },
                if ready {
                    Style::new().fg(p.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(p.amber).add_modifier(Modifier::BOLD)
                },
            ),
            Span::styled(action, Style::new().fg(p.text)),
        ];
        if show_bar && let Some(progress) = tutorial.progress {
            spans.push(Span::styled("  ", Style::new().fg(p.text)));
            spans.push(Span::styled(
                chapter_line(&progress),
                Style::new().fg(p.text),
            ));
            spans.push(Span::styled(" ", Style::new().fg(p.text)));
            spans.push(Span::styled(
                bar(progress_fraction(&progress), 5, glyphs::set(ui.ascii)),
                Style::new().fg(p.amber),
            ));
        }
        Line::from(spans)
    };

    vec![
        header,
        instruction,
        Line::from(Span::styled(format!("  {why1}"), Style::new().fg(p.dim))),
        Line::from(Span::styled(format!("  {why2}"), Style::new().fg(p.dim))),
        status_line,
    ]
}

/// Draw the coach band above the screen footer.
pub fn render(frame: &mut Frame, ui: &UiState<'_>, hits: &HitMap) {
    let area = frame.area();
    let Some(band) = band_rect(ui, area) else {
        return;
    };
    let p = ui.palette;

    // Off-screen targets point at the owning tab: the header names the tab and
    // the border/scrim exclusion use the tab rect. On the owner screen the item
    // name and item rect are used.
    let coach_target = ui.coach_target();
    let effective_target: Option<Rect> = match coach_target {
        Some(target) if target_offscreen(ui, target) => owner_tab(target)
            .and_then(|tab| hits.tabs.iter().find(|(_, t)| *t == tab).map(|(r, _)| *r)),
        _ => hits.tutorial_target,
    };
    let display_name = match coach_target {
        Some(target) => {
            if target_offscreen(ui, target) {
                owner_tab(target)
                    .map(|tab| format!("{} TAB", tab_label(tab)))
                    .unwrap_or_default()
            } else {
                target_name(target).to_string()
            }
        }
        None => String::new(),
    };

    // --- Scrim: dim cells outside target and known interactive regions ---
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let tab_y = inner.y;
    let footer_y = inner.y.saturating_add(inner.height).saturating_sub(1);
    let expanded_target = effective_target.map(|t| {
        Rect::new(
            t.x.saturating_sub(1),
            t.y.saturating_sub(1),
            t.width.saturating_add(2),
            t.height.saturating_add(2),
        )
    });
    let buf = frame.buffer_mut();
    for y in inner.y..inner.y.saturating_add(inner.height) {
        for x in inner.x..inner.x.saturating_add(inner.width) {
            // Skip if inside any exclusion zone
            let skip = expanded_target.is_some_and(|t| contains_rect(t, x, y))
                || hits.radar.is_some_and(|r| contains_rect(r, x, y))
                || hits.wave.is_some_and(|w| contains_rect(w, x, y))
                || contains_rect(band, x, y)
                || y == tab_y
                || y == footer_y;
            if skip {
                continue;
            }
            let cell = &mut buf[(x, y)];
            cell.set_style(Style::new().fg(p.dim).bg(p.bg).add_modifier(Modifier::DIM));
        }
    }

    // --- Target border: 1-cell amber border around effective_target ---
    if let Some(target) = effective_target {
        draw_target_border(frame, target, p);
    }

    // --- Band: clear to panel, restore side borders, render lines ---
    let filler = " ".repeat(band.width as usize);
    let mut lines = Vec::with_capacity(HEIGHT as usize);
    for _ in 0..HEIGHT {
        lines.push(Line::from(Span::styled(
            filler.clone(),
            Style::new().bg(p.panel),
        )));
    }
    frame.render_widget(Paragraph::new(lines), band);

    // Restore the outer frame side borders on the band rows
    let sides = border_set(ui.ascii);
    let side_style = Style::new().fg(p.dim);
    let right_x = area.x.saturating_add(area.width.saturating_sub(1));
    for y in band.y..band.y.saturating_add(band.height) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(sides.vertical_left, side_style))),
            Rect::new(area.x, y, 1, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(sides.vertical_right, side_style))),
            Rect::new(right_x, y, 1, 1),
        );
    }

    let lines = coach_lines(ui, band.width as usize, display_name);
    let rows = Layout::vertical([Constraint::Length(1); 5]).split(band);
    for (row, line) in rows.iter().zip(lines) {
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(p.panel).fg(p.text)),
            *row,
        );
    }
}

/// Check if cell (x, y) is inside rect.
fn contains_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

/// Draw a 1-cell amber border around the tutorial target, recolouring the
/// perimeter cells while keeping their symbols.
fn draw_target_border(frame: &mut Frame, target: Rect, p: Palette) {
    let buf = frame.buffer_mut();
    let area = buf.area;
    let left = target.x;
    let right = target.x.saturating_add(target.width).saturating_sub(1);
    let top = target.y;
    let bottom = target.y.saturating_add(target.height).saturating_sub(1);
    let x0 = left.max(area.x);
    let x1 = right.min(area.x.saturating_add(area.width).saturating_sub(1));
    let y0 = top.max(area.y);
    let y1 = bottom.min(area.y.saturating_add(area.height).saturating_sub(1));
    if x0 > x1 || y0 > y1 {
        return;
    }
    let mut mark = |x: u16, y: u16| {
        let cell = &mut buf[(x, y)];
        cell.set_style(
            Style::new()
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM),
        );
        cell.set_fg(p.amber);
        cell.set_bg(p.panel);
    };
    // Top and bottom rows (a 1-cell-tall rect still marks its row edges).
    if top >= y0 && top <= y1 {
        for x in x0..=x1 {
            mark(x, top);
        }
    }
    if bottom >= y0 && bottom <= y1 && bottom != top {
        for x in x0..=x1 {
            mark(x, bottom);
        }
    }
    // Left and right columns.
    if left >= x0 && left <= x1 {
        for y in y0..=y1 {
            mark(left, y);
        }
    }
    if right >= x0 && right <= x1 && right != left {
        for y in y0..=y1 {
            mark(right, y);
        }
    }
}

/// Skip confirmation overlay.
pub fn render_skip_confirm(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let p = ui.palette;
    let sep = if ui.ascii { " | " } else { " · " };
    let inner = overlay_box(frame, area, 64, 7, p, ui.ascii, "SKIP TUTORIAL?");
    let lines = vec![
        Line::from(Span::styled(
            "You can replay it from SYSTEM (4) before your first descent.",
            Style::new().fg(p.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Y skip{sep}N/Esc keep"),
            Style::new().fg(p.dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(p.panel).fg(p.text)),
        inner,
    );
}

/// Replay confirmation overlay.
pub fn render_replay_confirm(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let p = ui.palette;
    let sep = if ui.ascii { " | " } else { " · " };
    let inner = overlay_box(frame, area, 52, 7, p, ui.ascii, "REPLAY TUTORIAL?");
    let lines = vec![
        Line::from(Span::styled(
            "Resume from your next unfinished step?",
            Style::new().fg(p.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Y replay{sep}N/Esc keep"),
            Style::new().fg(p.dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(p.panel).fg(p.text)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorDepth, Theme};

    fn palette() -> Palette {
        Palette::resolve(Theme::Abyss, ColorDepth::Auto)
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn instruction_badges_have_single_spacing() {
        let p = palette();
        let line = instruction_line(
            "Press SPACE. The capacitor is full: one pulse pays at least 10 Echo.",
            p,
        );
        let text = line_text(&line);
        let body = text.trim_start();
        assert!(body.starts_with("Press SPACE."), "got {text:?}");
        assert!(!body.contains("  "), "double space in {text:?}");

        let line = instruction_line(
            "Press u to focus SIGNAL CHAIN; Enter buys Impedance Match.",
            p,
        );
        let text = line_text(&line);
        let body = text.trim_start();
        assert!(
            body.starts_with("Press u to focus SIGNAL CHAIN; Enter buys Impedance Match."),
            "got {text:?}"
        );
        assert!(!body.contains("  "), "double space in {text:?}");

        // Punctuation attaches outside the badge, not to it.
        let line = instruction_line("Press Enter. Then 2.", p);
        let text = line_text(&line);
        assert!(text.contains("Press Enter. Then 2."), "got {text:?}");
    }
}
