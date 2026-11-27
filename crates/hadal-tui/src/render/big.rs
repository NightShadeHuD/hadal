//! Big block-digit renderer: 5-row × 3-col ASCII art digits for the ECHO display.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::UiState;
use super::canvas::Canvas;
use super::sonar::split_numeric;

/// 3-col × 5-row block font for digits 0-9 and '.', ',', '+', '-'.
/// `#` = filled, `.` = empty. Each glyph is stored as 5 strings of 3 chars.
const DIGIT_GLYPHS: [&[&str; 5]; 14] = [
    // 0
    &["###", "#.#", "#.#", "#.#", "###"],
    // 1
    &["..#", ".##", "..#", "..#", "..#"],
    // 2
    &["###", "..#", "###", "#..", "###"],
    // 3
    &["###", "..#", "###", "..#", "###"],
    // 4
    &["#.#", "#.#", "###", "..#", "..#"],
    // 5
    &["###", "#..", "###", "..#", "###"],
    // 6
    &["###", "#..", "###", "#.#", "###"],
    // 7
    &["###", "..#", "..#", "..#", "..#"],
    // 8
    &["###", "#.#", "###", "#.#", "###"],
    // 9
    &["###", "#.#", "###", "..#", "###"],
    // '.'
    &["...", "...", "...", "...", ".#."],
    // ','
    &["...", "...", "...", ".#.", "#.."],
    // '+'
    &["...", ".#.", "###", ".#.", "..."],
    // '-'
    &["...", "...", "###", "...", "..."],
];

/// Map a character to a glyph index.
fn char_to_digit(c: char) -> Option<usize> {
    match c {
        '0'..='9' => Some(c as usize - '0' as usize),
        '.' => Some(10),
        ',' => Some(11),
        '+' => Some(12),
        '-' => Some(13),
        _ => None,
    }
}

/// Render the big ECHO display.
pub fn render(frame: &mut Frame, area: Rect, ui: &UiState<'_>, rows: u16) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let value = super::amount(ui.anim.echo_display);
    let (numeric, suffix) = split_numeric(&value);
    let suffix = suffix.to_owned();
    let digits: Vec<usize> = numeric.chars().filter_map(char_to_digit).collect();

    if rows >= 5 {
        render_tall(frame, area, ui, &digits, &suffix);
    } else {
        render_compact(frame, area, ui, &digits, &suffix, rows);
    }
}

/// Five-row text glyphs (tall layout). The suffix is drawn only on the first row.
fn render_tall(frame: &mut Frame, area: Rect, ui: &UiState<'_>, digits: &[usize], suffix: &str) {
    let p = ui.palette;
    // Transpose the chosen glyphs into five text rows.
    let glyph_rows: Vec<Vec<&str>> = (0..5)
        .map(|ri| digits.iter().map(|&d| DIGIT_GLYPHS[d][ri]).collect())
        .collect();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(5);
    for (ri, row) in glyph_rows.iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(row.len() * 2 + 2);
        if ri == 0 {
            spans.push(Span::styled("ECHO ", Style::default().fg(p.dim)));
        } else {
            spans.push(Span::raw("     "));
        }
        for (di, &cell) in row.iter().enumerate() {
            if di > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                cell.to_owned(),
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ));
        }
        if ri == 0 && !suffix.is_empty() {
            spans.push(Span::styled(suffix.to_owned(), Style::default().fg(p.text)));
        }
        lines.push(Line::from(spans));
    }
    for (i, line) in lines.iter().enumerate() {
        let y = area.top().saturating_add(i as u16);
        if y >= area.top().saturating_add(area.height) {
            break;
        }
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(area.left(), y, area.width, 1),
        );
    }
}

/// Compact layout: the same five-row glyphs drawn as half-block pixels across
/// `rows` text rows, offset past the `ECHO ` label.
fn render_compact(
    frame: &mut Frame,
    area: Rect,
    ui: &UiState<'_>,
    digits: &[usize],
    suffix: &str,
    rows: u16,
) {
    let p = ui.palette;
    if area.width < 6 || rows == 0 {
        return;
    }
    // Label on the first row, left of the blitted glyph block.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "ECHO ",
            Style::default().fg(p.dim),
        ))),
        Rect::new(area.left(), area.top(), 5, 1),
    );

    let glyph_x = area.left().saturating_add(5);
    let avail = area.width.saturating_sub(5);
    // Each digit is 3 cells wide plus a 1-cell gap: glyph width = 4*d - 1.
    let max_digits = ((avail as usize + 1) / 4).max(1);
    let visible = digits.len().min(max_digits);
    let digits = &digits[..visible];
    let glyph_w = if digits.is_empty() {
        0
    } else {
        (digits.len() * 4 - 1) as u16
    };

    if glyph_w > 0 {
        let mut canvas = Canvas::new(glyph_w, rows);
        for (di, &didx) in digits.iter().enumerate() {
            let gx = (di * 4) as i32;
            let glyph = &DIGIT_GLYPHS[didx];
            for (ry, pat) in glyph.iter().enumerate() {
                for (cx, ch) in pat.chars().enumerate() {
                    if ch == '#' {
                        canvas.put(gx + cx as i32, ry as i32, p.accent);
                    }
                }
            }
        }
        canvas.blit(
            frame.buffer_mut(),
            Rect::new(glyph_x, area.top(), glyph_w, rows.min(area.height)),
            ui.settings.color_depth,
            ui.ascii,
            p.bg,
        );
    }

    // Suffix after the glyphs, first row only.
    if !suffix.is_empty() {
        let sx = glyph_x.saturating_add(glyph_w);
        let right = area.left().saturating_add(area.width);
        if sx < right {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    suffix.to_owned(),
                    Style::default().fg(p.text),
                ))),
                Rect::new(sx, area.top(), right - sx, 1),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Anim, Screen, UiState};
    use crate::theme::{ColorDepth, Palette, Theme};
    use hadal_core::{Decimal, Engine};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn render_value(value: u64) -> Buffer {
        let engine = Engine::new();
        let anim = Anim::settled(Decimal::from_u64(value));
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let ui = UiState::settled(engine.view(), palette, false, Screen::Sonar, &anim);
        let backend = TestBackend::new(24, 3);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, Rect::new(0, 0, 24, 3), &ui, 3))
            .expect("draw frame");
        terminal.backend().buffer().clone()
    }

    fn symbols(buf: &Buffer) -> String {
        buf.content()
            .iter()
            .map(|c| c.symbol().to_owned())
            .collect()
    }

    /// All ten digits render and differ pairwise.
    #[test]
    fn block_digits_cover_0_to_9() {
        let mut representations: Vec<String> = Vec::new();
        for glyph in DIGIT_GLYPHS.iter().take(10) {
            let repr: String = glyph.join("\n");
            representations.push(repr);
        }

        for i in 0..10 {
            for j in (i + 1)..10 {
                assert_ne!(
                    representations[i], representations[j],
                    "digits {i} and {j} produce identical glyphs"
                );
            }
        }
    }

    /// The compact (rows<5) path must keep the full 5-row font legible.
    #[test]
    fn compact_digits_use_full_font() {
        let nine = symbols(&render_value(9));
        let eight = symbols(&render_value(8));
        let one = symbols(&render_value(1));
        assert_ne!(nine, eight, "9 and 8 must differ at rows=3");
        assert_ne!(one, eight, "1 and 8 must differ at rows=3");
    }
}
