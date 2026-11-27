//! Golden layout snapshots and render-level behavior tests.
//!
//! Snapshots capture visible characters with settled animation values and use
//! synthetic paths only, so they are stable and path-free.

use crossterm::event::MouseButton;
use hadal_core::BuyQuantity;
use hadal_core::Engine;
use hadal_core::GameState;
use hadal_core::content::story::{BOOT_BEATS, BOOT_LINES};
use hadal_core::simulate_offline;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::boot::{WORDMARK_ASCII, WORDMARK_UNICODE};
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

fn engine_with(engine: Engine, mutate: impl FnOnce(&mut serde_json::Value)) -> Engine {
    let mut value = serde_json::to_value(engine.state()).expect("state json");
    mutate(&mut value);
    let state: GameState = serde_json::from_value(value).expect("state");
    Engine::from_state(state)
}

fn coach_ui(fixture: &Fixture, screen: Screen, ascii: bool) -> UiState<'_> {
    let mut ui = fixture.ui(screen, ascii);
    ui.tutorial = Some(fixture.engine.tutorial());
    ui
}

// ---------------------------------------------------------------------
// Tutorial coach
// ---------------------------------------------------------------------

#[test]
fn skip_confirm_80x24() {
    let fixture = Fixture::new(Engine::new());
    let mut ui = coach_ui(&fixture, Screen::Sonar, false);
    ui.confirm_skip = true;
    insta::assert_snapshot!("skip_confirm_80x24", draw(80, 24, &ui));
}

#[test]
fn system_tutorial_80x24() {
    let fixture = Fixture::new(Engine::new());
    let mut ui = coach_ui(&fixture, Screen::System, false);
    ui.focus = Focus::System(10);
    insta::assert_snapshot!("system_tutorial_80x24", draw(80, 24, &ui));
}

fn tutorial_engine(step: u8) -> Engine {
    engine_with(Engine::demo(), |value| {
        value["meta"]["tutorial"] = serde_json::json!({
            "step": step,
            "completed": false,
            "skipped": false,
            "seen_carrier_change": true,
            "tuned_since_change": true,
            "manual_opened": true,
            "manual_closed": true,
            "system_opened": true,
            "acknowledged": false,
        });
    })
}

#[test]
fn generator_columns_align_with_fixed_gutter() {
    let fixture = Fixture::new(tutorial_engine(3));
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let text = draw(80, 24, &ui);
    let lines: Vec<&str> = text.lines().collect();
    let header = lines
        .iter()
        .find(|line| line.contains("GENERATORS"))
        .expect("generator header");
    let header_chars: Vec<char> = header.chars().collect();
    let owned_char = header_chars
        .windows(5)
        .position(|window| window == ['O', 'W', 'N', 'E', 'D'])
        .expect("OWNED header")
        .saturating_sub(1);
    for name in ["Hydrophone", "Drifter", "Sonar Mast", "Tidal Core"] {
        let row = lines
            .iter()
            .find(|line| {
                line.contains(name) && line.contains("NEED")
                    || line.contains(name) && line.contains("MAX")
            })
            .expect("generator row");
        let chars: Vec<char> = row.chars().collect();
        let field: String = chars[owned_char..owned_char + 6].iter().collect();
        assert!(
            field.trim().parse::<u32>().is_ok(),
            "{name}: owned field {field:?} in {row:?}"
        );
    }
    assert!(
        text.contains("  h/H Hydrophone") || text.contains("  h/H"),
        "non-target rows use the blank gutter"
    );
    assert!(text.contains("▶ d/D"), "target row uses the marked gutter");
}

#[test]
fn coach_band_and_target_never_overlap() {
    for &(width, height) in &[(80u16, 24u16), (100, 30)] {
        for step in 0u8..=12 {
            let engine = tutorial_engine(step);
            let fixture = Fixture::new(engine);
            for screen in [Screen::Sonar, Screen::Descent, Screen::System] {
                let ui = coach_ui(&fixture, screen, false);
                let area = ratatui::layout::Rect::new(0, 0, width, height);
                let coach = crate::render::coach::band_rect(&ui, area).expect("active coach");
                let hits = draw_hits(width, height, &ui);
                if step == 4 && screen == Screen::Sonar {
                    let text = draw(width, height, &ui);
                    assert!(text.contains("SIGNAL CHAIN"), "{width}x{height}: {text}");
                    assert!(text.contains("Impedance Match"), "{width}x{height}: {text}");
                }
                // Steps whose target has no rect on this screen are skipped;
                // tab targets still have a rect.
                let Some(target) = hits.tutorial_target else {
                    continue;
                };
                let overlap = coach.x < target.x.saturating_add(target.width)
                    && target.x < coach.x.saturating_add(coach.width)
                    && coach.y < target.y.saturating_add(target.height)
                    && target.y < coach.y.saturating_add(coach.height);
                assert!(
                    !overlap,
                    "step {step} {width}x{height} {screen:?}: coach {coach:?} overlaps target {target:?}"
                );
                let inner = ratatui::layout::Rect::new(1, 1, width - 2, height - 2);
                assert!(
                    target.x >= inner.x
                        && target.y >= inner.y
                        && target.x.saturating_add(target.width)
                            <= inner.x.saturating_add(inner.width)
                        && target.y.saturating_add(target.height)
                            <= inner.y.saturating_add(inner.height),
                    "step {step} {width}x{height} {screen:?}: target {target:?} outside inner {inner:?}"
                );
            }
        }
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

fn anim_with(echo: hadal_core::Decimal, frame_ms: u64, pulse: Option<f64>) -> Anim {
    let mut anim = Anim::settled(echo);
    anim.frame_ms = frame_ms;
    anim.pulse_phase = pulse;
    anim
}

fn count_radar(buffer: &Buffer, rect: ratatui::layout::Rect, pal: Palette) -> (usize, usize) {
    let mut non_space = 0;
    let mut colored = 0;
    for y in rect.y..rect.y.saturating_add(rect.height) {
        for x in rect.x..rect.x.saturating_add(rect.width) {
            let cell = &buffer[(x, y)];
            if cell.symbol() != " " {
                non_space += 1;
            }
            if cell.fg == pal.accent || cell.fg == pal.amber {
                colored += 1;
            }
        }
    }
    (non_space, colored)
}

#[test]
fn sonar_graphical_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_graphical_80x24", draw(80, 24, &ui));
}

#[test]
fn sonar_graphical_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_graphical_100x30", draw(100, 30, &ui));
}

#[test]
fn sonar_graphical_140x40() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, false);
    insta::assert_snapshot!("sonar_graphical_140x40", draw(140, 40, &ui));
}

#[test]
fn sonar_graphical_ascii_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let ui = fixture.ui(Screen::Sonar, true);
    insta::assert_snapshot!("sonar_graphical_ascii_80x24", draw(80, 24, &ui));
}

#[test]
fn sonar_graphical_mono_80x24() {
    let engine = Engine::demo();
    let anim = Anim::settled(engine.view().echo());
    let ui = UiState::settled(
        engine.view(),
        palette(Theme::Mono),
        false,
        Screen::Sonar,
        &anim,
    );
    insta::assert_snapshot!("sonar_graphical_mono_80x24", draw(80, 24, &ui));
    // Mono is applied: the title "H" uses the mono accent.
    let buffer = draw_buffer(80, 24, &ui);
    assert_eq!(buffer[(2, 0)].symbol(), "H");
    assert_eq!(buffer[(2, 0)].fg, Color::Rgb(0xFF, 0xFF, 0xFF));
}

#[test]
fn render_80x24_has_graphical_radar() {
    // Post-tutorial: the hero disc is 5 rows (34x5, radius 8).
    let engine = Engine::demo();
    let anim = anim_with(engine.view().echo(), 0, Some(0.8));
    let ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &anim,
    );
    let hits = draw_hits(80, 24, &ui);
    let rect = hits.radar.expect("radar rect at 80x24");
    assert_eq!(rect.height, 5, "post-tutorial disc is 5 rows");
    let buffer = draw_buffer(80, 24, &ui);
    let (non_space, colored) = count_radar(&buffer, rect, palette(Theme::Abyss));
    assert!(non_space >= 60, "80x24 radar lit cells: {non_space}");
    assert!(colored >= 15, "80x24 radar accent/amber cells: {colored}");
}

#[test]
fn radar_visible_while_coach_active() {
    // Coach active at 80x24: the band costs a row, so the disc is 4 rows but
    // must still be visibly drawn.
    let fixture = Fixture::new(Engine::new());
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let hits = draw_hits(80, 24, &ui);
    let rect = hits.radar.expect("radar rect with coach band");
    assert_eq!(rect.height, 4, "coach-active disc is 4 rows");
    let buffer = draw_buffer(80, 24, &ui);
    let (non_space, _colored) = count_radar(&buffer, rect, palette(Theme::Abyss));
    assert!(non_space >= 30, "coach-active radar lit cells: {non_space}");
}

#[test]
fn render_100x30_has_more_radar_rows_than_80() {
    let engine = Engine::demo();
    let anim = anim_with(engine.view().echo(), 0, Some(1.0));
    let ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &anim,
    );
    let hits100 = draw_hits(100, 30, &ui);
    let rect100 = hits100.radar.expect("radar rect at 100x30");
    let buffer = draw_buffer(100, 30, &ui);
    let (non_space, colored) = count_radar(&buffer, rect100, palette(Theme::Abyss));
    assert!(non_space >= 120, "100x30 radar lit cells: {non_space}");
    assert!(colored >= 30, "100x30 radar accent/amber cells: {colored}");

    let hits80 = draw_hits(80, 24, &ui);
    let rect80 = hits80.radar.expect("radar rect at 80x24");
    assert!(
        rect100.height > rect80.height,
        "radar height 100x30 {} should exceed 80x24 {}",
        rect100.height,
        rect80.height
    );
}

#[test]
fn render_is_deterministic_at_fixed_frame() {
    let engine = Engine::demo();
    let anim = anim_with(engine.view().echo(), 1234, None);
    let ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &anim,
    );
    let first = draw(100, 30, &ui);
    let second = draw(100, 30, &ui);
    assert_eq!(first, second, "fixed frame must render byte-identically");
}

#[test]
fn radar_reduce_motion_freezes_sweep() {
    let engine = Engine::demo();
    // Contacts repeat every 64*90 ms; reduce_motion pins the sweep at index 0
    // and drops the trail, so these two frames must be identical.
    let mut a0 = anim_with(engine.view().echo(), 0, None);
    a0.reduce_motion = true;
    let mut a1 = anim_with(engine.view().echo(), 90 * 64, None);
    a1.reduce_motion = true;
    let ui0 = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &a0,
    );
    let ui1 = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Sonar,
        &a1,
    );
    let hits = draw_hits(80, 24, &ui0);
    let rect = hits.radar.expect("radar rect");
    let b0 = draw_buffer(80, 24, &ui0);
    let b1 = draw_buffer(80, 24, &ui1);
    for y in rect.y..rect.y.saturating_add(rect.height) {
        for x in rect.x..rect.x.saturating_add(rect.width) {
            assert_eq!(
                b0[(x, y)].symbol(),
                b1[(x, y)].symbol(),
                "radar moved under reduce_motion at {x},{y}"
            );
        }
    }
}

#[test]
fn boot_graphical_100x30() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 3 }, false);
    insta::assert_snapshot!("boot_graphical_100x30", draw(100, 30, &ui));
}

#[test]
fn boot_graphical_stage1_80x24() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 1 }, false);
    insta::assert_snapshot!("boot_graphical_stage1_80x24", draw(80, 24, &ui));
}

#[test]
fn boot_graphical_ascii_80x24() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 3 }, true);
    insta::assert_snapshot!("boot_graphical_ascii_80x24", draw(80, 24, &ui));
}

#[test]
fn boot_graphical_stage0_80x24() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 0 }, false);
    let text = draw(80, 24, &ui);
    // No story beat is visible before the first stage.
    for beat in BOOT_BEATS {
        assert!(!text.contains(beat), "stage 0 shows beat: {beat:?}");
    }
    // The three-row wordmark is still drawn.
    for row in crate::boot::WORDMARK_UNICODE {
        let trimmed = row.trim_end();
        assert!(
            text.contains(trimmed),
            "stage 0 missing wordmark row: {trimmed:?}"
        );
    }
    insta::assert_snapshot!("boot_graphical_stage0_80x24", text);
}

#[test]
fn boot_has_no_paragraph() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 3 }, false);
    let text = draw(100, 30, &ui);
    for line in BOOT_LINES {
        assert!(!text.contains(line), "boot shows old boot line: {line:?}");
    }
    // No period-terminated sentence longer than ~50 characters.
    for line in text.lines() {
        let mut start = 0usize;
        for (i, ch) in line.char_indices() {
            if ch == '.' {
                let len = line[start..i].trim().chars().count();
                assert!(len <= 50, "long period sentence in {line:?}");
                start = i + 1;
            }
        }
    }
    for beat in BOOT_BEATS {
        assert!(text.contains(beat), "missing boot beat: {beat:?}");
    }
}

#[test]
fn boot_art_deterministic_at_fixed_frame() {
    let engine = Engine::new();
    let mut anim = Anim::settled(engine.view().echo());
    anim.frame_ms = 1337;
    let ui = UiState::settled(
        engine.view(),
        palette(Theme::Abyss),
        false,
        Screen::Boot { revealed: 3 },
        &anim,
    );
    let a = draw(100, 30, &ui);
    let b = draw(100, 30, &ui);
    assert_eq!(a, b, "boot art must be byte-identical at a fixed frame");
}

#[test]
fn boot_ascii_is_pure() {
    let fixture = Fixture::new(Engine::new());
    let ui = fixture.ui(Screen::Boot { revealed: 3 }, true);
    assert_buffer_ascii(&draw_buffer(80, 24, &ui), "boot", 80, 24);
}

#[test]
fn boot_wordmark_rows_present() {
    let fixture = Fixture::new(Engine::new());
    // Unicode art: all three rows, in order, with no radar glyphs between.
    for (width, height) in [(80u16, 24u16), (100, 30)] {
        let ui = fixture.ui(Screen::Boot { revealed: 3 }, false);
        let text = draw(width, height, &ui);
        let lines: Vec<&str> = text.lines().collect();
        let mut last = 0usize;
        for pattern in WORDMARK_UNICODE {
            let needle = pattern.trim_end();
            let pos = lines
                .iter()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("missing wordmark row {needle:?} at {width}x{height}"));
            assert!(
                pos >= last,
                "wordmark rows out of order at {width}x{height}"
            );
            last = pos;
            let row = lines[pos];
            assert!(
                row.chars().all(|c| c == ' ' || c == '█'),
                "non-wordmark glyph in row {row:?} at {width}x{height}"
            );
        }
    }
    // ASCII art: all three rows appear.
    let ui = fixture.ui(Screen::Boot { revealed: 3 }, true);
    let text = draw(80, 24, &ui);
    let lines: Vec<&str> = text.lines().collect();
    for pattern in WORDMARK_ASCII {
        let needle = pattern.trim_end();
        assert!(
            lines.iter().any(|line| line.contains(needle)),
            "missing ascii wordmark row {needle:?}"
        );
    }
}

#[test]
fn demo_graphical_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Sonar, false);
    ui.demo = true;
    insta::assert_snapshot!("demo_graphical_100x30", draw(100, 30, &ui));
}

#[test]
fn demo_banner_fits_80x24() {
    let fixture = Fixture::new(Engine::demo());
    let mut sonar = fixture.ui(Screen::Sonar, false);
    sonar.demo = true;
    let text = draw(80, 24, &sonar);
    assert!(text.contains("DEMO // NOTHING IS SAVED"), "{text}");
    assert!(text.contains("automation is on; keys additive"), "{text}");
    assert!(text.contains("? manual   1-4 screens"), "{text}");
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
fn descent_140x40() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(0);
    insta::assert_snapshot!("descent_140x40", draw(140, 40, &ui));
}

#[test]
fn descent_hull_schematic_100x30() {
    let fixture = Fixture::new(Engine::demo());
    let mut ui = fixture.ui(Screen::Descent, false);
    ui.focus = Focus::Hull(0);
    insta::assert_snapshot!("descent_hull_schematic_100x30", draw(100, 30, &ui));
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
    // At 100 columns the signal-map panel takes 30 cols + a divider, so the
    // list wraps at 67; content_rows excludes the two-cell gutter (65).
    let rows = archive::content_rows(fixture.engine.view(), 65, false);
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
    let sonar = draw(100, 30, &up);
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
    let sonar = draw(100, 30, &hover);
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
        Screen::Boot { revealed: 3 },
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

// ---------------------------------------------------------------------
// Tutorial coach graphical tests
// ---------------------------------------------------------------------

#[test]
fn scrim_dims_outside_target_and_keeps_radar() {
    let fixture = Fixture::new(Engine::new());
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let buffer = draw_buffer(80, 24, &ui);
    let hits = draw_hits(80, 24, &ui);
    let p = palette(Theme::Abyss);

    // A generator-row cell outside the target must be dimmed by the scrim.
    let gen_rect = hits
        .generators
        .first()
        .map(|(rect, _)| *rect)
        .expect("generator rows are drawn");
    let mut dim_in_generator = false;
    for y in gen_rect.y..gen_rect.y.saturating_add(gen_rect.height) {
        for x in gen_rect.x..gen_rect.x.saturating_add(gen_rect.width) {
            let cell = &buffer[(x, y)];
            if cell.fg == p.dim && cell.modifier.contains(Modifier::DIM) {
                dim_in_generator = true;
            }
        }
    }
    assert!(
        dim_in_generator,
        "generator-row cell outside target is dimmed"
    );

    // A radar-rect cell keeps its original fg (not dimmed by the scrim).
    let radar = hits.radar.expect("radar rect at 80x24 with coach");
    let mut radar_preserved = false;
    for y in radar.y..radar.y.saturating_add(radar.height) {
        for x in radar.x..radar.x.saturating_add(radar.width) {
            if buffer[(x, y)].fg != p.dim {
                radar_preserved = true;
            }
        }
    }
    assert!(radar_preserved, "radar cells keep their fg");

    // A wave-rect cell keeps its fg.
    let wave = hits.wave.expect("wave rect at 80x24 with coach");
    let mut wave_preserved = false;
    for y in wave.y..wave.y.saturating_add(wave.height) {
        for x in wave.x..wave.x.saturating_add(wave.width) {
            if buffer[(x, y)].fg != p.dim {
                wave_preserved = true;
            }
        }
    }
    assert!(wave_preserved, "wave cells keep their fg");
}

#[test]
fn target_border_is_drawn() {
    let fixture = Fixture::new(Engine::new());
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let hits = draw_hits(80, 24, &ui);
    let buffer = draw_buffer(80, 24, &ui);
    let p = palette(Theme::Abyss);
    let target = hits.tutorial_target.expect("step 0 has target rect");
    let left = target.x;
    let right = target.x.saturating_add(target.width).saturating_sub(1);
    let top = target.y;
    let bottom = target.y.saturating_add(target.height).saturating_sub(1);
    // Check top edge
    for x in left..=right {
        let cell = &buffer[(x, top)];
        assert_eq!(cell.fg, p.amber, "top edge {x},{top}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "top edge {x},{top} bold"
        );
    }
    // Check bottom edge
    for x in left..=right {
        let cell = &buffer[(x, bottom)];
        assert_eq!(cell.fg, p.amber, "bottom edge {x},{bottom}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "bottom edge {x},{bottom} bold"
        );
    }
    // Check left edge
    for y in top..=bottom {
        let cell = &buffer[(left, y)];
        assert_eq!(cell.fg, p.amber, "left edge {left},{y}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "left edge {left},{y} bold"
        );
    }
    // Check right edge
    for y in top..=bottom {
        let cell = &buffer[(right, y)];
        assert_eq!(cell.fg, p.amber, "right edge {right},{y}");
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "right edge {right},{y} bold"
        );
    }
}

#[test]
fn coach_header_names_target() {
    // Step 0 (Pulse) -> TARGET PULSE
    let engine = tutorial_engine(0);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let text = draw(80, 24, &ui);
    assert!(text.contains("TARGET PULSE"), "step 0: {text}");

    // Step 4 (Signal Chain) -> TARGET SIGNAL CHAIN
    let engine = tutorial_engine(4);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    let text = draw(80, 24, &ui);
    assert!(text.contains("TARGET SIGNAL CHAIN"), "step 4: {text}");
}

#[test]
fn coach_graphical_step0_80x24() {
    let engine = tutorial_engine(0);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    insta::assert_snapshot!("coach_graphical_step0_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_step0_100x30() {
    let engine = tutorial_engine(0);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    insta::assert_snapshot!("coach_graphical_step0_100x30", draw(100, 30, &ui));
}

#[test]
fn coach_graphical_chain_80x24() {
    // Step 4: Signal Chain
    let engine = tutorial_engine(4);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    insta::assert_snapshot!("coach_graphical_chain_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_peak_80x24() {
    // Step 6: Peak Echo
    let engine = tutorial_engine(6);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, false);
    insta::assert_snapshot!("coach_graphical_peak_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_hull_80x24() {
    // Step 8: Hull on DESCENT
    let engine = tutorial_engine(8);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Descent, false);
    insta::assert_snapshot!("coach_graphical_hull_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_system_80x24() {
    // Step 10: System tab
    let engine = tutorial_engine(10);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::System, false);
    insta::assert_snapshot!("coach_graphical_system_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_ascii_80x24() {
    let engine = tutorial_engine(0);
    let fixture = Fixture::new(engine);
    let ui = coach_ui(&fixture, Screen::Sonar, true);
    insta::assert_snapshot!("coach_graphical_ascii_80x24", draw(80, 24, &ui));
}

#[test]
fn coach_graphical_nudge_80x24() {
    let engine = tutorial_engine(0);
    let fixture = Fixture::new(engine);
    let mut ui = coach_ui(&fixture, Screen::Sonar, false);
    ui.tutorial_nudge = Some("PRESS SPACE");
    insta::assert_snapshot!("coach_graphical_nudge_80x24", draw(80, 24, &ui));
}
