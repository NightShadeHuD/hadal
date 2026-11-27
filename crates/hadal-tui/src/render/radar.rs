//! Radar renderer: 64-direction sweep with contacts, pulse rings, and
//! centre crosshair, all drawn through [`super::canvas::Canvas`].

use ratatui::Frame;

use super::UiState;
use super::canvas::Canvas;

/// 64 fixed-direction offsets: `(round(1024*cos(i*TAU/64)), round(1024*sin(i*TAU/64)))`.
pub(crate) const DIRS: [(i32, i32); 64] = [
    (1024, 0),
    (1019, 100),
    (1004, 200),
    (980, 297),
    (946, 392),
    (903, 483),
    (851, 569),
    (792, 650),
    (724, 724),
    (650, 792),
    (569, 851),
    (483, 903),
    (392, 946),
    (297, 980),
    (200, 1004),
    (100, 1019),
    (0, 1024),
    (-100, 1019),
    (-200, 1004),
    (-297, 980),
    (-392, 946),
    (-483, 903),
    (-569, 851),
    (-650, 792),
    (-724, 724),
    (-792, 650),
    (-851, 569),
    (-903, 483),
    (-946, 392),
    (-980, 297),
    (-1004, 200),
    (-1019, 100),
    (-1024, 0),
    (-1019, -100),
    (-1004, -200),
    (-980, -297),
    (-946, -392),
    (-903, -483),
    (-851, -569),
    (-792, -650),
    (-724, -724),
    (-650, -792),
    (-569, -851),
    (-483, -903),
    (-392, -946),
    (-297, -980),
    (-200, -1004),
    (-100, -1019),
    (0, -1024),
    (100, -1019),
    (200, -1004),
    (297, -980),
    (392, -946),
    (483, -903),
    (569, -851),
    (650, -792),
    (724, -724),
    (792, -650),
    (851, -569),
    (903, -483),
    (946, -392),
    (980, -297),
    (1004, -200),
    (1019, -100),
];

/// Unit-circle direction offset for a sweep index (wraps at 64).
pub(crate) fn direction(index: usize) -> (i32, i32) {
    DIRS[index % 64]
}

/// Contact positions: `(direction_index, percent_radius)`.
const CONTACTS: [(usize, u8); 4] = [(9, 35), (25, 72), (41, 50), (53, 88)];

/// Ring radius in pixels for a `w_cells × h_cells` display: limited by the
/// horizontal half-width and the full pixel height, minus a 2-pixel margin.
fn radius_for(w_cells: i32, h_cells: i32) -> i32 {
    (w_cells / 2 - 2).min(h_cells * 2 - 2)
}

/// Render the radar display.
pub fn render(frame: &mut Frame, area: ratatui::layout::Rect, ui: &UiState<'_>) {
    let p = &ui.palette;
    let canvas_w = area.width as i32;
    let canvas_h = area.height as i32;
    let pixel_h = canvas_h * 2;

    // Determine ring radius in pixels (uses the full cell height, not half).
    let radius = radius_for(canvas_w, canvas_h);

    // If the area is too small, draw a single dim SONAR text row and return.
    if radius < 3 {
        use ratatui::text::Line;
        use ratatui::widgets::Paragraph;
        frame.render_widget(
            Paragraph::new(Line::from(ratatui::text::Span::styled(
                "SONAR",
                ratatui::style::Style::new().fg(p.dim),
            ))),
            area,
        );
        return;
    }

    let centre_x = canvas_w / 2;
    let centre_y = pixel_h - 1; // bottom pixel row, visible

    let mut canvas = Canvas::new(area.width, area.height);
    canvas.clear(None);

    // Rings at radius/3, 2*radius/3, radius
    let r1 = radius / 3;
    let r2 = 2 * radius / 3;
    canvas.ring(centre_x, centre_y, r1, p.dim);
    canvas.ring(centre_x, centre_y, r2, p.dim);
    canvas.ring(centre_x, centre_y, radius, p.dim);

    // Eight spokes every 8th DIRS entry
    for i in (0..64).step_by(8) {
        let (dx, dy) = DIRS[i];
        // Scale from unit circle (1024) to radius
        let ex = centre_x + dx * radius / 1024;
        let ey = centre_y + dy * radius / 1024;
        canvas.line(centre_x, centre_y, ex, ey, p.dim);
    }

    // Sweep
    let idx = if ui.anim.reduce_motion {
        0
    } else {
        (ui.anim.frame_ms / 40) as usize % 64
    };

    // Current sweep direction: centre -> radius
    let (sdx, sdy) = DIRS[idx];
    let sx = centre_x + sdx * radius / 1024;
    let sy = centre_y + sdy * radius / 1024;
    canvas.line(centre_x, centre_y, sx, sy, p.accent);

    // Previous 6 directions at radius*3/4
    if !ui.anim.reduce_motion {
        let trail_r = radius * 3 / 4;
        for j in 1..=6usize {
            let prev = (idx + 64 - j) % 64;
            let (dx, dy) = DIRS[prev];
            let ex = centre_x + dx * trail_r / 1024;
            let ey = centre_y + dy * trail_r / 1024;
            canvas.line(centre_x, centre_y, ex, ey, p.dim);
        }
    }

    // Contacts: 2x2 pixel blocks
    for (i, &(ci, cr_pct)) in CONTACTS.iter().enumerate() {
        let (cdx, cdy) = DIRS[ci];
        let r = radius * cr_pct as i32 / 100;
        let cx = centre_x + cdx * r / 1024;
        let cy = centre_y + cdy * r / 1024;

        let flash = (ui.anim.frame_ms / 90 + i as u64 * 11) % 64 < 32;
        let col = if flash { p.accent } else { p.text };
        // A 2x2 outline covers exactly the four pixels of the contact block.
        canvas.rect(cx, cy, 2, 2, col);
    }

    // Pulse ring
    if let Some(phase) = ui.anim.pulse_phase {
        let pulse_r = (radius as f64 * phase) as i32;
        if pulse_r > 0 && pulse_r <= radius {
            canvas.ring(centre_x, centre_y, pulse_r, p.amber);
        }
        let dim_r = (radius as f64 * phase * 0.7) as i32;
        if dim_r > 0 && dim_r <= radius {
            canvas.ring(centre_x, centre_y, dim_r, p.dim);
        }
    }

    // Centre 3x3 '+'
    canvas.hline(centre_x - 1, centre_x + 1, centre_y, p.amber);
    canvas.vline(centre_x, centre_y - 1, centre_y + 1, p.amber);

    canvas.blit(
        frame.buffer_mut(),
        area,
        ui.settings.color_depth,
        ui.ascii,
        p.bg,
    );
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn dirs_has_64_entries() {
        super::DIRS.iter().for_each(|&(dx, dy)| {
            let r_sq = (dx * dx + dy * dy) as f64;
            let r = r_sq.sqrt();
            let target = 1024.0;
            let pct = ((r - target).abs() / target) * 100.0;
            assert!(
                pct < 1.0,
                "DIR radius {}% off: ({}, {}) → r={}",
                pct,
                dx,
                dy,
                r
            );
        });
        assert_eq!(super::DIRS.len(), 64);
    }

    #[test]
    fn radar_radius_uses_full_height() {
        // 80x24 SONAR disc is 34 cells wide by 5 tall; the old half-height
        // formula gave 3, the plan wants 8.
        assert_eq!(super::radius_for(34, 5), 8);
        // Square-ish area is height-limited by the full pixel height.
        assert_eq!(super::radius_for(80, 20), 38);
        assert_eq!(super::radius_for(10, 2), 2);
    }
}
