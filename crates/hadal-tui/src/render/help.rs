//! HELP overlay: controls by screen, offline and number rules, version.

use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::render::{UiState, overlay_box};
use crate::theme::Palette;

fn section(text: &str, p: Palette) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
    ))
}

fn line(text: &str, p: Palette) -> Line<'static> {
    Line::from(Span::styled(text.to_owned(), Style::new().fg(p.text)))
}

fn blank() -> Line<'static> {
    Line::from("")
}

/// Help content lines.
pub fn help_lines(p: Palette) -> Vec<Line<'static>> {
    vec![
        section("CONTROLS", p),
        line(
            "GLOBAL   1-4 tabs  Tab cycle  ? help  Esc back  Ctrl+S save  q quit",
            p,
        ),
        line(
            "SONAR    Space pulse  f tune  h/H d/D m/M c/C buy  u chain",
            p,
        ),
        line(
            "DESCENT  arrows focus  Enter buy  p then y descend  +/- governor",
            p,
        ),
        line("ARCHIVE  arrows scroll  PgUp/PgDn page", p),
        line("SYSTEM   arrows focus  Enter/Space change  t toggle", p),
        line("         s save  e export  i import  R reset", p),
        blank(),
        section("OFFLINE", p),
        line("8 h base cap. Long Storage 24 h, Extended Ballast 72 h,", p),
        line(
            "Abyssal Reservoir 168 h. Automation only, exact ticks, full rate.",
            p,
        ),
        blank(),
        section("NUMBERS", p),
        line("Amounts floor, costs ceil; affordability is exact.", p),
        blank(),
        line(&format!("HADAL v{}", env!("CARGO_PKG_VERSION")), p),
        blank(),
        line("ANY KEY CLOSES", p),
    ]
}

/// Draw the help overlay.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let width = area.width.saturating_sub(6).clamp(50, 78);
    let height = area.height.saturating_sub(2).min(23);
    let inner = overlay_box(frame, area, width, height, ui.palette, ui.ascii, "HELP");
    let lines = help_lines(ui.palette);
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(ui.palette.panel)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorDepth, Theme};

    #[test]
    fn help_covers_controls_offline_and_number_rules() {
        let p = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let text = help_lines(p)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("GLOBAL"));
        assert!(text.contains("SONAR"));
        assert!(text.contains("DESCENT"));
        assert!(text.contains("ARCHIVE"));
        assert!(text.contains("SYSTEM"));
        assert!(text.contains("8 h base cap"));
        assert!(text.contains("24 h"));
        assert!(text.contains("168 h"));
        assert!(text.contains("Automation only"));
        assert!(text.contains("Amounts floor"));
        assert!(text.contains("costs ceil"));
        assert!(text.contains("HADAL v"));
        assert!(text.contains("ANY KEY CLOSES"));
    }
}
