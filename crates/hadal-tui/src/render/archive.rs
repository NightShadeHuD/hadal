//! ARCHIVE: logs, achievement grid, and lifetime statistics.

use hadal_core::StateView;
use hadal_core::TICKS_PER_SECOND;
use hadal_core::content::story::log_text;
use hadal_core::content::tables;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::glyphs;
use crate::model;
use crate::render::{HitMap, UiState, amount, count, render_rule, render_tabs, titled_frame};
use crate::theme::Palette;

type P = Palette;

fn dim(p: P) -> Style {
    Style::new().fg(p.dim)
}
fn text(p: P) -> Style {
    Style::new().fg(p.text)
}
fn accent(p: P) -> Style {
    Style::new().fg(p.accent)
}
fn bold_amber(p: P) -> Style {
    Style::new().fg(p.amber).add_modifier(Modifier::BOLD)
}

/// Visual class of an ARCHIVE row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Header,
    LogUnlocked,
    LogLocked,
    AchievementUnlocked,
    AchievementLocked,
    Stat,
}

/// One plain-text content row (styled at render time).
#[derive(Clone, Debug)]
pub struct ArchiveRow {
    pub text: String,
    pub kind: RowKind,
}

/// Total ARCHIVE content rows for the current width.
pub fn content_rows(view: StateView<'_>, width: usize, ascii: bool) -> Vec<ArchiveRow> {
    let mut rows = Vec::new();
    let wrap_width = width.saturating_sub(8).max(16);

    rows.push(header("LOGS"));
    for (index, id) in tables::LOGS.iter().enumerate() {
        let number = index + 1;
        if view.log_recorded(*id) {
            let body = model::ascii_text(log_text(*id), ascii);
            for (line_index, line) in wrap_words(&body, wrap_width).into_iter().enumerate() {
                let prefix = if line_index == 0 {
                    format!("[{number:02}] ")
                } else {
                    "     ".to_owned()
                };
                rows.push(ArchiveRow {
                    text: format!("{prefix}{line}"),
                    kind: RowKind::LogUnlocked,
                });
            }
        } else {
            let mark = if ascii { "-" } else { "—" };
            rows.push(ArchiveRow {
                text: format!("[{number:02}] {mark}"),
                kind: RowKind::LogLocked,
            });
        }
    }

    rows.push(header("ACHIEVEMENTS   each unlocked grants +1% output"));
    for def in &tables::ACHIEVEMENTS {
        let unlocked = view.achievement_unlocked(def.id);
        let (mark, state, kind) = if unlocked {
            ("[x]", "UNLOCKED", RowKind::AchievementUnlocked)
        } else {
            ("[ ]", "LOCKED", RowKind::AchievementLocked)
        };
        rows.push(ArchiveRow {
            text: format!("{mark} {:<20} {state}", def.name),
            kind,
        });
        for line in wrap_words(&model::ascii_text(def.description, ascii), wrap_width) {
            rows.push(ArchiveRow {
                text: format!("    {line}"),
                kind,
            });
        }
    }

    rows.push(header("LIFETIME STATS"));
    rows.push(stat("best peak", amount(view.best_peak())));
    rows.push(stat("descents", count(view.descents())));
    rows.push(stat("era fathoms", count(view.era_fathoms())));
    rows.push(stat("lifetime fathoms", count(view.lifetime_fathoms())));
    rows.push(stat("manual pulses", count(view.pulses_manual())));
    rows.push(stat("auto pulses", count(view.pulses_auto())));
    rows.push(stat("purchases", count(view.purchases())));
    rows.push(stat("bands matched", count(view.bands_matched())));
    rows.push(stat("total uptime", format_uptime(view.total_ticks())));
    rows.push(stat(
        "current depth",
        format!("{} m", count(view.depth_m())),
    ));
    rows
}

fn header(text: &str) -> ArchiveRow {
    ArchiveRow {
        text: text.to_owned(),
        kind: RowKind::Header,
    }
}

fn stat(label: &str, value: String) -> ArchiveRow {
    ArchiveRow {
        text: format!("  {label:<18} {value}"),
        kind: RowKind::Stat,
    }
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Format ticks as `h:mm:ss`.
pub fn format_uptime(ticks: u64) -> String {
    let seconds = ticks / TICKS_PER_SECOND;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours}:{minutes:02}:{secs:02}")
}

/// Clamp a scroll offset after a signed delta.
pub fn clamp_scroll(offset: usize, delta: isize, total: usize, visible: usize) -> usize {
    let max = total.saturating_sub(visible);
    let next = if delta >= 0 {
        offset.saturating_add(delta as usize)
    } else {
        offset.saturating_sub(delta.unsigned_abs())
    };
    next.min(max)
}

/// Draw the ARCHIVE screen.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) -> HitMap {
    let p = ui.palette;
    let g = glyphs::set(ui.ascii);
    let area = frame.area();
    let view = ui.view;

    let recorded = tables::LOGS
        .iter()
        .filter(|id| view.log_recorded(**id))
        .count();
    let unlocked = tables::ACHIEVEMENTS
        .iter()
        .filter(|def| view.achievement_unlocked(def.id))
        .count();

    let left = Line::from(vec![
        Span::styled(
            " HADAL",
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled("ARCHIVE ", text(p)),
    ]);
    let right = Line::from(vec![
        Span::styled(format!(" LOGS {recorded}/10 "), dim(p)),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled(format!("ACH {unlocked}/11 "), dim(p)),
    ]);
    let block = titled_frame(ui.ascii, &p, left, right);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 5 {
        return HitMap::default();
    }

    let tabs = render_tabs(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        &p,
        ui.screen.tab(),
        crate::render::coach::target_tab(ui),
        ui.ascii,
    );
    let content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let coach = if ui.coach_active() {
        crate::render::coach::HEIGHT
    } else {
        0
    };
    let rows_area = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(coach),
        Constraint::Length(1),
    ])
    .areas::<4>(content);
    let (body, rule, _, footer) = (rows_area[0], rows_area[1], rows_area[2], rows_area[3]);

    // Show the signal map when the terminal is at least 100 columns wide
    // (content width is the terminal width minus the two frame border cells).
    let show_signal_map = area.width >= 100;
    let panel_width: u16 = if show_signal_map { 30 } else { 0 };
    let divider_width: u16 = if show_signal_map { 1 } else { 0 };

    let list_width = if show_signal_map {
        body.width
            .saturating_sub(panel_width)
            .saturating_sub(divider_width)
    } else {
        body.width
    };
    let list_area = if show_signal_map {
        Rect::new(body.x, body.y, list_width, body.height)
    } else {
        body
    };

    let visible = list_area.height as usize;
    let all = content_rows(view, list_width.saturating_sub(2) as usize, ui.ascii);
    let total = all.len();
    let start = ui.archive_scroll.min(total.saturating_sub(visible));
    let lines: Vec<Line> = all
        .iter()
        .skip(start)
        .take(visible)
        .map(|row| {
            let mut line = styled_row(row, p);
            let mut spans = vec![Span::styled("  ", dim(p))];
            spans.extend(line.spans);
            line = Line::from(spans);
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);

    if show_signal_map {
        let divider_area = Rect::new(
            list_area.x + list_area.width,
            list_area.y,
            divider_width,
            list_area.height,
        );
        let divider_lines: Vec<Line> = (0..divider_area.height)
            .map(|_| Line::from(Span::styled(g.v_line.to_string(), dim(p))))
            .collect();
        frame.render_widget(Paragraph::new(divider_lines), divider_area);
        let panel_area = Rect::new(
            divider_area.x + divider_area.width,
            list_area.y,
            panel_width,
            list_area.height,
        );
        render_signal_map(frame, panel_area, ui, recorded);
    }

    render_rule(frame, rule, &p, g);
    let end = start.saturating_add(visible).min(total);
    let scroll_text = format!(
        "{} {} scroll  PgUp/PgDn page  Esc back  {start}-{end}/{total}  ",
        g.arrow_up, g.arrow_down
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(scroll_text, dim(p)),
            crate::render::coach::help_span(ui, p),
        ])),
        footer,
    );

    let mut hits = HitMap {
        tabs,
        archive_total: total,
        archive_visible: visible,
        ..HitMap::default()
    };
    hits.tutorial_target = match ui.coach_target() {
        Some(crate::render::TutorialTarget::HelpKey) => Some(footer),
        _ => hits.tabs.iter().find_map(|(rect, tab)| {
            (Some(*tab) == crate::render::coach::target_tab(ui)).then_some(*rect)
        }),
    };
    hits
}

// ---------------------------------------------------------------------
// Signal map panel (width >= 100 only)
// ---------------------------------------------------------------------

/// Write one styled cell into a buffer, ignoring out-of-bounds coordinates.
fn put_cell(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

/// Draw the signal map as a 5×2 serpentine of log slots.
///
/// The wire runs left→right on even rows and right→left on odd rows, with the
/// vertical drop on alternating outer edges, so consecutive logs are joined by
/// box-drawing links rather than free-form lines.
fn render_signal_map(frame: &mut Frame, area: Rect, ui: &UiState<'_>, recorded: usize) {
    let p = ui.palette;
    let ascii = ui.ascii;
    if area.width < 30 || area.height < 8 {
        return;
    }

    // Header row.
    let header = if ascii {
        format!("SIG MAP  {}/10", count(recorded as u32))
    } else {
        format!("SIGNAL MAP  {}/10", count(recorded as u32))
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(header, bold_amber(p)))),
        Rect::new(area.x, area.y, area.width, 1),
    );

    // Five evenly spaced slot rows between the header and the legend.
    let legend_row = area.y + area.height - 1;
    let top = 1u16;
    let bottom = area.height.saturating_sub(2);
    let step = (bottom.saturating_sub(top) / 4).max(1);
    let slot_rows: [u16; 5] = [
        top,
        top + step,
        top + step * 2,
        top + step * 3,
        top + step * 4,
    ];

    let recorded_set: Vec<bool> = tables::LOGS
        .iter()
        .map(|id| ui.view.log_recorded(*id))
        .collect();
    let newest = recorded_set
        .iter()
        .enumerate()
        .filter(|(_, r)| **r)
        .map(|(i, _)| i)
        .max()
        .unwrap_or(0);

    // Serpentine order: even rows are (2r, 2r+1); odd rows swap the sides.
    let log_at = |row: usize, left: bool| -> usize {
        if row.is_multiple_of(2) {
            if left { row * 2 } else { row * 2 + 1 }
        } else if left {
            row * 2 + 1
        } else {
            row * 2
        }
    };

    let (own, locked, hot) = if ascii {
        ("##", "..", "%%")
    } else {
        ("██", "··", "▓▓")
    };
    let link_h = if ascii { '-' } else { '─' };
    let link_v = if ascii { '|' } else { '│' };
    let ring_glyph = if ascii { 'o' } else { '○' };
    let pulse_on = ui.anim.reduce_motion || (ui.anim.frame_ms % 1600) < 800;

    let left_slot = 2u16; // relative to area.x, one-cell margin from the divider
    let right_slot = 23u16;
    let slot_len = 5u16;
    let left_lane = left_slot - 1;
    let right_lane = right_slot + slot_len;

    // Horizontal links between the two slots on each row.
    for &rel_row in &slot_rows {
        let y = area.y + rel_row;
        for rel_x in (left_slot + slot_len)..right_slot {
            put_cell(frame.buffer_mut(), area.x + rel_x, y, link_h, dim(p));
        }
    }

    // Vertical links between consecutive slot rows, alternating outer edges.
    for i in 0..4usize {
        let from = slot_rows[i];
        let to = slot_rows[i + 1];
        let lane = if i.is_multiple_of(2) {
            right_lane
        } else {
            left_lane
        };
        for rel_row in (from + 1)..to {
            put_cell(
                frame.buffer_mut(),
                area.x + lane,
                area.y + rel_row,
                link_v,
                dim(p),
            );
        }
    }

    // Slots: `nn ██` recorded / `nn ··` locked; the newest recorded is amber.
    for (row, &rel_row) in slot_rows.iter().enumerate() {
        let y = area.y + rel_row;
        for left in [true, false] {
            let log = log_at(row, left);
            if log >= tables::LOGS.len() {
                continue;
            }
            let rel_x = if left { left_slot } else { right_slot };
            let is_recorded = recorded_set[log];
            let is_newest = is_recorded && log == newest;
            let pair = if is_newest {
                Span::styled(hot.to_owned(), Style::new().fg(p.amber))
            } else if is_recorded {
                Span::styled(own.to_owned(), accent(p))
            } else {
                Span::styled(locked.to_owned(), dim(p))
            };
            let number = Span::styled(format!("{:02} ", log + 1), dim(p));
            frame.render_widget(
                Paragraph::new(Line::from(vec![number, pair])),
                Rect::new(area.x + rel_x, y, slot_len, 1),
            );
            // Newest recorded: pulse a one-cell ring on the outer edge.
            if is_newest && pulse_on {
                let ring_x = area.x + if left { left_lane } else { right_lane };
                put_cell(
                    frame.buffer_mut(),
                    ring_x,
                    y,
                    ring_glyph,
                    Style::new().fg(p.accent),
                );
            }
        }
    }

    // Bottom legend.
    let legend = if ascii {
        "## recorded   .. locked"
    } else {
        "██ recorded   ·· locked"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(legend, dim(p)))),
        Rect::new(area.x, legend_row, area.width, 1),
    );
}

fn styled_row(row: &ArchiveRow, p: P) -> Line<'static> {
    let style = match row.kind {
        RowKind::Header => bold_amber(p),
        RowKind::LogUnlocked => text(p),
        RowKind::LogLocked => dim(p),
        RowKind::AchievementUnlocked => accent(p),
        RowKind::AchievementLocked => dim(p),
        RowKind::Stat => text(p),
    };
    Line::from(Span::styled(row.text.clone(), style))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_archive_does_not_leak_future_log_prose() {
        let engine = hadal_core::Engine::new();
        let rows = content_rows(engine.view(), 78, false);
        let joined = rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // Boot is recorded; the rest are not.
        assert!(joined.contains("You wake the console"));
        assert!(!joined.contains("A pulse leaves the hull"));
        assert!(!joined.contains("BALLAST RELEASED"));
        let locked_logs = rows
            .iter()
            .filter(|row| row.kind == RowKind::LogLocked)
            .count();
        assert_eq!(locked_logs, 9);
    }

    #[test]
    fn all_achievements_render_with_state_labels() {
        let engine = hadal_core::Engine::demo();
        let rows = content_rows(engine.view(), 78, false);
        let joined = rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for def in &tables::ACHIEVEMENTS {
            assert!(joined.contains(def.name), "missing {}", def.name);
        }
        assert!(joined.contains("UNLOCKED"));
        assert!(joined.contains("LOCKED"));
        let unlocked = rows
            .iter()
            .filter(|row| row.kind == RowKind::AchievementUnlocked)
            .count();
        assert!(unlocked > 0);
        // Demo is missing only Signal.
        assert!(joined.contains("Signal"));
        assert!(joined.contains("+1% output"));
    }

    #[test]
    fn stats_include_uptime_and_depth() {
        let engine = hadal_core::Engine::demo();
        let rows = content_rows(engine.view(), 78, false);
        let joined = rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("total uptime"));
        assert!(joined.contains("current depth"));
        assert!(joined.contains("100:00:00"));
        assert!(joined.contains("9,412 m"));
    }

    #[test]
    fn scroll_clamps_at_both_ends() {
        assert_eq!(clamp_scroll(0, -1, 50, 20), 0);
        assert_eq!(clamp_scroll(0, 1, 50, 20), 1);
        assert_eq!(clamp_scroll(29, 100, 50, 20), 30);
        assert_eq!(clamp_scroll(30, 100, 50, 20), 30);
        assert_eq!(clamp_scroll(5, -100, 50, 20), 0);
        assert_eq!(clamp_scroll(0, 5, 10, 20), 0);
    }

    #[test]
    fn uptime_formatting() {
        assert_eq!(format_uptime(0), "0:00:00");
        assert_eq!(format_uptime(10 * 61), "0:01:01");
        assert_eq!(format_uptime(10 * 3600), "1:00:00");
        assert_eq!(format_uptime(3_600_000), "100:00:00");
    }

    #[test]
    fn signal_map_has_ten_indexed_nodes() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        fn panel_rows(ui: &crate::render::UiState<'_>) -> Vec<String> {
            let backend = TestBackend::new(100, 30);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();

            let row_text = |y: u16| -> String {
                let mut line = String::new();
                for x in 0..buf.area().width {
                    line.push_str(buf[(x, y)].symbol());
                }
                line
            };

            let mut header_row = None;
            let mut panel_x = 0usize;
            for y in 0..buf.area().height {
                let line = row_text(y);
                if let Some(byte_pos) = line.find("SIGNAL MAP") {
                    panel_x = line[..byte_pos].chars().count();
                    header_row = Some(y);
                    break;
                }
            }
            let header_row = header_row.expect("signal map panel present at 100x30");

            let mut legend_row = None;
            for y in (0..buf.area().height).rev() {
                if row_text(y).contains("recorded") {
                    legend_row = Some(y);
                    break;
                }
            }
            let legend_row = legend_row.expect("signal map legend present");

            ((header_row + 1)..legend_row)
                .map(|y| row_text(y).chars().skip(panel_x).take(30).collect())
                .collect()
        }

        let engine = hadal_core::Engine::demo();
        let anim = crate::render::Anim::settled(engine.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );
        let ui = crate::render::UiState::settled(
            engine.view(),
            pal,
            false,
            crate::render::Screen::Archive,
            &anim,
        );

        let rows = panel_rows(&ui);
        for n in 1..=10 {
            let token = format!("{n:02}");
            let count: usize = rows.iter().map(|row| row.matches(&token).count()).sum();
            assert_eq!(count, 1, "log index {token} should appear exactly once");
        }

        let recorded = tables::LOGS
            .iter()
            .filter(|id| ui.view.log_recorded(**id))
            .count();
        let pairs: usize = rows
            .iter()
            .map(|row| row.matches("██").count() + row.matches("▓▓").count())
            .sum();
        assert_eq!(
            pairs, recorded,
            "recorded slot pairs must equal the recorded log count"
        );
    }

    #[test]
    fn archive_constellation_lights_recorded_logs() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        fn bright_cells(ui: &crate::render::UiState<'_>, pal: crate::theme::Palette) -> usize {
            let backend = TestBackend::new(100, 30);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            // Bright (accent/amber) cells carry the recorded state; locked
            // slots are dim, so they do not count as lit.
            let mut lit = 0usize;
            for y in 0..buf.area().height {
                for x in 68..98 {
                    let fg = buf[(x, y)].fg;
                    if fg == pal.accent || fg == pal.amber {
                        lit += 1;
                    }
                }
            }
            lit
        }

        let engine_demo = hadal_core::Engine::demo();
        let engine_new = hadal_core::Engine::new();
        let anim = crate::render::Anim::settled(engine_demo.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );

        let ui_demo = crate::render::UiState::settled(
            engine_demo.view(),
            pal,
            false,
            crate::render::Screen::Archive,
            &anim,
        );
        let ui_new = crate::render::UiState::settled(
            engine_new.view(),
            pal,
            false,
            crate::render::Screen::Archive,
            &anim,
        );

        let lit_demo = bright_cells(&ui_demo, pal);
        let lit_new = bright_cells(&ui_new, pal);
        assert!(
            lit_demo > lit_new,
            "demo should light more recorded slots ({lit_demo}) than new ({lit_new})"
        );
    }

    #[test]
    fn archive_constellation_hidden_below_100() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        fn screen_text(width: u16, height: u16, ui: &crate::render::UiState<'_>) -> String {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        }

        let engine = hadal_core::Engine::demo();
        let anim = crate::render::Anim::settled(engine.view().echo());
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );
        let ui = crate::render::UiState::settled(
            engine.view(),
            pal,
            false,
            crate::render::Screen::Archive,
            &anim,
        );

        let text_80 = screen_text(80, 24, &ui);
        let text_100 = screen_text(100, 30, &ui);

        assert!(
            !text_80.contains("SIGNAL MAP"),
            "80x24 should not contain SIGNAL MAP"
        );
        assert!(
            text_100.contains("SIGNAL MAP"),
            "100x30 should contain SIGNAL MAP"
        );
    }

    #[test]
    fn archive_art_deterministic_at_fixed_frame() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let engine = hadal_core::Engine::demo();
        let view = engine.view();
        let pal = crate::theme::Palette::resolve(
            crate::theme::Theme::Abyss,
            crate::theme::ColorDepth::Auto,
        );

        fn render_frame(ui: &crate::render::UiState<'_>) -> String {
            let backend = TestBackend::new(100, 30);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let _ = render(frame, ui);
                })
                .expect("draw frame");
            let buf = terminal.backend().buffer();
            let mut out = String::new();
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            out
        }

        let anim1 = crate::render::Anim {
            frame_ms: 777,
            ..crate::render::Anim::settled(view.echo())
        };
        let ui1 = crate::render::UiState::settled(
            view,
            pal,
            false,
            crate::render::Screen::Archive,
            &anim1,
        );
        let text1 = render_frame(&ui1);

        let anim2 = crate::render::Anim {
            frame_ms: 777,
            ..crate::render::Anim::settled(view.echo())
        };
        let ui2 = crate::render::UiState::settled(
            view,
            pal,
            false,
            crate::render::Screen::Archive,
            &anim2,
        );
        let text2 = render_frame(&ui2);

        assert_eq!(text1, text2, "same frame_ms must produce identical output");
    }
}
