//! ARCHIVE: logs, achievement grid, and lifetime statistics.

use hadal_core::StateView;
use hadal_core::TICKS_PER_SECOND;
use hadal_core::content::story::log_text;
use hadal_core::content::tables;
use ratatui::Frame;
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

    let visible = body.height as usize;
    let all = content_rows(view, body.width.saturating_sub(2) as usize, ui.ascii);
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
    frame.render_widget(Paragraph::new(lines), body);

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
}
