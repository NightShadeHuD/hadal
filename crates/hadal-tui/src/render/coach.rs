//! The interactive tutorial coach: a five-row band above the screen footer.

/// Reserved coach band height.
pub const HEIGHT: u16 = 5;

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

/// The tab that should receive the coach marker.
pub fn target_tab(ui: &UiState<'_>) -> Option<Tab> {
    match ui.coach_target()? {
        TutorialTarget::DescentTab => Some(Tab::Descent),
        TutorialTarget::SystemTab => Some(Tab::System),
        TutorialTarget::Hull(_) if matches!(ui.screen, crate::render::Screen::Sonar) => {
            Some(Tab::Descent)
        }
        _ => None,
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
    match chapter {
        1 => "FIRST SIGNAL",
        _ => "SECOND SIGNAL",
    }
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
    let mut spans = vec![Span::styled("  ", Style::new().fg(p.text))];
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" ", Style::new().fg(p.text)));
        }
        let key = word.trim_matches(|c: char| c == ',' || c == '.' || c == ';' || c == ':');
        if key_words().contains(&key) && !key.is_empty() {
            let prefix = &word[..word.find(key).unwrap_or(0)];
            let suffix = &word[key.len() + prefix.len()..];
            if !prefix.is_empty() {
                spans.push(Span::styled(prefix.to_owned(), Style::new().fg(p.text)));
            }
            spans.push(Span::styled(format!(" {key} "), badge));
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix.to_owned(), Style::new().fg(p.text)));
            }
        } else {
            spans.push(Span::styled(word.to_owned(), Style::new().fg(p.text)));
        }
    }
    Line::from(spans)
}

fn capitalized_after(text: &str, prefix: &str) -> String {
    let rest = text.strip_prefix(prefix).unwrap_or(text);
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().collect::<String>() + chars.as_str()
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

fn status(ui: &UiState<'_>) -> (bool, String) {
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
            )
        }
        Some(TutorialStepId::Upgrade) => {
            let id = hadal_core::content::ids::UpgradeId::ImpedanceMatch;
            let affordable = view.upgrade_affordable(id);
            (
                affordable,
                if affordable {
                    "u then Enter buy Impedance Match".to_owned()
                } else {
                    format!("need {} Echo", amount(view.upgrade_cost(id)))
                },
            )
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
            )
        }
        Some(TutorialStepId::Peak) => (
            view.can_descend(),
            if view.can_descend() {
                "2 open DESCENT".to_owned()
            } else {
                "need 1.00M Peak Echo".to_owned()
            },
        ),
        Some(TutorialStepId::Descend | TutorialStepId::SecondDescent) => (
            view.can_descend(),
            if view.can_descend() {
                "2 then p then y descend".to_owned()
            } else {
                "need 1.00M Peak Echo".to_owned()
            },
        ),
        Some(TutorialStepId::Hull) => {
            let id = hadal_core::content::ids::HullId::AutoPulse;
            let affordable = view.hull_affordable(id);
            (
                affordable,
                if affordable {
                    "2 then Auto-Pulse then Enter".to_owned()
                } else {
                    format!("need {} Fathom", view.hull_cost(id))
                },
            )
        }
        Some(TutorialStepId::System) => (true, "4 open SYSTEM".to_owned()),
        Some(TutorialStepId::Manual) => (true, "? open, Esc close".to_owned()),
        Some(TutorialStepId::Offline) => (true, "Enter finish the tutorial".to_owned()),
        None => (true, "free play".to_owned()),
    }
}

fn coach_lines(ui: &UiState<'_>, width: usize) -> Vec<Line<'static>> {
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
    let header_right = "[S] SKIP  ".to_owned();
    let pad = width
        .saturating_sub(header_left.chars().count() + header_right.chars().count())
        .max(1);
    let header = Line::from(vec![
        Span::styled(
            header_left,
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad), Style::new().fg(p.text)),
        Span::styled(
            header_right,
            Style::new().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
    ]);

    let instruction_text = if matches!(ui.screen, crate::render::Screen::Descent) {
        match step.id {
            TutorialStepId::Hull => capitalized_after(step.instruction, "Press 2, "),
            TutorialStepId::Descend => capitalized_after(step.instruction, "Open DESCENT with 2, "),
            _ => step.instruction.to_owned(),
        }
    } else {
        step.instruction.to_owned()
    };
    let instruction = instruction_line(&instruction_text, p);
    let why = model::ascii_text(step.why, ui.ascii);
    let [why1, why2] = wrap_two(&why, width.saturating_sub(2), ui.ascii);

    let (ready, action) = status(ui);
    let mut status_spans = vec![
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
    if let Some(progress) = tutorial.progress {
        status_spans.push(Span::styled("  ", Style::new().fg(p.text)));
        status_spans.push(Span::styled(
            chapter_line(&progress),
            Style::new().fg(p.text),
        ));
        status_spans.push(Span::styled(" ", Style::new().fg(p.text)));
        status_spans.push(Span::styled(
            bar(progress_fraction(&progress), 5, glyphs::set(ui.ascii)),
            Style::new().fg(p.amber),
        ));
    }

    vec![
        header,
        instruction,
        Line::from(Span::styled(format!("  {why1}"), Style::new().fg(p.dim))),
        Line::from(Span::styled(format!("  {why2}"), Style::new().fg(p.dim))),
        Line::from(status_spans),
    ]
}

/// Draw the coach band above the screen footer.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let Some(band) = band_rect(ui, area) else {
        return;
    };
    let height = HEIGHT;
    let p = ui.palette;
    let filler = " ".repeat(band.width as usize);
    let mut lines = Vec::with_capacity(height as usize);
    for _ in 0..height {
        lines.push(Line::from(Span::styled(
            filler.clone(),
            Style::new().bg(p.panel),
        )));
    }
    frame.render_widget(Paragraph::new(lines), band);

    // Restore the outer frame side borders on the band rows so underlying
    // horizontal-rule junctions cannot survive at the frame edge.
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

    let lines = coach_lines(ui, band.width as usize);
    let rows = Layout::vertical([Constraint::Length(1); 5]).split(band);
    for (row, line) in rows.iter().zip(lines) {
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(p.panel).fg(p.text)),
            *row,
        );
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
            "Restart from step 1 of 13?",
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
