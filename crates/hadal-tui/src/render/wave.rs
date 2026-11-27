//! Waveform renderer: scrolling triangle wave with carrier/tuned band labels.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::UiState;
use super::canvas::Canvas;

/// Triangle-wave pixel row for column `x`, given horizontal offset `off` and a
/// strip `pixel_h` pixels tall. Shared by the SONAR wave strip and the boot intro.
pub(crate) fn triangle_row(x: i32, off: i32, pixel_h: i32) -> i32 {
    let baseline = pixel_h - 1;
    let t = ((x + off) % 32 + 32) % 32;
    let dy = if t < 16 { t } else { 31 - t };
    baseline - dy * (pixel_h - 1) / 15
}

/// Render the waveform display.
pub fn render(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = &ui.palette;
    let matched = ui.view.carrier_matched();
    let carrier = ui.view.carrier_band() + 1;
    let tuned = ui.view.tuned_band();

    let off: i32 = if ui.anim.reduce_motion {
        0
    } else {
        (ui.anim.frame_ms / 35) as i32
    };

    // --- Wave area (everything below the label row) ------------------
    if area.height > 1 {
        let wave_area = Rect::new(area.left(), area.top() + 1, area.width, area.height - 1);
        let canvas_w = wave_area.width as i32;
        let canvas_h = wave_area.height as i32;

        let mut canvas = Canvas::new(wave_area.width, wave_area.height);
        // Fill the background so the wave reads against the panel.
        canvas.fill_rect(0, 0, canvas_w, canvas_h * 2, p.bg);

        let pixel_h = canvas_h * 2;
        let colour = if matched { p.accent } else { p.dim };
        let mut prev: Option<(i32, i32)> = None;
        for x in 0..canvas_w {
            let row = triangle_row(x, off, pixel_h);
            match prev {
                Some((px, pr)) => canvas.line(px, pr, x, row, colour),
                None => canvas.put(x, row, colour),
            }
            prev = Some((x, row));
        }

        canvas.blit(
            frame.buffer_mut(),
            wave_area,
            ui.settings.color_depth,
            ui.ascii,
            p.bg,
        );
    }

    // --- Label row ---------------------------------------------------
    let label = if matched {
        format!(" CARRIER {:02}  TUNED", carrier)
    } else {
        format!(" CARRIER {:02} ", carrier)
    };
    let fg = if matched { p.accent } else { p.amber };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, Style::default().fg(fg)))),
        Rect::new(area.left(), area.top(), area.width, 1),
    );

    // Carrier marker `▲` / `^` on the label row.
    let marker_x = if area.width > 0 {
        (off.max(0) as u16) % area.width
    } else {
        0
    };
    let marker = if ui.ascii { '^' } else { '▲' };
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((area.left() + marker_x, area.top()))
    {
        cell.set_char(marker);
        cell.set_fg(p.accent);
        cell.set_bg(p.bg);
        cell.set_style(Style::default().add_modifier(Modifier::empty()));
    }

    // Tuned-band strip: a bright run of cells on the label row, one per band.
    const BANDS: u16 = 6;
    let strip_len = BANDS * 2;
    if area.width > strip_len {
        let strip_x0 = area.left() + area.width - strip_len - 1;
        for b in 0..BANDS {
            let bright = b == u16::from(tuned);
            let ch = if bright { '=' } else { '-' };
            for half in 0..2 {
                let x = strip_x0 + b * 2 + half;
                if let Some(cell) = frame.buffer_mut().cell_mut((x, area.top())) {
                    cell.set_char(ch);
                    cell.set_fg(if bright { p.accent } else { p.dim });
                    cell.set_bg(p.bg);
                    cell.set_style(Style::default().add_modifier(Modifier::empty()));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::super::canvas::Canvas;
    use crate::theme::ColorDepth;

    /// The full-height triangle wave is deterministic across 3- and 4-row areas.
    #[test]
    fn triangle_wave_is_deterministic() {
        let frame_ms = 12345u64;
        let off = (frame_ms / 35) as i32;
        let canvas_w = 32i32;

        for canvas_h in [3i32, 4i32] {
            let pixel_h = canvas_h * 2;
            let baseline = pixel_h - 1;

            let draw = || {
                let mut canvas = Canvas::new(canvas_w as u16, canvas_h as u16);
                canvas.clear(None);
                let mut prev: Option<(i32, i32)> = None;
                for x in 0..canvas_w {
                    let t = ((x + off) % 32 + 32) % 32;
                    let dy = if t < 16 { t } else { 31 - t };
                    let row = baseline - dy * (pixel_h - 1) / 15;
                    match prev {
                        Some((px, pr)) => canvas.line(px, pr, x, row, ratatui::style::Color::Red),
                        None => canvas.put(x, row, ratatui::style::Color::Red),
                    }
                    prev = Some((x, row));
                }
                let mut buf = Buffer::empty(Rect::new(0, 0, canvas_w as u16, canvas_h as u16));
                canvas.blit(
                    &mut buf,
                    Rect::new(0, 0, canvas_w as u16, canvas_h as u16),
                    ColorDepth::TrueColor,
                    false,
                    ratatui::style::Color::Black,
                );
                buf
            };

            let buf1 = draw();
            let buf2 = draw();
            for (a, b) in buf1.content().iter().zip(buf2.content().iter()) {
                assert_eq!(a.symbol(), b.symbol(), "canvas_h={canvas_h}");
            }
        }
    }
}
