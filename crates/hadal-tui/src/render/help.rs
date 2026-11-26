//! HELP overlay: a scrollable briefing, keymap, rules, and glossary.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model;
use crate::render::{UiState, overlay_box};
use crate::theme::Palette;

fn section(text: &str, p: Palette) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        Style::new().fg(p.amber).add_modifier(Modifier::BOLD),
    ))
}

fn line(text: &str, p: Palette) -> Line<'static> {
    Line::from(Span::styled(text.to_owned(), Style::new().fg(p.text)))
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn wrap_items(prefix: &str, items: &[&str], p: Palette) -> Vec<Line<'static>> {
    let indent = " ".repeat(prefix.chars().count());
    let mut lines = Vec::new();
    let mut current = prefix.to_owned();
    let mut has_item = false;
    for item in items {
        let addition = if has_item {
            format!(", {item}")
        } else {
            format!(" {item}")
        };
        if current.chars().count() + addition.chars().count() > 72 {
            lines.push(line(&current, p));
            current = format!("{indent}{item}");
        } else {
            current.push_str(&addition);
        }
        has_item = true;
    }
    lines.push(line(&current, p));
    lines
}

/// Help content lines.
pub fn help_lines(ui: &UiState<'_>) -> Vec<Line<'static>> {
    let p = ui.palette;
    let view = ui.view;
    let l = |text: &str| line(&model::ascii_text(text, ui.ascii), p);
    let mut lines = vec![
        section("BASICS", p),
        l("  Echo is this run's currency; generators produce it continuously."),
        l("  SPACE pulses the capacitor: 10 s of production per full pulse."),
        l("  h/H buy Hydrophones; d/D, m/M, c/C buy higher tiers."),
        l("  f tunes bands; matching the carrier is worth ×1.5."),
        l("  Descent at 1.00M Peak Echo turns the run into Fathoms."),
        blank(),
        section("WHAT TO DO NOW", p),
        l(&format!("  {}", model::objective_line(view, ui.ascii))),
        blank(),
        section("CONTROLS", p),
        l("  GLOBAL   1-4 tabs  Tab cycle  ? help  Esc back"),
        l("           Ctrl+S save  q quit"),
        l("  SONAR    Space pulse  f tune  h/H d/D m/M c/C buy"),
        l("           u chain  arrows focus  Enter buy"),
        l("  DESCENT  arrows focus  Enter buy  p then y descend"),
        l("           +/- governor"),
        l("  ARCHIVE  arrows scroll  PgUp/PgDn page"),
        l("  SYSTEM   arrows focus  Enter/Space change  t toggle"),
        l("           s save  e export  i import  R reset"),
        l("  HELP     Up/Down scroll  PgUp/PgDn page  Esc close"),
        blank(),
        section("DESCENT & FATHOMS", p),
        l("  Threshold: 1.00M Peak Echo."),
        l("  Bucket gains start at +3 Fathoms and rise with deeper Peak."),
        l("  Every Descent sinks K-9 another 500 m and starts a new run."),
    ];
    lines.extend(wrap_items("  RESETS   ", view.reset_scope(), p));
    lines.extend(wrap_items("  PERSISTS ", model::PERSIST_SCOPE, p));
    lines.extend([
        l("  Offline cap is preserved and upgraded by hulls."),
        blank(),
        section("OFFLINE", p),
        l("  Away time runs at the full production rate, automation only."),
        l("  Base cap 8 h. Long Storage 24 h, Extended Ballast 72 h,"),
        l("  Abyssal Reservoir 168 h."),
        blank(),
        section("NUMBERS", p),
        l("  Amounts floor, costs ceil; affordability is exact."),
        blank(),
        section("GLOSSARY", p),
        l("  Echo         This run's spendable currency; generators produce it."),
        l("  Rate         Echo/s from generators, tiers, upgrades and hulls."),
        l("  Peak         Highest Echo balance this run; Descent checks it."),
        l("  Pulse        SPACE converts 10 s of current production into Echo."),
        l("  Carrier      The repeating frequency band K-9 is receiving."),
        l("  Tune         f selects a band; matching the carrier is worth ×1.5."),
        l("  Generator    A producer bought with Echo; costs rise per unit."),
        l("  Signal Chain One-time upgrades bought with Echo; reset by Descent."),
        l("  Descent      Reset at 1.00M Peak to earn Fathoms and sink 500 m."),
        l("  Fathom       Permanent currency from Descent; buys hulls."),
        l("  Hull         A permanent upgrade bought with Fathoms."),
        l("  Automation   Hull-driven actions that run while you do not."),
        l("  Offline      Away time runs at full rate, capped by hull upgrades."),
        l("  Achievement  A one-time milestone worth a permanent +1% output."),
        blank(),
        section("VERSION", p),
        l(&format!("  HADAL v{}", env!("CARGO_PKG_VERSION"))),
    ]);
    lines
}

/// Draw the help overlay; returns `(total, visible)` rows for scroll clamping.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) -> (usize, usize) {
    let area = frame.area();
    let width = area.width.saturating_sub(2).clamp(50, 78);
    let height = area.height.saturating_sub(2).min(23);
    let inner = overlay_box(frame, area, width, height, ui.palette, ui.ascii, "HELP");
    if inner.height == 0 || inner.width == 0 {
        return (0, 0);
    }

    let lines = help_lines(ui);
    let total = lines.len();
    let footer = 1;
    let visible = inner.height.saturating_sub(footer) as usize;
    let scroll = ui.help_scroll.min(total.saturating_sub(visible));

    let body = Rect::new(inner.x, inner.y, inner.width, visible as u16);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll as u16, 0))
            .style(Style::new().bg(ui.palette.panel)),
        body,
    );

    let footer_area = Rect::new(
        inner.x,
        inner.y + visible as u16,
        inner.width,
        inner.height.saturating_sub(visible as u16),
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Up/Down scroll   PgUp/PgDn page   Esc close",
            Style::new().fg(ui.palette.dim),
        )))
        .style(Style::new().bg(ui.palette.panel)),
        footer_area,
    );
    (total, visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Anim;
    use crate::theme::{ColorDepth, Theme};
    use hadal_core::Engine;

    #[test]
    fn help_covers_sections_objective_and_glossary() {
        let engine = Engine::demo();
        let anim = Anim::settled(engine.view().echo());
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let mut ui = UiState::settled(
            engine.view(),
            palette,
            false,
            crate::render::Screen::Sonar,
            &anim,
        );
        ui.help = true;
        let text = help_lines(&ui)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        for section in [
            "BASICS",
            "WHAT TO DO NOW",
            "CONTROLS",
            "DESCENT & FATHOMS",
            "OFFLINE",
            "NUMBERS",
            "GLOSSARY",
            "VERSION",
        ] {
            assert!(text.contains(section), "missing {section}");
            let first = text.find("BASICS").expect("basics");
            let second = text.find("WHAT TO DO NOW").expect("now");
            let third = text.find("CONTROLS").expect("controls");
            let fourth = text.find("DESCENT & FATHOMS").expect("descent");
            let fifth = text.find("OFFLINE").expect("offline");
            let sixth = text.find("NUMBERS").expect("numbers");
            let seventh = text.find("GLOSSARY").expect("glossary");
            let eighth = text.find("VERSION").expect("version");
            assert!(first < second && second < third && third < fourth);
            assert!(fourth < fifth && fifth < sixth && sixth < seventh);
            assert!(seventh < eighth);
        }
        for entry in [
            "Echo",
            "Rate",
            "Peak",
            "Pulse",
            "Carrier",
            "Tune",
            "Generator",
            "Signal Chain",
            "Descent",
            "Fathom",
            "Hull",
            "Automation",
            "Offline",
            "Achievement",
        ] {
            let glossary = text.split("GLOSSARY").nth(1).expect("glossary");
            assert!(
                glossary.contains(&format!("  {entry}")),
                "missing glossary {entry}"
            );
        }
        assert!(text.contains("Up/Down scroll"), "footer");
        assert!(!text.contains("ANY KEY CLOSES"));
    }

    #[test]
    fn help_objective_matches_sonar_helper() {
        let engine = Engine::new();
        let anim = Anim::settled(engine.view().echo());
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let mut ui = UiState::settled(
            engine.view(),
            palette,
            false,
            crate::render::Screen::Sonar,
            &anim,
        );
        ui.help = true;
        let text = help_lines(&ui)
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let objective = model::objective_line(engine.view(), false);
        assert!(text.contains(&objective));
    }
}
