//! Boot/title screen: a graphical, story-driven intro sequence.
//!
//! A three-row `HADAL` wordmark sits on a cleared band above a dim radar
//! half-disc, with a carrier waveform strip between. Beats fade in by stage
//! (see `app::BOOT_STAGE_MS`). No paragraphs, no boot log lines.

use hadal_core::content::story::BOOT_BEATS;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::render::UiState;
use crate::render::canvas::Canvas;
use crate::render::radar::direction;
use crate::render::wave::triangle_row;

/// Three-row Unicode wordmark (all rows are 29 columns wide).
pub(crate) const WORDMARK_UNICODE: [&str; 3] = [
    "█   █  ███  ████   ███  █    ",
    "█   █ █   █ █   █ █   █ █    ",
    "█████ █████ ████  █████ █████",
];

/// Three-row ASCII wordmark (all rows are 29 columns wide).
pub(crate) const WORDMARK_ASCII: [&str; 3] = [
    "#   #  ###  ####   ###  #    ",
    "#   # #   # #   # #   # #    ",
    "##### ##### ####  ##### #####",
];

/// Width of the wordmark block (all art rows share this width).
const WORDMARK_W: i32 = 29;

// Relative composition rows. Each text row is two pixel rows.
const BEVEL_ROW: i32 = 3;
const WAVE_ROW: i32 = 5;
const BEATS_ROW: i32 = 8;
const BEATS_BOTTOM: i32 = BEATS_ROW + 2;

/// Story-beat stages (0 = none, 3 = all three).
const STAGES: usize = 3;

/// Render the boot screen for the given reveal stage (`0..=3`).
pub fn render(frame: &mut Frame, ui: &UiState<'_>, revealed: usize) {
    let area = frame.area();
    let p = ui.palette;
    frame.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let w = area.width as i32;
    let h = area.height as i32;
    // Top calibrated so the 80x24 layout (wordmark 4-6, beats 12) and the
    // 100x30 layout (wordmark 9-11, beats 17-19) both hold; the "ANY KEY
    // SKIPS" hint is fixed at row 2 regardless of `top`.
    let top = (4 + (h - 24) * 5 / 6).max(0);

    // --- Backdrop: bottom radar half-disc -----------------------------
    let mut canvas = Canvas::new(area.width, area.height);
    canvas.clear(None);

    // Upper half of a disc centred on the bottom pixel row.
    let cx = w / 2;
    let cy = 2 * h - 1;
    // Formula radius, clamped so the dome apex stays below the beat row.
    // At 80×24 the single beat row sits between the outer and middle dome rings;
    let apex_row = top + BEATS_BOTTOM + 1;
    let fit = (cy - 2 * apex_row).max(0);
    let radius = (w / 2 - 6).min(h * 2 - 10).min(fit).max(0);
    if radius >= 3 {
        canvas.ring(cx, cy, radius / 3, p.dim);
        canvas.ring(cx, cy, 2 * radius / 3, p.dim);
        canvas.ring(cx, cy, radius, p.dim);
        for i in 0..8 {
            let (dx, dy) = direction(i * 8);
            canvas.line(
                cx,
                cy,
                cx + dx * radius / 1024,
                cy + dy * radius / 1024,
                p.dim,
            );
        }
        let idx = if ui.anim.reduce_motion {
            0
        } else {
            (ui.anim.frame_ms / 40) as usize % 64
        };
        let (sx, sy) = direction(idx);
        canvas.line(
            cx,
            cy,
            cx + sx * radius / 1024,
            cy + sy * radius / 1024,
            p.dim,
        );
    }

    canvas.blit(
        frame.buffer_mut(),
        area,
        ui.settings.color_depth,
        ui.ascii,
        p.bg,
    );

    // --- Wordmark: 3-row text art on a cleared band -------------------
    let art = if ui.ascii {
        &WORDMARK_ASCII
    } else {
        &WORDMARK_UNICODE
    };
    // Clear the row above, the three wordmark rows and the bevel row.
    for row in (top - 1)..=(top + BEVEL_ROW) {
        fill_row(frame, area, row, p.bg);
    }
    for (i, line) in art.iter().enumerate() {
        centered(frame, area, top + i as i32, line, p.accent, true, p.bg);
    }
    // 1-cell dim underline bevel.
    let bevel = bevel_run(ui.ascii);
    centered(frame, area, top + BEVEL_ROW, &bevel, p.dim, false, p.bg);

    // --- Wave strip on a cleared band ---------------------------------
    let wave_w = (w - 20).clamp(0, 40);
    let wave_x0 = (w - wave_w) / 2;
    let wave_rows: u16 = 2;
    let wave_px_h = i32::from(wave_rows) * 2;
    let off = if ui.anim.reduce_motion {
        0
    } else {
        (ui.anim.frame_ms / 35) as i32
    };
    let colour = if ui.view.carrier_matched() {
        p.accent
    } else {
        p.dim
    };
    for row in (top + WAVE_ROW)..(top + WAVE_ROW + i32::from(wave_rows)) {
        fill_row(frame, area, row, p.bg);
    }
    let mut wave = Canvas::new(wave_w as u16, wave_rows);
    let mut prev: Option<(i32, i32)> = None;
    for x in 0..wave_w {
        let row = triangle_row(x, off, wave_px_h);
        match prev {
            Some((px, pr)) => wave.line(px, pr, x, row, colour),
            None => wave.put(x, row, colour),
        }
        prev = Some((x, row));
    }
    let wave_rect = Rect::new(
        area.x + wave_x0 as u16,
        area.y + (top + WAVE_ROW) as u16,
        wave_w as u16,
        wave_rows,
    );
    wave.blit(
        frame.buffer_mut(),
        wave_rect,
        ui.settings.color_depth,
        ui.ascii,
        p.bg,
    );

    // --- Story beats (stage-gated), then the skip hint at top-right ----
    let shown = revealed.min(STAGES);
    for (i, beat) in BOOT_BEATS.iter().take(shown).enumerate() {
        let row = top + BEATS_ROW + i as i32;
        fill_row(frame, area, row, p.bg);
        let fg = if i == 0 { p.amber } else { p.text };
        centered(frame, area, row, beat, fg, false, p.bg);
    }
    // The skip hint lives on row 2, right-aligned, clear of the radar dome so
    // no spoke is severed. It is always shown (independent of stage).
    fill_row(frame, area, 2, p.bg);
    right_aligned(frame, area, 2, "ANY KEY SKIPS  ", p.dim, p.bg);
}

/// A run of `───` (or `---`) matching the wordmark width, for the bevel.
fn bevel_run(ascii: bool) -> String {
    let ch = if ascii { '-' } else { '─' };
    std::iter::repeat_n(ch, WORDMARK_W as usize).collect()
}

/// Overwrite every cell of a row with a blank on `bg` (erases backdrop glyphs).
fn fill_row(frame: &mut Frame, area: Rect, row: i32, bg: Color) {
    if row < 0 || row >= area.height as i32 {
        return;
    }
    let y = area.y + row as u16;
    let buf = frame.buffer_mut();
    for x in area.x..area.x.saturating_add(area.width) {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(' ');
            cell.set_style(Style::new().bg(bg));
        }
    }
}

/// Draw one horizontally-centred text row on a cleared row.
fn centered(frame: &mut Frame, area: Rect, row: i32, text: &str, fg: Color, bold: bool, bg: Color) {
    if row < 0 || row >= area.height as i32 {
        return;
    }
    let mut style = Style::new().fg(fg);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    let y = area.y + row as u16;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_owned(), style)))
            .alignment(Alignment::Center)
            .style(Style::new().bg(bg)),
        Rect::new(area.x, y, area.width, 1),
    );
}

/// Draw one horizontally-right-aligned text row on a cleared row.
fn right_aligned(frame: &mut Frame, area: Rect, row: i32, text: &str, fg: Color, bg: Color) {
    if row < 0 || row >= area.height as i32 {
        return;
    }
    let y = area.y + row as u16;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text.to_owned(),
            Style::new().fg(fg),
        )))
        .alignment(Alignment::Right)
        .style(Style::new().bg(bg)),
        Rect::new(area.x, y, area.width, 1),
    );
}
