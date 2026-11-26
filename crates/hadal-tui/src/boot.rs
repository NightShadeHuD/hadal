//! Boot/title screen: wordmark, station readout, and typewriter boot lines.

use hadal_core::StateView;
use hadal_core::content::story::BOOT_LINES;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::glyphs::Glyphs;
use crate::model::format_count;
use crate::theme::Palette;

/// Three-line Unicode wordmark.
pub const WORDMARK_UNICODE: [&str; 3] = [
    "█   █  ███  ████   ███  █    ",
    "█   █ █   █ █   █ █   █ █    ",
    "█████ █████ ████  █████ █████",
];

/// Three-line ASCII wordmark.
pub const WORDMARK_ASCII: [&str; 3] = [
    "#   #  ###  ####   ###  #    ",
    "#   # #   # #   # #   # #    ",
    "##### ##### ####  ##### #####",
];

/// Render the boot screen with `revealed` boot lines shown.
pub fn render(
    frame: &mut Frame,
    state: StateView<'_>,
    palette: &Palette,
    _glyphs: &Glyphs,
    ascii: bool,
    revealed: usize,
) {
    let area = frame.area();
    let bg = Style::new().bg(palette.bg).fg(palette.text);
    frame.render_widget(Block::new().style(bg), area);

    let wordmark = if ascii {
        WORDMARK_ASCII
    } else {
        WORDMARK_UNICODE
    };

    let mut lines: Vec<Line> = Vec::new();
    for row in wordmark {
        lines.push(Line::from(Span::styled(
            row,
            Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "STATION K-9   //   DEPTH {} M   //   CARRIER {:02}",
            format_count(state.depth_m()),
            state.carrier_band() + 1
        ),
        Style::new().fg(palette.amber),
    )));
    lines.push(Line::from(""));

    let shown = revealed.min(BOOT_LINES.len());
    for text in BOOT_LINES.iter().take(shown) {
        lines.push(Line::from(Span::styled(
            *text,
            Style::new().fg(palette.text),
        )));
    }
    if shown < BOOT_LINES.len() {
        let caret = if ascii { "|" } else { "▌" };
        lines.push(Line::from(Span::styled(
            caret.to_owned(),
            Style::new().fg(palette.accent),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "any key skips",
        Style::new().fg(palette.dim),
    )));

    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let view = Rect::new(area.x, top, area.width, height.min(area.height));
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).style(bg),
        view,
    );
}
