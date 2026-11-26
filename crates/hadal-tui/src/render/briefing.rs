//! First-run briefing: the operator's one-panel onboarding.

use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::glyphs;
use crate::model;
use crate::render::{UiState, overlay_box, titled_frame};

const REGULAR_HEADER: &str = "OPERATOR 7 // RELAY K-9 // 7,912 M";

/// Briefing content for the current mode (demo adds its honesty header).
pub fn lines(ui: &UiState<'_>) -> Vec<Line<'static>> {
    let p = ui.palette;
    let mut lines = Vec::with_capacity(24);

    if ui.demo {
        lines.push(Line::from(Span::styled(
            "DEMO // NOTHING IS SAVED",
            Style::new().fg(p.alert).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            REGULAR_HEADER.to_owned(),
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "late game: {} descents, {} hulls, automation is on",
                ui.view.descents(),
                ui.view.hull_count()
            ),
            Style::new().fg(p.dim),
        )));
        lines.push(Line::from(Span::styled(
            "the station plays itself; keys are additive",
            Style::new().fg(p.dim),
        )));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(Span::styled(
            REGULAR_HEADER,
            Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    let body = [
        "The station is dead. The trench is not.",
        "A carrier repeats on the hydrophones, and it is",
        "getting clearer as K-9 sinks.",
        "",
        "YOUR FIRST HOUR",
        "  SPACE  pulse the capacitor: one pulse wakes 10 s of Echo",
        "  h      buy a Hydrophone (10 Echo): it listens while you do not",
        "  f      tune to the carrier band: a match is worth ×1.5",
        "  2      DESCENT at 1.00M Peak Echo: earn Fathoms, buy permanent hulls",
        "",
        "Echo is this run's currency. Fathoms are forever.",
        "Every Descent sinks K-9 another 500 m and starts again, louder.",
        "",
    ];
    for text in body {
        let text = model::ascii_text(text, ui.ascii);
        let style = if text == "YOUR FIRST HOUR" {
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(p.text)
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    lines.push(Line::from(vec![
        Span::styled(
            "[?] full briefing",
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  [any key] begin", Style::new().fg(p.dim)),
    ]));
    lines
}

/// Draw the briefing inside the console frame.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) {
    let p = ui.palette;
    let g = glyphs::set(ui.ascii);
    let area = frame.area();

    let left = Line::from(vec![
        Span::styled(
            " HADAL",
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}{} ", g.h_line, g.h_line),
            Style::new().fg(p.dim),
        ),
        Span::styled("RELAY K-9 ", Style::new().fg(p.text)),
    ]);
    let right = Line::from(Span::styled(
        " FIRST RUN ",
        Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
    ));
    let block = titled_frame(ui.ascii, &p, left, right);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 20 || inner.height < 8 {
        return;
    }

    let lines = lines(ui);
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let width = area.width.saturating_sub(4).min(76);
    let panel = overlay_box(frame, area, width, height, p, ui.ascii, "BRIEFING");
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(p.panel).fg(p.text)),
        panel,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Anim;
    use crate::theme::{ColorDepth, Palette, Theme};
    use hadal_core::Engine;

    #[test]
    fn copy_uses_ascii_and_unicode_multipliers() {
        let engine = Engine::new();
        let anim = Anim::settled(engine.view().echo());
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let unicode = UiState::settled(
            engine.view(),
            palette,
            false,
            crate::render::Screen::Briefing,
            &anim,
        );
        let ascii = UiState::settled(
            engine.view(),
            palette,
            true,
            crate::render::Screen::Briefing,
            &anim,
        );
        let unicode_text = lines(&unicode)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let ascii_text = lines(&ascii)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(unicode_text.contains("×1.5"));
        assert!(!unicode_text.contains("x1.5"));
        assert!(ascii_text.contains("x1.5"));
        assert!(!ascii_text.contains("×1.5"));
        assert!(unicode_text.contains("7,912 M"));
        assert!(unicode_text.contains("DESCENT at 1.00M"));
        assert!(unicode_text.contains("[?] full briefing  [any key] begin"));
    }

    #[test]
    fn demo_header_reports_late_game() {
        let engine = Engine::demo();
        let anim = Anim::settled(engine.view().echo());
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let mut ui = UiState::settled(
            engine.view(),
            palette,
            false,
            crate::render::Screen::Briefing,
            &anim,
        );
        ui.demo = true;
        let text = lines(&ui)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("DEMO // NOTHING IS SAVED"));
        assert!(text.contains("late game: 12 descents, 15 hulls, automation is on"));
        assert!(text.contains("keys are additive"));
    }
}
