//! SYSTEM: settings, automation toggles, save controls, version and controls.

use hadal_core::content::ids::{GeneratorId, HullId};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::glyphs;
use crate::model;
use crate::render::{
    HitMap, UiState, count, fit_tail, render_rule, render_tabs, save_age, titled_frame,
    window_start,
};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Header,
    Setting,
    Summary,
}

#[derive(Clone, Debug)]
struct SystemRow {
    text: String,
    kind: RowKind,
    focus: Option<usize>,
}

fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

fn push_setting(rows: &mut Vec<SystemRow>, index: usize, label: String, value: &str) {
    rows.push(SystemRow {
        text: format!("{label:<20} {value}"),
        kind: RowKind::Setting,
        focus: Some(index),
    });
}

fn settings_rows(ui: &UiState<'_>, width: usize) -> Vec<SystemRow> {
    let s = ui.settings;
    let mut rows = Vec::new();
    rows.push(SystemRow {
        text: "SETTINGS".to_owned(),
        kind: RowKind::Header,
        focus: None,
    });
    push_setting(&mut rows, 0, "THEME".to_owned(), s.theme.label());
    push_setting(
        &mut rows,
        1,
        "REDUCE MOTION".to_owned(),
        on_off(s.reduce_motion),
    );
    push_setting(&mut rows, 2, "ASCII GLYPHS".to_owned(), on_off(s.ascii));
    push_setting(&mut rows, 3, "MOUSE".to_owned(), on_off(s.mouse));
    push_setting(
        &mut rows,
        4,
        "COLOR DEPTH".to_owned(),
        s.color_depth.label(),
    );
    push_setting(&mut rows, 5, "TERMINAL BELL".to_owned(), on_off(s.bell));
    rows.push(SystemRow {
        text: "AUTOMATION".to_owned(),
        kind: RowKind::Header,
        focus: None,
    });
    for (offset, id) in GeneratorId::ALL.iter().enumerate() {
        push_setting(
            &mut rows,
            6 + offset,
            format!("AUTO-BUY {}", model::generator_name(*id)),
            on_off(ui.view.auto_buy(*id)),
        );
    }
    let tutorial = match ui.tutorial {
        Some(view) if view.completed => "COMPLETE".to_owned(),
        Some(view) if view.skipped => "SKIPPED".to_owned(),
        Some(view) => format!("STEP {}/13", view.index.saturating_add(1)),
        None => "UNAVAILABLE".to_owned(),
    };
    push_setting(&mut rows, 10, "TUTORIAL".to_owned(), &tutorial);
    for line in hull_summary(ui, width) {
        rows.push(SystemRow {
            text: line,
            kind: RowKind::Summary,
            focus: None,
        });
    }
    rows.push(SystemRow {
        text: "NOTE   settings apply live and save to config.json immediately".to_owned(),
        kind: RowKind::Summary,
        focus: None,
    });
    rows.push(SystemRow {
        text: "NOTE   --color from the command line is a session-only override".to_owned(),
        kind: RowKind::Summary,
        focus: None,
    });
    rows
}

/// Read-only ownership summary: owned automation hulls, not active switches.
///
/// `governor` carries its current target (`governor 3F`) or `OFF`.
fn hull_summary(ui: &UiState<'_>, width: usize) -> Vec<String> {
    let view = ui.view;
    let sep = if ui.ascii { " / " } else { " · " };
    let candidates = [
        (HullId::AutoPulse, "auto-pulse", "pulse"),
        (HullId::TunedBallast, "tuned ballast", "tune"),
        (HullId::ChainAutomator, "chain automator", "chain"),
    ];
    let mut full: Vec<String> = Vec::new();
    let mut compact: Vec<String> = Vec::new();
    for (id, name, short) in candidates {
        if view.hull_owned(id) {
            full.push(name.to_owned());
            compact.push(short.to_owned());
        }
    }
    if view.hull_owned(HullId::BallastGovernor) {
        let target = view.auto_descend_at();
        let value = if target == 0 {
            "OFF".to_owned()
        } else {
            format!("{}F", count(target))
        };
        full.push(format!("governor {value}"));
        compact.push(format!("gov {value}"));
    }
    if full.is_empty() {
        return vec!["HULLS: none".to_owned()];
    }
    let full_line = format!("HULLS: {}", full.join(sep));
    if full_line.chars().count() <= width {
        return vec![full_line];
    }
    let compact_line = format!("HULLS: {}", compact.join(" / "));
    if compact_line.chars().count() <= width {
        return vec![compact_line];
    }
    // Very narrow: pack compact items across indented lines.
    let mut lines: Vec<String> = Vec::new();
    let mut current = "HULLS: ".to_owned();
    for item in compact {
        let has_item = current.chars().count() > "HULLS: ".chars().count();
        let piece = if has_item {
            format!(" / {item}")
        } else {
            item.clone()
        };
        if has_item && current.chars().count() + piece.chars().count() > width {
            lines.push(current);
            current = format!("       {item}");
        } else {
            current.push_str(&piece);
        }
    }
    if !current.trim().is_empty() {
        lines.push(current);
    }
    lines
}

/// Draw the SYSTEM screen.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) -> HitMap {
    let p = ui.palette;
    let g = glyphs::set(ui.ascii);
    let area = frame.area();

    let left = Line::from(vec![
        Span::styled(
            " HADAL",
            Style::new().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled("SYSTEM ", text(p)),
    ]);
    let right = Line::from(vec![
        Span::styled(format!(" v{} ", env!("CARGO_PKG_VERSION")), dim(p)),
        Span::styled(format!(" {}{} ", g.h_line, g.h_line), dim(p)),
        Span::styled("RELAY K-9 ", dim(p)),
    ]);
    let block = titled_frame(ui.ascii, &p, left, right);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 8 {
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
    let rows = Layout::vertical([
        Constraint::Min(0),        // settings + automation
        Constraint::Length(1),     // rule
        Constraint::Length(3),     // save block
        Constraint::Length(1),     // version/credits
        Constraint::Length(coach), // reserved coach
        Constraint::Length(1),     // controls
    ])
    .areas::<6>(content);
    let (body, rule, save_area, version_area, _coach_area, controls_area) =
        (rows[0], rows[1], rows[2], rows[3], rows[4], rows[5]);

    let focus_index = match ui.focus {
        crate::render::Focus::System(index) => Some(index),
        _ => None,
    };
    let all = settings_rows(ui, body.width as usize);
    let focus_pos =
        focus_index.and_then(|index| all.iter().position(|row| row.focus == Some(index)));
    let visible = body.height as usize;
    let start = window_start(all.len(), visible, focus_pos);
    let lines: Vec<Line> = all
        .iter()
        .skip(start)
        .take(visible)
        .map(|row| styled_setting(row, focus_index, p))
        .collect();
    frame.render_widget(Paragraph::new(lines), body);

    let mut hits = HitMap {
        tabs,
        ..HitMap::default()
    };
    let mut y = body.y;
    for row in all.iter().skip(start).take(visible) {
        if let Some(index) = row.focus {
            hits.system
                .push((Rect::new(body.x, y, body.width, 1), index));
        }
        y = y.saturating_add(1);
    }

    render_rule(frame, rule, &p, g);

    let save_rows = Layout::vertical([Constraint::Length(1); 3]).areas::<3>(save_area);
    let path = fit_tail(
        ui.save_path,
        save_area.width.saturating_sub(11) as usize,
        ui.ascii,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("SAVE PATH  ", dim(p)),
            Span::styled(path, text(p)),
        ])),
        save_rows[0],
    );
    let status = match ui.last_save {
        Some(saved) => format!("SAVED {}", save_age(ui.now_unix, saved)),
        None => "NOT SAVED YET".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("STATUS     ", dim(p)),
            Span::styled(status, accent(p)),
        ])),
        save_rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "s save   e export   i import   R reset",
            dim(p),
        ))),
        save_rows[2],
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "HADAL v{}   -   credits: The HADAL Authors",
                env!("CARGO_PKG_VERSION")
            ),
            dim(p),
        ))),
        version_area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "CONTROLS  {}{} focus  Enter change  t toggle  Esc back  ",
                    g.arrow_up, g.arrow_down
                ),
                dim(p),
            ),
            crate::render::coach::help_span(ui, p),
        ])),
        controls_area,
    );

    hits.tutorial_target = match ui.coach_target() {
        Some(crate::render::TutorialTarget::HelpKey) => Some(controls_area),
        _ => hits.tabs.iter().find_map(|(rect, tab)| {
            (Some(*tab) == crate::render::coach::target_tab(ui)).then_some(*rect)
        }),
    };
    hits
}

fn styled_setting(row: &SystemRow, focus: Option<usize>, p: P) -> Line<'static> {
    match row.kind {
        RowKind::Header => Line::from(Span::styled(row.text.clone(), bold_amber(p))),
        RowKind::Summary => Line::from(Span::styled(row.text.clone(), dim(p))),
        RowKind::Setting => {
            let is_focused = row.focus.is_some() && row.focus == focus;
            let marker = if is_focused { "> " } else { "  " };
            let style = if is_focused { bold_amber(p) } else { text(p) };
            Line::from(vec![
                Span::styled(
                    marker.to_owned(),
                    if is_focused { bold_amber(p) } else { dim(p) },
                ),
                Span::styled(row.text.clone(), style),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ui<'a>(engine: &'a hadal_core::Engine, anim: &'a crate::render::Anim) -> UiState<'a> {
        let mut ui = UiState::settled(
            engine.view(),
            Palette::resolve(crate::theme::Theme::Abyss, crate::theme::ColorDepth::Auto),
            false,
            crate::render::Screen::System,
            anim,
        );
        ui.focus = crate::render::Focus::System(0);
        ui
    }

    fn engine_with(mutate: impl FnOnce(&mut serde_json::Value)) -> hadal_core::Engine {
        let engine = hadal_core::Engine::demo();
        let mut value = serde_json::to_value(engine.state()).expect("state json");
        mutate(&mut value);
        let state: hadal_core::GameState = serde_json::from_value(value).expect("state");
        hadal_core::Engine::from_state(state)
    }

    #[test]
    fn settings_rows_cover_all_focusable_controls() {
        let engine = hadal_core::Engine::demo();
        let anim = crate::render::Anim::settled(engine.view().echo());
        let ui = make_ui(&engine, &anim);
        let rows = settings_rows(&ui, 78);
        let focused: Vec<usize> = rows.iter().filter_map(|row| row.focus).collect();
        assert_eq!(focused, (0..11).collect::<Vec<_>>());
        let joined = rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for label in [
            "THEME",
            "REDUCE MOTION",
            "ASCII GLYPHS",
            "MOUSE",
            "COLOR DEPTH",
            "TERMINAL BELL",
            "AUTO-BUY Hydrophone",
            "AUTO-BUY Drifter",
            "AUTO-BUY Sonar Mast",
            "AUTO-BUY Tidal Core",
            "TUTORIAL",
        ] {
            assert!(joined.contains(label), "missing {label}");
        }
    }

    #[test]
    fn hull_summary_is_ownership_not_switches() {
        let engine = hadal_core::Engine::demo();
        let anim = crate::render::Anim::settled(engine.view().echo());
        let ui = make_ui(&engine, &anim);
        let joined = settings_rows(&ui, 78)
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("HULLS:"));
        assert!(joined.contains("auto-pulse"));
        assert!(joined.contains("tuned ballast"));
        assert!(joined.contains("chain automator"));
        assert!(joined.contains("governor OFF"));
        assert!(!joined.contains("AUTO   pulse ON"));

        let engine = engine_with(|value| {
            value["meta"]["auto_descend_at"] = serde_json::json!(3);
        });
        let anim = crate::render::Anim::settled(engine.view().echo());
        let ui = make_ui(&engine, &anim);
        let joined = settings_rows(&ui, 78)
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("governor 3F"), "{joined}");

        let joined = settings_rows(&ui, 30)
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("HULLS: pulse"), "{joined}");
        assert!(joined.contains("gov 3F"), "{joined}");
    }
}
