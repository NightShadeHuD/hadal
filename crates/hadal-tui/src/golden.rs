//! Golden layout snapshots and render-level behavior tests.
//!
//! Snapshots capture visible characters with settled animation values and use
//! synthetic paths only, so they are stable and path-free.

use crossterm::event::MouseButton;
use hadal_core::BuyQuantity;
use hadal_core::Engine;
use hadal_core::content::story::BOOT_LINES;
use hadal_core::simulate_offline;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

use crate::input::Command;
use crate::render::hits::center;
use crate::render::{
    Anim, EditKind, EditView, Focus, HitMap, Screen, Tab, UiState, archive, command_for_click,
    render,
};
use crate::theme::{ColorDepth, Palette, Theme};

fn palette(theme: Theme) -> Palette {
    Palette::resolve(theme, ColorDepth::Auto)
}

fn buffer_text(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn draw(width: u16, height: u16, ui: &UiState<'_>) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            let _ = render(frame, ui);
        })
        .expect("draw frame");
    buffer_text(terminal.backend().buffer())
}

fn draw_hits(width: u16, height: u16, ui: &UiState<'_>) -> HitMap {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| {
            hits = render(frame, ui);
        })
        .expect("draw frame");
    hits
}

fn draw_buffer(width: u16, height: u16, ui: &UiState<'_>) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            let _ = render(frame, ui);
        })
        .expect("draw frame");
    terminal.backend().buffer().clone()
}

struct Fixture {
    engine: Engine,
    anim: Anim,
}

impl Fixture {
    fn new(engine: Engine) -> Self {
        let anim = Anim::settled(engine.view().echo());
        Self { engine, anim }
    }

    fn ui(&self, screen: Screen, ascii: bool) -> UiState<'_> {
        UiState::settled(
            self.engine.view(),
            palette(Theme::Abyss),
            ascii,
            screen,
            &self.anim,
        )
    }
}

// ---------------------------------------------------------------------
// Existing screens
// ---------------------------------------------------------------------

#[test]
fn too_small_79x24() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("too_small_79x24", draw(79, 24, &ui));
}

#[test]
fn sonar_80x24_unicode() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_80x24_unicode", draw(80, 24, &ui));
}

#[test]
fn sonar_100x30_unicode() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_100x30_unicode", draw(100, 30, &ui));
}

#[test]
fn sonar_80x24_ascii() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Sonar, true);
    insta::assert_snapshot!("sonar_80x24_ascii", draw(80, 24, &ui));
}

#[test]
fn boot_100x30() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(
        Screen::Boot {
            revealed: BOOT_LINES.len(),
        },
        false,
    );
    insta::assert_snapshot!("boot_100x30", draw(100, 30, &ui));
}

#[test]
fn briefing_80x24_unicode() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Briefing, false);
    insta::assert_snapshot!("briefing_80x24_unicode", draw(80, 24, &ui));
}

#[test]
fn briefing_80x24_ascii() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Briefing, true);
    insta::assert_snapshot!("briefing_80x24_ascii", draw(80, 24, &ui));
}

#[test]
fn briefing_100x30() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Briefing, false);
    insta::assert_snapshot!("briefing_100x30", draw(100, 30, &ui));
}

#[test]
fn demo_banner_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Sonar, false);
    ui.demo = true;
    insta::assert_snapshot!("demo_banner_100x30", draw(100, 30, &ui));
}

#[test]
fn demo_briefing_and_banner_fit_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut briefing = fixture.ui(Screen::Briefing, false);
    briefing.demo = true;
    let text = draw(80, 24, &briefing);
    assert!(text.contains("DEMO // NOTHING IS SAVED"), "{text}");
    assert!(
        text.contains("late game: 12 descents, 15 hulls, automation is on"),
        "{text}"
    );
    assert!(text.contains("the station plays itself; keys are additive"));
    assert!(text.contains("[any key] begin"), "footer clipped: {text}");

    let mut sonar = fixture.ui(Screen::Sonar, false);
    sonar.demo = true;
    let text = draw(80, 24, &sonar);
    assert!(text.contains("DEMO // NOTHING IS SAVED"), "{text}");
    assert!(text.contains("automation is on; keys additive"), "{text}");
    assert!(text.contains("? briefing   1-4 screens"), "{text}");
}

#[test]
fn descent_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(0);
    insta::assert_snapshot!("descent_80x24", draw(80, 24, &ui));
}

#[test]
fn descent_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(11);
    insta::assert_snapshot!("descent_100x30", draw(100, 30, &ui));
}

// ---------------------------------------------------------------------
// 140x40
// ---------------------------------------------------------------------

#[test]
fn sonar_140x40() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_140x40", draw(140, 40, &ui));
}

#[test]
fn descent_140x40() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(0);
    insta::assert_snapshot!("descent_140x40", draw(140, 40, &ui));
}

// ---------------------------------------------------------------------
// ARCHIVE
// ---------------------------------------------------------------------

#[test]
fn archive_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Archive, false);
    insta::assert_snapshot!("archive_80x24", draw(80, 24, &ui));
}

#[test]
fn archive_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Archive, false);
    insta::assert_snapshot!("archive_100x30", draw(100, 30, &ui));
}

#[test]
fn archive_locked_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let rows = archive::content_rows(fixture.engine.view(), 98, false);
    let locked = rows
        .iter()
        .position(|row| row.text.contains("Signal") && row.text.contains("LOCKED"))
        .expect("locked Signal achievement row");
    let mut ui = fixture.ui(Screen::Archive, false);
    ui.archive_scroll = locked.saturating_sub(6);
    insta::assert_snapshot!("archive_locked_100x30", draw(100, 30, &ui));
}

// ---------------------------------------------------------------------
// SYSTEM
// ---------------------------------------------------------------------

fn system_ui<'a>(fixture: &'a Fixture) -> UiState<'a> {
    let mut ui = fixture.ui(Screen::System, false);
    ui.focus = Focus::System(0);
    ui.save_path = "data/save.json";
    ui.now_unix = 1_000;
    ui.last_save = Some(988);
    ui
}

#[test]
fn system_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let ui = system_ui(&fixture);
    insta::assert_snapshot!("system_80x24", draw(80, 24, &ui));
}

#[test]
fn system_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let ui = system_ui(&fixture);
    insta::assert_snapshot!("system_100x30", draw(100, 30, &ui));
}

#[test]
fn system_save_failure_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = system_ui(&fixture);
    ui.banner = Some("SAVE FAILED: disk full — retrying on next autosave");
    insta::assert_snapshot!("system_save_failure_80x24", draw(80, 24, &ui));
}

// ---------------------------------------------------------------------
// Overlays and themes
// ---------------------------------------------------------------------

#[test]
fn help_80x24() {
    let fixture = Fixture::new(Engine::new());
    let mut ui = fixture.ui(Screen::Sonar, false);
    ui.help = true;
    insta::assert_snapshot!("help_80x24", draw(80, 24, &ui));
}

#[test]
fn help_glossary_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Sonar, false);
    ui.help = true;
    let index = crate::render::help::help_lines(&ui)
        .iter()
        .position(|line| line.to_string().contains("GLOSSARY"))
        .expect("glossary section");
    ui.help_scroll = index;
    insta::assert_snapshot!("help_glossary_80x24", draw(80, 24, &ui));
}

#[test]
fn import_80x24() {
    let fixture = Fixture::new(Engine::new());
    let mut ui = fixture.ui(Screen::System, false);
    ui.edit = Some(EditView {
        kind: EditKind::Import,
        buffer: "saves/import.json",
        error: None,
    });
    insta::assert_snapshot!("import_80x24", draw(80, 24, &ui));
}

#[test]
fn reset_80x24() {
    let fixture = Fixture::new(Engine::new());
    let mut ui = fixture.ui(Screen::System, false);
    ui.edit = Some(EditView {
        kind: EditKind::Reset,
        buffer: "DISC",
        error: Some("Type DISCARD exactly (case-sensitive)."),
    });
    insta::assert_snapshot!("reset_80x24", draw(80, 24, &ui));
}

#[test]
fn sonar_100x30_ascii() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, true);
    insta::assert_snapshot!("sonar_100x30_ascii", draw(100, 30, &ui));
}

#[test]
fn sonar_80x24_mono() {
    let fixture = Fixture::new(Engine::new());
    let anim = Anim::settled(fixture.engine.view().echo());
    let ui = UiState::settled(
        fixture.engine.view(),
        palette(Theme::Mono),
        false,
        Screen::Sonar,
        &anim,
    );
    insta::assert_snapshot!("sonar_80x24_mono", draw(80, 24, &ui));
    // Mono really is applied: the "H" in the title uses the mono accent.
    let buffer = draw_buffer(80, 24, &ui);
    assert_eq!(buffer[(2, 0)].symbol(), "H");
    assert_eq!(buffer[(2, 0)].fg, Color::Rgb(0xFF, 0xFF, 0xFF));
}

// ---------------------------------------------------------------------
// Behavior tests
// ---------------------------------------------------------------------

#[test]
fn too_small_boundary() {
    use crate::render::{MIN_HEIGHT, MIN_WIDTH, too_small};
    use ratatui::layout::Rect;

    assert!(too_small(Rect::new(0, 0, 79, 24)));
    assert!(too_small(Rect::new(0, 0, 80, 23)));
    assert!(!too_small(Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT)));
}

#[test]
fn selection_explainer_covers_focus_and_hover() {
    let engine = Engine::new();
    let fixture = Fixture::new(engine);
    let mut up = fixture.ui(Screen::Sonar, false);
    up.focus = Focus::Upgrade(0);
    let sonar = draw(80, 24, &up);
    assert!(
        sonar.contains("Impedance Match: all production ×2 this run"),
        "{sonar}"
    );
    assert!(sonar.contains("costs 100 Echo"), "{sonar}");

    let mut hull = fixture.ui(Screen::Descent, false);
    hull.focus = Focus::Hull(0);
    let descent = draw(80, 24, &hull);
    assert!(descent.contains("Auto-Pulse:"), "{descent}");
    assert!(descent.contains("costs 1 Fathom"), "{descent}");

    let mut hover = fixture.ui(Screen::Sonar, false);
    hover.hovered = Some(hadal_core::content::ids::GeneratorId::Hydrophone);
    let sonar = draw(80, 24, &hover);
    assert!(sonar.contains("Hydrophone: +0.75/s each"), "{sonar}");
    assert!(sonar.contains("next unit costs 10 Echo"), "{sonar}");
}

#[test]
fn demo_renders_sonar_and_descent() {
    let fixture = Fixture::new(Engine::demo());
    let sonar = draw(100, 30, &fixture.ui(Screen::Sonar, false));
    assert!(sonar.contains("SONAR"));
    assert!(sonar.contains("ECHO"));
    let mut descent_ui = fixture.ui(Screen::Descent, false);
    descent_ui.focus = Focus::Hull(0);
    let descent = draw(100, 30, &descent_ui);
    assert!(descent.contains("DESCENT"));
    assert!(descent.contains("HULL TREE"));
}

#[test]
fn mouse_clicks_map_to_the_same_commands() {
    let fixture = Fixture::new(Engine::new());
    let sonar_ui = fixture.ui(Screen::Sonar, false);
    let hits = draw_hits(100, 30, &sonar_ui);

    let (rect, id) = hits.generators[0];
    let (x, y) = center(rect);
    assert_eq!(
        command_for_click(&hits, x, y, MouseButton::Left, false),
        Some(Command::Buy(id, BuyQuantity::One))
    );
    assert_eq!(
        command_for_click(&hits, x, y, MouseButton::Right, false),
        Some(Command::Buy(id, BuyQuantity::Max))
    );
    assert_eq!(
        command_for_click(&hits, x, y, MouseButton::Left, true),
        Some(Command::Buy(id, BuyQuantity::Max))
    );

    let (tab_rect, _tab) = hits
        .tabs
        .iter()
        .find(|(_, tab)| *tab == Tab::System)
        .copied()
        .expect("system tab");
    let (tx, ty) = center(tab_rect);
    assert_eq!(
        command_for_click(&hits, tx, ty, MouseButton::Left, false),
        Some(Command::OpenSystem)
    );

    let (upgrade_rect, upgrade) = hits.upgrades.first().copied().expect("upgrade row");
    let (ux, uy) = center(upgrade_rect);
    assert_eq!(
        command_for_click(&hits, ux, uy, MouseButton::Left, false),
        Some(Command::FocusUpgradeAt(upgrade))
    );

    let demo = Fixture::new(Engine::demo());
    let descent_ui = demo.ui(Screen::Descent, false);
    let descent_hits = draw_hits(100, 30, &descent_ui);
    let (hull_rect, hull) = descent_hits.hulls.first().copied().expect("hull");
    let (hx, hy) = center(hull_rect);
    assert_eq!(
        command_for_click(&descent_hits, hx, hy, MouseButton::Left, false),
        Some(Command::FocusHullAt(hull))
    );

    let system_ui = demo.ui(Screen::System, false);
    let system_hits = draw_hits(100, 30, &system_ui);
    let (row_rect, row) = system_hits.system.first().copied().expect("system row");
    let (sx, sy) = center(row_rect);
    assert_eq!(
        command_for_click(&system_hits, sx, sy, MouseButton::Left, false),
        Some(Command::SystemFocusAt(row))
    );
}

#[test]
fn wording_defects_are_fixed() {
    let fixture = Fixture::new(Engine::new());
    let sonar = draw(80, 24, &fixture.ui(Screen::Sonar, false));
    assert!(sonar.contains("NEED"), "{sonar}");
    assert!(!sonar.contains(" SAVE"), "{sonar}");
    assert!(!sonar.contains("───DEPTH"), "{sonar}");
    assert!(sonar.contains(" DEPTH"), "{sonar}");

    let ascii = draw(100, 30, &fixture.ui(Screen::Sonar, true));
    assert!(!ascii.contains("---DEPTH"), "{ascii}");
    assert!(ascii.contains(" DEPTH"), "{ascii}");

    let demo = Fixture::new(Engine::demo());
    let descent = draw(80, 24, &demo.ui(Screen::Descent, false));
    assert!(descent.contains("1F"), "{descent}");
    assert!(!descent.contains("1 F"), "{descent}");
}

#[test]
fn confirm_descent_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(0);
    ui.confirm_descent = true;
    insta::assert_snapshot!("confirm_descent_80x24", draw(80, 24, &ui));
}

#[test]
fn offline_summary_80x24() {
    let mut engine = Engine::demo();
    let report = simulate_offline(&mut engine, 8 * 3600);
    let anim = Anim::settled(engine.view().echo());
    let mut ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &anim,
    );
    ui.offline = Some(&report);
    insta::assert_snapshot!("offline_summary_80x24", draw(80, 24, &ui));
}

#[test]
fn confirmation_overlay_renders_at_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.confirm_descent = true;
    let text = draw(80, 24, &ui);

    assert!(text.contains("CONFIRM DESCENT"));
    assert!(text.contains("Descent grants"));
    assert!(text.contains("[y] confirm descent"));
    assert!(text.contains("[Esc] cancel"));
    for item in fixture.engine.view().reset_scope() {
        assert!(text.contains(item), "overlay missing reset {item}");
    }
    for item in crate::model::PERSIST_SCOPE {
        assert!(text.contains(item), "overlay missing persist {item}");
    }
}

#[test]
fn dive_renders_count_up_depth() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(
        Screen::Dive {
            from_m: 7_912,
            to_m: 8_412,
            progress: 500,
        },
        false,
    );
    let text = draw(80, 24, &ui);
    assert!(text.contains("8,162 m"), "{text}");
    assert!(text.contains("BALLAST RELEASED"));
    assert!(text.contains("any key skips"));
}

#[test]
fn huge_overlay_string_is_clipped() {
    let fixture = Fixture::new(Engine::demo());
    let huge = "x".repeat(10_000);
    let anim = Anim::settled(fixture.engine.view().echo());
    let mut ui = UiState::settled(
        fixture.engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::System,
        &anim,
    );
    ui.edit = Some(EditView {
        kind: EditKind::Import,
        buffer: &huge,
        error: None,
    });

    let text = draw(80, 24, &ui);
    assert!(text.contains("Enter submit"), "overlay rendered");
    let longest = text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    assert!(longest <= 80, "clipped to screen width: {longest}");
}

// ---------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------

fn assert_buffer_ascii(buffer: &Buffer, label: &str, width: u16, height: u16) {
    for y in 0..height {
        for x in 0..width {
            let symbol = buffer[(x, y)].symbol();
            assert!(
                symbol.is_ascii(),
                "{label} {width}x{height} at {x},{y}: {symbol:?}"
            );
        }
    }
}

#[test]
fn resize_storm_renders_every_screen_safely() {
    let fixture = Fixture::new(Engine::demo());
    let sizes = [
        (20u16, 10u16),
        (40, 12),
        (60, 20),
        (79, 24),
        (80, 23),
        (80, 24),
        (81, 25),
        (90, 26),
        (100, 30),
        (110, 32),
        (120, 35),
        (130, 38),
        (140, 40),
        (150, 45),
        (160, 50),
        (170, 55),
        (180, 60),
        (200, 60),
        (79, 40),
        (100, 24),
    ];
    for (width, height) in sizes {
        for screen in [
            Screen::Sonar,
            Screen::Descent,
            Screen::Archive,
            Screen::System,
        ] {
            let mut ui = fixture.ui(screen, false);
            ui.focus = match screen {
                Screen::Descent => Focus::Hull(0),
                Screen::System => Focus::System(0),
                _ => Focus::None,
            };
            let hits = draw_hits(width, height, &ui);
            for (rect, _) in &hits.tabs {
                assert!(
                    rect.x.saturating_add(rect.width) <= width,
                    "tab outside width at {width}x{height} {screen:?}"
                );
                assert!(
                    rect.y.saturating_add(rect.height) <= height,
                    "tab outside height at {width}x{height} {screen:?}"
                );
            }
            let text = draw(width, height, &ui);
            if width < 80 || height < 24 {
                assert!(
                    text.contains("TERMINAL TOO SMALL"),
                    "{width}x{height} {screen:?}"
                );
                assert!(
                    hits.tabs.is_empty(),
                    "tabs on too-small {width}x{height} {screen:?}"
                );
            } else {
                assert!(
                    !text.contains("TERMINAL TOO SMALL"),
                    "{width}x{height} {screen:?}"
                );
                assert_eq!(
                    hits.tabs.len(),
                    4,
                    "expected four tabs at {width}x{height} {screen:?}"
                );
            }
        }
    }
}

#[test]
fn ascii_mode_never_emits_non_ascii() {
    let fixture = Fixture::new(Engine::demo());
    let screens = [
        Screen::Sonar,
        Screen::Descent,
        Screen::Archive,
        Screen::System,
        Screen::Boot {
            revealed: BOOT_LINES.len(),
        },
        Screen::Briefing,
    ];
    for (width, height) in [(80u16, 24u16), (100, 30), (140, 40)] {
        for screen in screens {
            let mut ui = fixture.ui(screen, true);
            ui.focus = match screen {
                Screen::Descent => Focus::Hull(0),
                Screen::System => Focus::System(0),
                _ => Focus::None,
            };
            assert_buffer_ascii(&draw_buffer(width, height, &ui), "screen", width, height);
        }
    }

    // Overlays.
    let mut ui = fixture.ui(Screen::Sonar, true);
    ui.help = true;
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "help", 80, 24);

    let mut ui = fixture.ui(Screen::System, true);
    ui.edit = Some(EditView {
        kind: EditKind::Import,
        buffer: "saves/import.json",
        error: None,
    });
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "import", 80, 24);

    let mut ui = fixture.ui(Screen::Descent, true);
    ui.confirm_descent = true;
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "confirm", 80, 24);

    let mut ui = fixture.ui(Screen::System, true);
    ui.edit = Some(EditView {
        kind: EditKind::Reset,
        buffer: "DISC",
        error: Some("Type DISCARD exactly."),
    });
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "reset", 80, 24);

    // The offline summary must also be pure ASCII.
    let mut engine = Engine::demo();
    let report = simulate_offline(&mut engine, 120);
    let anim = Anim::settled(engine.view().echo());
    let mut ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        true,
        Screen::Sonar,
        &anim,
    );
    ui.offline = Some(&report);
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "offline", 80, 24);
}

#[test]
fn mono_and_reduced_color_palettes_render() {
    let fixture = Fixture::new(Engine::demo());
    for palette in [
        Palette::resolve(Theme::Mono, ColorDepth::Ansi16),
        Palette::resolve(Theme::Mono, ColorDepth::None),
        Palette::resolve(Theme::AmberCrt, ColorDepth::Ansi256),
    ] {
        for screen in [
            Screen::Sonar,
            Screen::Descent,
            Screen::Archive,
            Screen::System,
        ] {
            let anim = Anim::settled(fixture.engine.view().echo());
            let mut ui = UiState::settled(fixture.engine.view(), palette, false, screen, &anim);
            ui.focus = match screen {
                Screen::Descent => Focus::Hull(0),
                Screen::System => Focus::System(0),
                _ => Focus::None,
            };
            let text = draw(80, 24, &ui);
            assert!(!text.is_empty(), "{screen:?} rendered");
        }
    }
}

#[test]
fn control_characters_in_paths_render_inert() {
    let weird = "base/\x1b[31mred\r\n\0tail/save.json";
    let clean = crate::sanitize::sanitize(weird);
    assert!(!clean.contains('\x1b'));

    let fixture = Fixture::new(Engine::new());
    let anim = Anim::settled(fixture.engine.view().echo());
    let mut ui = UiState::settled(
        fixture.engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::System,
        &anim,
    );
    ui.save_path = &clean;
    ui.edit = Some(EditView {
        kind: EditKind::Export,
        buffer: &clean,
        error: None,
    });

    let buffer = draw_buffer(80, 24, &ui);
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            assert!(!symbol.contains('\x1b'), "escape at {x},{y}");
            assert!(!symbol.contains('\r'), "carriage return at {x},{y}");
            assert!(!symbol.contains('\n'), "newline at {x},{y}");
            assert!(!symbol.contains('\0'), "NUL at {x},{y}");
        }
    }
    assert!(
        buffer_text(&buffer).contains('\u{FFFD}'),
        "sanitized path visible"
    );
}
