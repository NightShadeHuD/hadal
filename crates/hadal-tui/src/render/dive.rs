//! The 900 ms dive transition: depth count-up plus boot lines.
//!
//! This screen is presentation only; the engine has already completed the
//! Descent. It never mutates state and never blocks the tick loop.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::glyphs;
use crate::render::{UiState, bar, count};

/// Lines revealed during the dive, in order.
pub const DIVE_LINES: [&str; 3] = [
    "BALLAST RELEASED.",
    "K-9 SINKS THROUGH ITS OWN SHADOW.",
    "THE CARRIER IS STILL REPEATING.",
];

/// Draw the dive transition at `progress` per-mille.
pub fn render(frame: &mut Frame, ui: &UiState<'_>, from_m: u32, to_m: u32, progress: u16) {
    let area = frame.area();
    let p = ui.palette;
    let g = glyphs::set(ui.ascii);
    let background = Style::new().bg(p.bg).fg(p.text);
    frame.render_widget(Block::new().style(background), area);

    let clamped = progress.min(1000);
    let t = f64::from(clamped) / 1000.0;
    let travelled = (u64::from(to_m.saturating_sub(from_m)) * u64::from(clamped) / 1000) as u32;
    let depth = from_m.saturating_add(travelled);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "DIVE",
        Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("DEPTH {} m", count(depth)),
        Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        bar(t, 24, g),
        Style::new().fg(p.accent),
    )));
    lines.push(Line::from(""));

    let shown = (usize::from(clamped) * DIVE_LINES.len() / 1000).min(DIVE_LINES.len());
    for text in DIVE_LINES.iter().take(shown) {
        lines.push(Line::from(Span::styled(*text, Style::new().fg(p.text))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "any key skips",
        Style::new().fg(p.dim),
    )));

    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let rect = Rect::new(area.x, top, area.width, height.min(area.height));
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(background),
        rect,
    );
}
