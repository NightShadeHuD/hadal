//! Pixel-level drawing canvas that maps a 2-pixels-per-row grid onto ratatui's
//! half-block character cells.
//!
//! Each terminal cell encodes two pixel rows: the top half becomes `▀` / `█`
//! and the bottom half becomes `▄` / `█`.  The canvas stores per-pixel colour
//! information and blits it to a ratatui buffer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::theme::{ColorDepth, to_color};

// ---------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------

/// A pixel canvas.  The pixel grid is `width × (height × 2)` because each
/// terminal row encodes two pixel rows (top / bottom half).
pub struct Canvas {
    width: u16,
    height: u16,
    /// Top half of each cell: `top[y * width + x]` is the colour at pixel
    /// row `2*y`, column `x`.
    top: Vec<Option<Color>>,
    /// Bottom half of each cell: `bottom[y * width + x]` is the colour at
    /// pixel row `2*y + 1`, column `x`.
    bottom: Vec<Option<Color>>,
}

impl Canvas {
    /// Create a new canvas that covers `area.width × area.height` cells.
    pub fn new(width: u16, height: u16) -> Self {
        let rows = (height as usize) * 2;
        let cols = width as usize;
        let len = rows * cols;
        Self {
            width,
            height,
            top: vec![None; len],
            bottom: vec![None; len],
        }
    }

    /// Fill the entire canvas with `color`.  `None` means transparent.
    pub fn clear(&mut self, color: Option<Color>) {
        self.top.fill(color);
        self.bottom.fill(color);
    }

    /// Place a single pixel.  Coordinates are in pixel rows (0-based).
    /// Clipped to the canvas bounds; negative values are silently ignored.
    pub fn put(&mut self, x: i32, y: i32, color: Color) {
        let cx = x as usize;
        let cy = y as usize;
        if cx >= self.width as usize || cy >= self.height as usize * 2 {
            return;
        }
        let idx = cy * self.width as usize + cx;
        if cy.is_multiple_of(2) {
            self.top[idx] = Some(color);
        } else {
            self.bottom[idx] = Some(color);
        }
    }

    /// Horizontal line from `x0` to `x1` (inclusive) at pixel row `y`.
    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, color: Color) {
        if y < 0 {
            return;
        }
        let (lo, hi) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for x in lo..=hi {
            self.put(x, y, color);
        }
    }

    /// Vertical line from `y0` to `y1` (inclusive) at pixel column `x`.
    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, color: Color) {
        if x < 0 {
            return;
        }
        let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for y in lo..=hi {
            self.put(x, y, color);
        }
    }

    /// Outline rectangle with top-left `(x, y)`, size `w × h`.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.hline(x, x + w - 1, y, color);
        self.hline(x, x + w - 1, y + h - 1, color);
        self.vline(x, y, y + h - 1, color);
        self.vline(x + w - 1, y, y + h - 1, color);
    }

    /// Filled rectangle with top-left `(x, y)`, size `w × h`.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        for dy in 0..h {
            self.hline(x, x + w - 1, y + dy, color);
        }
    }

    /// Draw a ring using the midpoint circle algorithm.
    /// `cx, cy` is the centre in pixel coordinates; `r` is the radius in pixels.
    pub fn ring(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        if r <= 0 {
            return;
        }
        let mut x = 0;
        let mut y = r;
        let mut d = 3 - 2 * r;

        let mut draw_sym = |px: i32, py: i32| {
            self.put(cx + px, cy + py, color);
            self.put(cx - px, cy + py, color);
            self.put(cx + px, cy - py, color);
            self.put(cx - px, cy - py, color);
        };

        while x <= y {
            draw_sym(x, y);
            draw_sym(y, x);
            if d < 0 {
                d = d + 4 * x + 6;
            } else {
                d = d + 4 * (x - y) + 10;
                y -= 1;
            }
            x += 1;
        }
    }

    /// Bresenham line from `(x0, y0)` to `(x1, y1)`.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -((y1 - y0).abs());
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;

        loop {
            self.put(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Blit the canvas onto a ratatui buffer, clipping to `area`.
    ///
    /// Per cell (top = pixel row `2r`, bottom = `2r+1`):
    /// - both transparent → `' '` with `background`
    /// - equal colours → `'█'` with `fg=color bg=background`
    /// - top only → `'▀'` with `fg=top bg=background`
    /// - bottom only → `'▄'` with `fg=bottom bg=background`
    /// - different → `'▀'` with `fg=top bg=bottom`
    ///
    /// When a half is transparent, `Modifier` is always cleared and `bg` is
    /// set to `background`.
    pub fn blit(
        &self,
        buf: &mut Buffer,
        area: Rect,
        depth: ColorDepth,
        ascii: bool,
        background: Color,
    ) {
        let area_left = area.left() as i32;
        let area_top = area.top() as i32;
        let area_w = area.width as i32;
        let area_h = area.height as i32;
        let buf_w = buf.area().width as i32;

        for cell_y in 0..area_h {
            for cell_x in 0..area_w {
                let buf_x = area_left + cell_x;
                let buf_y = area_top + cell_y;
                if buf_x < 0 || buf_x >= buf_w || buf_y < 0 || buf_y >= buf.area().height as i32 {
                    continue;
                }

                // Pixel coords: canvas pixel row = 2*cell_y and 2*cell_y+1
                let py0 = cell_y * 2;
                let py1 = py0 + 1;

                let top_color = self.pixel_color(cell_x, py0);
                let bottom_color = self.pixel_color(cell_x, py1);

                let (ch, fg, bg, modif) = match (top_color, bottom_color) {
                    (None, None) => (' ', None, background, Modifier::empty()),
                    (Some(tc), Some(bc)) if tc == bc => {
                        let q = quantize(tc, depth);
                        let ch = if ascii { '#' } else { '█' };
                        (ch, Some(q), background, Modifier::empty())
                    }
                    (Some(tc), Some(bc)) => {
                        let qtop = quantize(tc, depth);
                        let qbot = quantize(bc, depth);
                        let ch = if ascii { '=' } else { '▀' };
                        (ch, Some(qtop), qbot, Modifier::empty())
                    }
                    (Some(tc), None) => {
                        let q = quantize(tc, depth);
                        let ch = if ascii { '.' } else { '▀' };
                        (ch, Some(q), background, Modifier::empty())
                    }
                    (None, Some(bc)) => {
                        let q = quantize(bc, depth);
                        let ch = if ascii { '.' } else { '▄' };
                        (ch, Some(q), background, Modifier::empty())
                    }
                };

                if let Some(cell) = buf.cell_mut((buf_x as u16, buf_y as u16)) {
                    cell.set_char(ch);
                    cell.set_style(
                        Style::default()
                            .fg(fg.unwrap_or(background))
                            .bg(bg)
                            .add_modifier(modif),
                    );
                }
            }
        }
    }

    /// Get the colour at a pixel coordinate, or `None` if transparent.
    fn pixel_color(&self, cx: i32, cy: i32) -> Option<Color> {
        let px = cx as usize;
        let py = cy as usize;
        if px >= self.width as usize || py >= self.height as usize * 2 {
            return None;
        }
        let idx = py * self.width as usize + px;
        if cy % 2 == 0 {
            self.top[idx]
        } else {
            self.bottom[idx]
        }
    }
}

// ---------------------------------------------------------------------
// Quantize
// ---------------------------------------------------------------------

/// Quantize a colour to the given depth, delegating to [`to_color`].
pub fn quantize(color: Color, depth: ColorDepth) -> Color {
    to_color(depth, color)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::layout::Rect;

    use super::*;

    fn cell(buf: &Buffer, x: u16, y: u16) -> &Cell {
        let w = buf.area().width as usize;
        &buf.content()[y as usize * w + x as usize]
    }

    #[test]
    fn canvas_maps_two_pixels_per_cell() {
        let mut canvas = Canvas::new(2, 2);
        // Pixel row 0 = top half of cell (0,0); pixel row 1 = bottom half.
        canvas.put(0, 0, Color::Red);
        canvas.put(0, 1, Color::Blue);

        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        canvas.blit(
            &mut buf,
            Rect::new(0, 0, 2, 2),
            ColorDepth::TrueColor,
            false,
            Color::Black,
        );

        // Cell (0,0): top=Red, bottom=Blue -> '▀' fg=Red bg=Blue.
        let c00 = cell(&buf, 0, 0);
        assert_eq!(c00.symbol(), "▀");
        assert_eq!(c00.fg, Color::Red);
        assert_eq!(c00.bg, Color::Blue);

        // Cell (1,0): both transparent -> ' ' bg=Black.
        let c10 = cell(&buf, 1, 0);
        assert_eq!(c10.symbol(), " ");
        assert_eq!(c10.bg, Color::Black);

        // Cell (0,1): both transparent -> ' ' bg=Black.
        let c01 = cell(&buf, 0, 1);
        assert_eq!(c01.symbol(), " ");
        assert_eq!(c01.bg, Color::Black);
    }

    #[test]
    fn canvas_clips_edges() {
        let mut canvas = Canvas::new(4, 4);
        // Draw outside the canvas: negative x, past the right edge, past the bottom.
        canvas.put(-1, 0, Color::Red);
        canvas.put(10, 0, Color::Red);
        canvas.put(0, 10, Color::Red);
        // Draw inside: only pixel row 1 (bottom of cell row 0) gets colour.
        canvas.put(1, 1, Color::Green);
        canvas.put(2, 1, Color::Green);

        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        canvas.blit(
            &mut buf,
            Rect::new(0, 0, 2, 2),
            ColorDepth::TrueColor,
            false,
            Color::Black,
        );

        // Cell (0,0): transparent -> space.
        assert_eq!(cell(&buf, 0, 0).symbol(), " ");
        // Cell (1,0): top transparent, bottom Green -> '▄'.
        let c10 = cell(&buf, 1, 0);
        assert_eq!(c10.symbol(), "▄");
        assert_eq!(c10.fg, Color::Green);
        // Cell (0,1) and (1,1): pixel rows 2-3, transparent.
        assert_eq!(cell(&buf, 0, 1).symbol(), " ");
        assert_eq!(cell(&buf, 1, 1).symbol(), " ");
    }

    #[test]
    fn canvas_quantizes_truecolor_256_16_none() {
        let rgb = Color::Rgb(255, 128, 64);

        // TrueColor: keeps Rgb.
        assert_eq!(quantize(rgb, ColorDepth::TrueColor), rgb);
        // Auto: keeps Rgb.
        assert_eq!(quantize(rgb, ColorDepth::Auto), rgb);
        // Ansi256: becomes Indexed.
        let q256 = quantize(rgb, ColorDepth::Ansi256);
        assert!(matches!(q256, Color::Indexed(_)));
        // Ansi16: reduced to a named terminal colour (never the raw Rgb).
        let q16 = quantize(rgb, ColorDepth::Ansi16);
        assert!(!matches!(q16, Color::Rgb(..)));
        // None: becomes Reset.
        assert_eq!(quantize(rgb, ColorDepth::None), Color::Reset);
    }

    #[test]
    fn canvas_ascii_is_ascii() {
        let mut canvas = Canvas::new(4, 2);
        canvas.put(0, 0, Color::Red);
        canvas.put(0, 1, Color::Red);
        canvas.put(1, 0, Color::Green);
        canvas.put(1, 1, Color::Blue);
        canvas.put(2, 0, Color::Cyan);
        canvas.put(2, 1, Color::Cyan);
        canvas.put(3, 0, Color::Magenta);
        canvas.put(3, 1, Color::Yellow);

        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        canvas.blit(
            &mut buf,
            Rect::new(0, 0, 4, 2),
            ColorDepth::TrueColor,
            true,
            Color::Black,
        );

        // With ascii=true every rendered glyph must be ASCII.
        for y in 0..2u16 {
            for x in 0..4u16 {
                let sym = cell(&buf, x, y).symbol();
                assert!(sym.is_ascii(), "glyph at ({x},{y}) '{sym}' is not ASCII");
            }
        }
    }
}
