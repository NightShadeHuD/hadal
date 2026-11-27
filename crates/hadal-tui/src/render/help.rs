//! HELP overlay: a scrollable manual, keymap, rules, and glossary.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model;
use crate::render::{UiState, overlay_box};
use crate::theme::Palette;
use hadal_core::content::{story, tutorial};

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
/// Place an overlong word by hard-splitting it into `width`-sized chunks.
///
/// Full chunks are pushed to `lines`; the final partial chunk is returned
/// together with its char count so the caller can seed the next line.
fn place_overlong(word: &str, width: usize, lines: &mut Vec<String>) -> (String, usize) {
    let mut chunk = String::new();
    let mut chunk_len = 0;
    for ch in word.chars() {
        if chunk_len == width {
            lines.push(std::mem::take(&mut chunk));
            chunk_len = 0;
        }
        chunk.push(ch);
        chunk_len += 1;
    }
    (chunk, chunk_len)
}

/// Word-wrap text to the given width. Splits on whitespace; a single
/// overlong word is hard-split at the boundary (char-safe, never byte
/// slicing) and leaves the following lines correctly seeded.
fn wrap_line(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in text.split_whitespace() {
        let wlen = word.chars().count();

        if current_len == 0 {
            if wlen <= width {
                current.push_str(word);
                current_len = wlen;
            } else {
                let (chunk, clen) = place_overlong(word, width, &mut lines);
                current = chunk;
                current_len = clen;
            }
        } else if current_len + 1 + wlen <= width {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + wlen;
        } else {
            lines.push(std::mem::take(&mut current));
            if wlen <= width {
                current.push_str(word);
                current_len = wlen;
            } else {
                let (chunk, clen) = place_overlong(word, width, &mut lines);
                current = chunk;
                current_len = clen;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
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
    let mut lines = vec![section("MISSION", p)];
    for para in story::MISSION {
        for w in wrap_line(para, 72) {
            lines.push(l(&format!("  {w}")));
        }
    }
    lines.push(blank());
    lines.extend([
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
    ]);
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
    ]);
    // TUTORIAL REFERENCE
    let mut ref_lines: Vec<Line<'static>> = Vec::new();
    ref_lines.push(section("TUTORIAL REFERENCE", p));
    for ch in &tutorial::CHAPTERS {
        ref_lines.push(section(ch.title, p));
        for intro_line in wrap_line(ch.intro, 72) {
            ref_lines.push(line(&format!("  {intro_line}"), p));
        }
        ref_lines.push(blank());
        for (i, step) in tutorial::STEPS.iter().enumerate() {
            if step.chapter != ch.number {
                continue;
            }
            ref_lines.push(line(&format!("  {:02}/13  {}", i + 1, step.title), p));
            for w in wrap_line(step.instruction, 70) {
                ref_lines.push(line(&format!("    {w}"), p));
            }
            for w in wrap_line(step.why, 70) {
                ref_lines.push(line(&format!("    {w}"), p));
            }
            for w in wrap_line(step.detail, 70) {
                ref_lines.push(line(&format!("    {w}"), p));
            }
            ref_lines.push(blank());
        }
    }
    lines.extend(ref_lines);
    lines.push(blank());
    lines.push(section("VERSION", p));
    lines.push(l(&format!("  HADAL v{}", env!("CARGO_PKG_VERSION"))));
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
            "MISSION",
            "BASICS",
            "WHAT TO DO NOW",
            "CONTROLS",
            "DESCENT & FATHOMS",
            "OFFLINE",
            "NUMBERS",
            "GLOSSARY",
            "TUTORIAL REFERENCE",
            "VERSION",
        ] {
            assert!(text.contains(section), "missing {section}");
            let first = text.find("MISSION").expect("mission");
            let second = text.find("BASICS").expect("basics");
            let third = text.find("WHAT TO DO NOW").expect("now");
            let fourth = text.find("CONTROLS").expect("controls");
            let fifth = text.find("DESCENT & FATHOMS").expect("descent");
            let sixth = text.find("OFFLINE").expect("offline");
            let seventh = text.find("NUMBERS").expect("numbers");
            let _eighth = text.find("GLOSSARY").expect("glossary");
            let ninth = text.find("TUTORIAL REFERENCE").expect("tutorial ref");
            let tenth = text.find("VERSION").expect("version");
            assert!(first < second && second < third && third < fourth);
            assert!(fourth < fifth && fifth < sixth && sixth < seventh);
            assert!(seventh < ninth && ninth < tenth);
        }
        // All 13 step titles appear.
        for step in &tutorial::STEPS {
            assert!(
                text.contains(step.title),
                "missing step title {}",
                step.title
            );
        }
        // Every detail string appears (rendered wrapped, so compare on a
        // whitespace-normalized copy of the help text).
        let text_norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for step in &tutorial::STEPS {
            let normalized = step.detail.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                text_norm.contains(&normalized),
                "missing detail for step {}",
                step.title
            );
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

    #[test]
    fn wrap_line_hard_splits_overlong_word() {
        // A single word longer than the width is split into width-sized chunks.
        let word = "supercalifragilisticexpialidocious";
        let out = wrap_line(word, 10);
        assert!(
            out.iter().all(|l| l.chars().count() <= 10),
            "chunk exceeded width: {out:?}"
        );
        assert_eq!(out.concat(), word);

        // A following normal word must not duplicate leftover text.
        let out = wrap_line("abcdefghijkl ok", 5);
        assert!(out.iter().all(|l| l.chars().count() <= 5));
        let joined = out.concat();
        assert_eq!(joined.matches("abcdef").count(), 1, "duplicated: {out:?}");
        assert!(out.last().is_some_and(|l| l.ends_with("ok")));
    }

    #[test]
    fn wrap_line_exact_width() {
        assert_eq!(
            wrap_line("abcde fghij", 5),
            vec!["abcde".to_string(), "fghij".to_string()]
        );
        assert_eq!(wrap_line("abcde", 5), vec!["abcde".to_string()]);
        assert_eq!(wrap_line("ab cd", 5), vec!["ab cd".to_string()]);
        // Multibyte input must stay char-safe (no byte slicing / panic).
        let out = wrap_line("héllo wörld", 5);
        assert_eq!(out, vec!["héllo".to_string(), "wörld".to_string()]);
    }

    #[test]
    fn tutorial_reference_renders_and_fits_width() {
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
        let lines: Vec<String> = help_lines(&ui).iter().map(|l| l.to_string()).collect();
        let start = lines
            .iter()
            .position(|l| l.contains("TUTORIAL REFERENCE"))
            .expect("tutorial reference section");
        let end = lines
            .iter()
            .position(|l| l.contains("VERSION"))
            .expect("version section");
        assert!(start < end);

        for l in &lines[start..end] {
            assert!(
                l.chars().count() <= 74,
                "tutorial reference line is {} cols (max 74): {l:?}",
                l.chars().count()
            );
        }

        // Both chapter intros and every step's copy must be present.
        let norm = lines[start..end]
            .join("\n")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for ch in &tutorial::CHAPTERS {
            let intro = ch.intro.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(norm.contains(&intro), "chapter intro missing: {}", ch.title);
        }
        for step in &tutorial::STEPS {
            for (label, text) in [
                ("instruction", step.instruction),
                ("why", step.why),
                ("detail", step.detail),
            ] {
                let needle = text.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(
                    norm.contains(&needle),
                    "step {} {label} missing",
                    step.title
                );
            }
        }
    }

    #[test]
    fn tutorial_reference_numbers_steps_by_position() {
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
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("  01/13  "), "missing 01/13 marker");
        assert!(text.contains("  13/13  "), "missing 13/13 marker");
    }
}
