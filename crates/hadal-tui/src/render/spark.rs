//! Sparkline renderer: rate history displayed as bar glyphs (ASCII fallback).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use hadal_core::Decimal;

use super::UiState;

/// Glyphs for sparkline bars, from shortest to tallest.
const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// ASCII fallback glyphs.
const ASCII_GLYPHS: [char; 8] = [' ', '.', ':', '-', '=', '+', '*', '@'];

/// Render the sparkline display.
pub fn render(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let p = &ui.palette;
    let history = &ui.anim.rate_history;
    let head = ui.anim.rate_head;
    let count = history.len();

    // Compute max using Decimal comparison
    let max = history
        .iter()
        .copied()
        .fold(Decimal::ZERO, |a, b| if b > a { b } else { a });

    // Build the line: label + samples
    let mut spans = Vec::with_capacity(1 + count);
    spans.push(Span::styled("HIST ", Style::default().fg(p.dim)));

    for i in 0..count {
        let idx = (head.wrapping_sub(count - 1 - i)).wrapping_rem(count);
        let val = history[idx];
        let (ch, zero) = sample_glyph(val, max, ui.ascii);
        let style = if zero {
            Style::default().fg(p.dim)
        } else {
            Style::default().fg(p.text)
        };
        spans.push(Span::styled(ch.to_string(), style));
    }

    // Leave one trailing cell blank so the glyph run never touches the border.
    let draw = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    frame.render_widget(Paragraph::new(Line::from(spans)), draw);
}

/// Map one sample to its glyph. Zero-rate samples return `('.', true)` (a dim
/// dot, or a space in ASCII) so a fresh run reads as empty history rather than
/// a flat low bar; non-zero samples return an ascending bar bucket.
fn sample_glyph(val: Decimal, max: Decimal, ascii: bool) -> (char, bool) {
    if val == Decimal::ZERO {
        let ch = if ascii { ASCII_GLYPHS[0] } else { '.' };
        return (ch, true);
    }
    let glyphs = if ascii { ASCII_GLYPHS } else { GLYPHS };
    (glyphs[bucket(val, max)], false)
}

/// Bar bucket `0..=7` for a non-zero value relative to `max`: the highest `k`
/// whose `max*k/8` threshold the value reaches. A sample equal to `max` clears
/// `max*7/8` and so maps to bucket 7; the mapping is monotonic in `val`.
fn bucket(val: Decimal, max: Decimal) -> usize {
    for j in (0..=7u64).rev() {
        let threshold = max
            .mul(Decimal::from_u64(j))
            .div(Decimal::from_u64(8))
            .unwrap();
        if val >= threshold {
            return j as usize;
        }
    }
    0
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use hadal_core::Decimal;

    fn glyphs_for(history: &[Decimal; 64], ascii: bool) -> Vec<(char, bool)> {
        let max = history
            .iter()
            .copied()
            .fold(Decimal::ZERO, |a, b| if b > a { b } else { a });
        history
            .iter()
            .map(|&val| sample_glyph(val, max, ascii))
            .collect()
    }

    /// The sparkline renders all 64 cells; a ramp produces ascending buckets,
    /// the max sample reaches bucket 7, and zero history produces dim dots
    /// (or ASCII space), not a low bar.
    #[test]
    fn sparkline_scales_and_pads() {
        // A monotonic integer ramp across all 64 cells: 1..=64 hundred.
        let mut ramp = [Decimal::ZERO; 64];
        for (i, slot) in ramp.iter_mut().enumerate() {
            *slot = Decimal::from_u64((i as u64 + 1) * 100);
        }
        let cells = glyphs_for(&ramp, false);
        assert_eq!(cells.len(), 64, "must render every cell");

        // Each cell is a real bar bucket (none treated as zero).
        assert!(
            cells.iter().all(|(_, zero)| !zero),
            "non-zero ramp must not render as empty"
        );
        // Buckets are non-decreasing across the ramp.
        let b: Vec<usize> = ramp.iter().map(|&v| bucket(v, ramp[63])).collect();
        assert!(
            b.windows(2).all(|w| w[0] <= w[1]),
            "ramp buckets must be ascending: {b:?}"
        );
        assert_eq!(b[63], 7, "the max sample must map to bucket 7");
        assert_eq!(b[0], 0, "the smallest ramp step maps to bucket 0");

        // The newest (max) cell renders the full block / ASCII '@'.
        let newest_unicode = sample_glyph(ramp[63], ramp[63], false);
        assert_eq!(newest_unicode, ('█', false), "max cell is a full block");
        let newest_ascii = sample_glyph(ramp[63], ramp[63], true);
        assert_eq!(newest_ascii, ('@', false), "max cell is ASCII '@'");

        // Zero history: every cell is the dim zero glyph, never a bar.
        let zero = [Decimal::ZERO; 64];
        let zero_cells = glyphs_for(&zero, false);
        assert_eq!(zero_cells.len(), 64);
        assert!(
            zero_cells.iter().all(|&(ch, z)| z && ch == '.'),
            "zero history must render dim dots"
        );
        let zero_ascii = glyphs_for(&zero, true);
        assert!(
            zero_ascii.iter().all(|&(ch, z)| z && ch == ' '),
            "zero history ASCII is a space"
        );
        assert!(
            !zero_cells.iter().any(|&(ch, _)| ch == GLYPHS[0]),
            "zero history must not render the lowest bar"
        );
    }
}
