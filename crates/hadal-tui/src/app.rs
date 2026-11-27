//! Session lifecycle, save/load/offline, fixed-step loop, and rendering driver.

use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use hadal_core::content::ids::{GeneratorId, HullId, UpgradeId};
use hadal_core::content::story::log_text;
use hadal_core::content::tables::achievement_def;
use hadal_core::content::tutorial::{STEPS, TutorialStepId};
use hadal_core::engine::TutorialUiEvent;
use hadal_core::save::SavePaths;
use hadal_core::save::config::{Config, ConfigWarning, load_config, save_config};
use hadal_core::save::lock::LockError;
use hadal_core::{
    Action, Decimal, Engine, Event, LoadWarning, OfflineReport, SaveError, SaveLock, SaveStore,
    simulate_offline,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};

use crate::cli::Cli;
use crate::input::{Command, InputQueue, Mode, map_key};
use crate::model;
use crate::render::{
    Anim, EditKind, EditView, Focus, HitMap, Screen, SettingsView, Tab, UiState, amount,
    command_for_click, fmt_duration, render,
};
use crate::sanitize::sanitize;
use crate::theme::{ColorDepth, Palette, Theme};

/// Fixed simulation step.
const STEP: Duration = Duration::from_millis(100);
/// Maximum fixed steps per frame.
const MAX_STEPS: u32 = 20;
/// Poll timeout per frame.
const POLL: Duration = Duration::from_millis(33);
/// Render interval cap (30 FPS).
const FRAME: Duration = Duration::from_millis(33);
/// Autosave interval.
const AUTOSAVE: Duration = Duration::from_secs(30);
/// Gap above which the loop routes through offline catch-up.
const GAP: Duration = Duration::from_secs(2);
/// Boot stage reveal thresholds (ms): first beat, second beat, third beat.
const BOOT_STAGE_MS: [u64; 3] = [800, 1400, 2000];
/// Number of boot reveal stages.
const BOOT_STAGES: usize = BOOT_STAGE_MS.len();
/// Total boot duration before the console settles (ms).
const BOOT_TOTAL_MS: u64 = 2500;
/// Short boot for an existing save (ms).
const BOOT_EXISTING_MS: u64 = 400;
/// Short boot under reduce-motion (ms).
const BOOT_REDUCE_MS: u64 = 800;
/// Echo odometer tween duration.
const TWEEN_MS: u64 = 250;
/// Pulse scope-ring duration.
const PULSE_ANIM_MS: u64 = 700;
/// Generator row flash duration.
const ROW_FLASH_MS: u64 = 250;
/// Carrier-change flash duration.
const CARRIER_FLASH_MS: u64 = 900;
/// Dive transition duration.
const DIVE_DURATION: Duration = Duration::from_millis(900);
/// Toast lifetime.
const TOAST_TTL: Duration = Duration::from_secs(5);
/// Maximum visible toasts.
const TOAST_CAP: usize = 3;
/// Minimum spacing between coalesced purchase toasts.
const PURCHASE_TOAST_GAP: Duration = Duration::from_secs(1);
/// Double-click window for hull rows.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Maximum text-entry buffer length.
const EDIT_MAX_CHARS: usize = 512;

// ---------------------------------------------------------------------
// Startup errors
// ---------------------------------------------------------------------

/// Typed failures that stop startup before the loop begins.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The save lock is held by another process (or another lock I/O failure).
    #[error("{0}")]
    Lock(String),
    /// The data directory cannot be used (wrong path type or creation failed).
    #[error("{0}")]
    DataDir(String),
    /// The save is from a newer schema.
    #[error("{0}")]
    FutureSchema(String),
    /// The save could not be loaded and no backup was usable.
    #[error("could not load the save: {0}")]
    Load(String),
}

// ---------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------

/// Resolved start-up options (config + CLI overrides).
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub data_dir: PathBuf,
    pub demo: bool,
    pub theme: Theme,
    pub ascii: bool,
    pub mouse: bool,
    pub color: ColorDepth,
    pub reduce_motion: bool,
    pub bell: bool,
    pub config: Config,
    pub config_warning: Option<String>,
}

/// Resolve the data directory, load config, and apply CLI overrides.
pub fn resolve_options(cli: &Cli) -> SessionOptions {
    let data_dir = hadal_core::save::resolve_data_dir(cli.data_dir.as_deref());
    let config_path = SavePaths::new(&data_dir).config();
    let (config, warning) = load_config(&config_path);

    let theme = cli
        .theme
        .map(Theme::from)
        .unwrap_or_else(|| Theme::from(config.theme));
    let color = cli
        .color
        .map(ColorDepth::from)
        .unwrap_or_else(|| ColorDepth::from(config.color_depth));
    let ascii = cli.ascii || config.ascii_only;
    let mouse = !cli.no_mouse && config.mouse;
    let bell = config.bell;

    debug_assert!(
        crate::theme::validate_contrast(theme),
        "theme palette must satisfy WCAG contrast"
    );

    let config_warning = warning.map(|w| match w {
        ConfigWarning::Corrupt { path, detail } => format!(
            "config unreadable at {} ({}); using defaults",
            sanitize(&path.display().to_string()),
            sanitize(&detail)
        ),
    });

    SessionOptions {
        data_dir,
        demo: cli.demo,
        theme,
        ascii,
        mouse,
        color,
        reduce_motion: config.reduce_motion,
        bell,
        config,
        config_warning,
    }
}

// ---------------------------------------------------------------------
// Boot, screens, animation
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenKind {
    Sonar,
    Descent,
    Archive,
    System,
}

#[derive(Debug, Clone, Copy)]
enum Boot {
    New { started: Instant },
    Existing { until: Instant },
    Done,
}

#[derive(Debug, Clone, Copy)]
struct DiveState {
    from_m: u32,
    to_m: u32,
    started: Instant,
    reduce_motion: bool,
}

impl DiveState {
    fn new(from_m: u32, to_m: u32, started: Instant, reduce_motion: bool) -> Self {
        Self {
            from_m,
            to_m,
            started,
            reduce_motion,
        }
    }

    fn progress(&self, now: Instant) -> f64 {
        if self.reduce_motion {
            return 1.0;
        }
        (now.saturating_duration_since(self.started).as_millis() as f64 / 900.0).clamp(0.0, 1.0)
    }

    fn finished(&self, now: Instant) -> bool {
        self.reduce_motion || now.saturating_duration_since(self.started) >= DIVE_DURATION
    }

    fn screen(&self, now: Instant) -> Screen {
        Screen::Dive {
            from_m: self.from_m,
            to_m: self.to_m,
            progress: (self.progress(now) * 1000.0).round() as u16,
        }
    }
}

struct EchoTween {
    from: Decimal,
    to: Decimal,
    started: Instant,
}

impl EchoTween {
    fn new(value: Decimal) -> Self {
        Self {
            from: value,
            to: value,
            started: Instant::now(),
        }
    }

    fn observe(&mut self, target: Decimal, now: Instant) {
        if target != self.to {
            let current = self.value(now);
            self.from = current;
            self.to = target;
            self.started = now;
        }
    }

    fn value(&self, now: Instant) -> Decimal {
        if self.from == self.to {
            return self.to;
        }
        let elapsed = now.saturating_duration_since(self.started).as_millis() as u64;
        if elapsed >= TWEEN_MS {
            return self.to;
        }
        let factor = Decimal::from_u64(elapsed)
            .div(Decimal::from_u64(TWEEN_MS))
            .unwrap_or(Decimal::ZERO);
        if self.to >= self.from {
            self.from.add((self.to - self.from).mul(factor))
        } else {
            self.from.sub((self.from - self.to).mul(factor))
        }
    }
}

struct AnimState {
    echo: EchoTween,
    pulse_started: Option<Instant>,
    carrier_flash_until: Option<Instant>,
    row_flash_until: [Option<Instant>; 4],
    started: Instant,
    samples: [Decimal; 64],
    head: usize,
    last_sample: Instant,
}

impl AnimState {
    fn new(echo: Decimal) -> Self {
        Self {
            echo: EchoTween::new(echo),
            pulse_started: None,
            carrier_flash_until: None,
            row_flash_until: [None; 4],
            started: Instant::now(),
            samples: [Decimal::ZERO; 64],
            head: 0,
            last_sample: Instant::now(),
        }
    }

    fn observe_echo(&mut self, target: Decimal, now: Instant) {
        self.echo.observe(target, now);
    }

    /// Sample the current rate into the history buffer (called once per second).
    fn sample_rate(&mut self, rate: Decimal, now: Instant) {
        let elapsed = now.duration_since(self.last_sample);
        if elapsed >= Duration::from_secs(1) {
            self.head = (self.head + 1) % 64;
            self.samples[self.head] = rate;
            self.last_sample = now;
        }
    }

    fn snapshot(&self, now: Instant, reduce_motion: bool) -> Anim {
        Anim {
            echo_display: self.echo.value(now),
            pulse_phase: if reduce_motion {
                None
            } else {
                self.pulse_started.and_then(|started| {
                    let phase = now.saturating_duration_since(started).as_millis() as f64
                        / PULSE_ANIM_MS as f64;
                    (phase < 1.0).then_some(phase)
                })
            },
            carrier_flash: !reduce_motion
                && self.carrier_flash_until.is_some_and(|until| now < until),
            row_flash: std::array::from_fn(|index| {
                !reduce_motion && self.row_flash_until[index].is_some_and(|until| now < until)
            }),
            frame_ms: now.duration_since(self.started).as_millis() as u64,
            rate_history: self.samples,
            rate_head: self.head,
            reduce_motion,
        }
    }
}

struct EditState {
    kind: EditKind,
    buffer: String,
    error: Option<String>,
}

impl EditState {
    fn push(&mut self, c: char) {
        if !c.is_control() && self.buffer.chars().count() < EDIT_MAX_CHARS {
            self.buffer.push(c);
            self.error = None;
        }
    }

    fn backspace(&mut self) {
        self.buffer.pop();
        self.error = None;
    }

    fn paste(&mut self, text: &str) {
        for c in text.chars() {
            if !c.is_control() && self.buffer.chars().count() < EDIT_MAX_CHARS {
                self.buffer.push(c);
            }
        }
        self.error = None;
    }
}

// ---------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------

/// A loaded game session, independent of the terminal.
pub struct Session {
    pub engine: Engine,
    pub ascii: bool,
    pub mouse: bool,
    pub bell: bool,
    pub palette: Palette,
    pub theme: Theme,
    pub color_depth: ColorDepth,
    pub reduce_motion: bool,
    pub config: Config,
    pub data_dir: PathBuf,
    pub save_path: String,
    pub demo: bool,
    pub offline: Option<OfflineReport>,
    pub offline_visible: bool,
    pub warning: Option<String>,
    pub save_error: Option<String>,
    pub config_error: Option<String>,
    pub should_quit: bool,
    started_at: Instant,
    boot: Boot,
    current: ScreenKind,
    focus: Focus,
    confirm_descent: bool,
    notice: Option<String>,
    toasts: Vec<String>,
    toast_until: Vec<Instant>,
    anim: AnimState,
    dive: Option<DiveState>,
    pending_events: Vec<Event>,
    hit_map: HitMap,
    archive_scroll: usize,
    archive_total: usize,
    archive_visible: usize,
    edit: Option<EditState>,
    help: bool,
    help_scroll: usize,
    help_total: usize,
    help_visible: usize,
    hovered_generator: Option<GeneratorId>,
    tutorial_step: u8,
    tutorial_finished: bool,
    confirm_tutorial_skip: bool,
    confirm_tutorial_replay: bool,
    last_save_unix: Option<u64>,
    last_hull_click: Option<(usize, Instant)>,
    bell_once: bool,
    /// Tutorial nudge text to display in the coach status row.
    tutorial_nudge: Option<(String, Instant)>,
    purchase_generators: u32,
    purchase_upgrades: u32,
    purchase_hulls: u32,
    last_purchase_toast: Option<Instant>,
    store: Option<SaveStore>,
    _lock: Option<SaveLock>,
}

impl Session {
    /// Start a session: demo, or lock + load + offline + immediate save.
    ///
    /// # Errors
    ///
    /// Returns [`StartupError`] when the lock is held, the save is from a
    /// future schema, or the save cannot be loaded.
    pub fn start(opts: &SessionOptions, now_unix: u64) -> Result<Self, StartupError> {
        let palette = Palette::resolve(opts.theme, opts.color);
        let save_path = sanitize(&SavePaths::new(&opts.data_dir).save().display().to_string());

        if opts.demo {
            let engine = Engine::demo();
            let anim = AnimState::new(engine.view().echo());
            let tutorial = engine.tutorial();
            let tutorial_step = tutorial.index;
            let tutorial_finished = tutorial.completed || tutorial.skipped;
            return Ok(Self {
                engine,
                ascii: opts.ascii,
                mouse: opts.mouse,
                bell: opts.bell,
                palette,
                theme: opts.theme,
                color_depth: opts.color,
                reduce_motion: opts.reduce_motion,
                config: opts.config,
                data_dir: opts.data_dir.clone(),
                save_path,
                demo: true,
                offline: None,
                offline_visible: false,
                warning: opts.config_warning.clone(),
                save_error: None,
                config_error: None,
                should_quit: false,
                started_at: Instant::now(),
                boot: Boot::Done,
                current: ScreenKind::Sonar,
                focus: Focus::None,
                confirm_descent: false,
                notice: None,
                toasts: Vec::new(),
                toast_until: Vec::new(),
                anim,
                dive: None,
                pending_events: Vec::new(),
                hit_map: HitMap::default(),
                archive_scroll: 0,
                archive_total: 0,
                archive_visible: 0,
                edit: None,
                help: false,
                help_scroll: 0,
                help_total: 0,
                help_visible: 0,
                hovered_generator: None,
                tutorial_step,
                tutorial_finished,
                confirm_tutorial_skip: false,
                confirm_tutorial_replay: false,
                last_save_unix: None,
                last_hull_click: None,
                bell_once: false,
                tutorial_nudge: None,
                purchase_generators: 0,
                purchase_upgrades: 0,
                purchase_hulls: 0,
                last_purchase_toast: None,
                store: None,
                _lock: None,
            });
        }

        let lock = SaveLock::acquire(&opts.data_dir).map_err(startup_lock_error)?;
        // Best-effort remove a stale save temp left by a SIGKILL.  Done under the
        // held lock and before load/save so a half-written temp cannot linger.
        let _ = std::fs::remove_file(SavePaths::new(&opts.data_dir).temp());
        let mut store = SaveStore::new(&opts.data_dir);

        let (engine, saved_at, load_warning) = match store.load(now_unix) {
            Ok(loaded) => {
                let warn = recovered_warning(&loaded.warning);
                (
                    Engine::from_state(loaded.state),
                    Some(loaded.saved_at_unix),
                    warn,
                )
            }
            Err(err) if is_missing(&err) => (Engine::new(), None, None),
            Err(err @ SaveError::FutureSchema { .. }) => {
                return Err(StartupError::FutureSchema(err.to_string()));
            }
            Err(err) => return Err(StartupError::Load(err.to_string())),
        };

        let anim = AnimState::new(engine.view().echo());
        let mut session = Self {
            engine,
            ascii: opts.ascii,
            mouse: opts.mouse,
            bell: opts.bell,
            palette,
            theme: opts.theme,
            color_depth: opts.color,
            reduce_motion: opts.reduce_motion,
            config: opts.config,
            data_dir: opts.data_dir.clone(),
            save_path,
            demo: false,
            offline: None,
            offline_visible: false,
            warning: combine_warnings(opts.config_warning.clone(), load_warning),
            save_error: None,
            config_error: None,
            should_quit: false,
            started_at: Instant::now(),
            boot: Boot::Done,
            current: ScreenKind::Sonar,
            focus: Focus::None,
            confirm_descent: false,
            notice: None,
            toasts: Vec::new(),
            toast_until: Vec::new(),
            anim,
            dive: None,
            pending_events: Vec::new(),
            hit_map: HitMap::default(),
            archive_scroll: 0,
            archive_total: 0,
            archive_visible: 0,
            edit: None,
            help: false,
            help_scroll: 0,
            help_total: 0,
            help_visible: 0,
            hovered_generator: None,
            tutorial_step: 0,
            tutorial_finished: false,
            confirm_tutorial_skip: false,
            confirm_tutorial_replay: false,
            last_save_unix: None,
            last_hull_click: None,
            bell_once: false,
            tutorial_nudge: None,
            purchase_generators: 0,
            purchase_upgrades: 0,
            purchase_hulls: 0,
            last_purchase_toast: None,
            store: Some(store),
            _lock: Some(lock),
        };

        if let Some(saved_at) = saved_at {
            let elapsed = now_unix.saturating_sub(saved_at);
            let report = simulate_offline(&mut session.engine, elapsed);
            session.offline_visible = elapsed >= 60;
            session.offline = Some(report);
            session.boot = Boot::Existing {
                until: Instant::now() + Duration::from_millis(BOOT_EXISTING_MS),
            };
            session.bell_once = session.offline_visible;
        } else {
            session.boot = Boot::New {
                started: Instant::now(),
            };
        }
        // Provision the app-owned exports directory so the default export path
        // always works.  Keep `export_to_path`'s rule of never creating arbitrary
        // user-typed parents — this only covers the app-owned default.
        let _ = std::fs::create_dir_all(SavePaths::new(&session.data_dir).exports());

        let tutorial = session.engine.tutorial();
        session.tutorial_step = tutorial.index;
        session.tutorial_finished = tutorial.completed || tutorial.skipped;
        session.save_now(now_unix);
        session.anim = AnimState::new(session.engine.view().echo());
        Ok(session)
    }

    /// Persistent warning banner text (save failure first).
    pub fn banner(&self) -> Option<&str> {
        self.save_error
            .as_deref()
            .or(self.config_error.as_deref())
            .or(self.warning.as_deref())
    }

    /// The screen to draw at `now`.
    pub fn screen(&self, now: Instant) -> Screen {
        if let Some(dive) = self.dive {
            return dive.screen(now);
        }
        if !self.boot_done(now) {
            return match self.boot {
                Boot::New { started } => Screen::Boot {
                    revealed: self.reveal(now, started),
                },
                Boot::Existing { .. } => Screen::Boot {
                    revealed: BOOT_STAGES,
                },
                Boot::Done => Screen::Sonar,
            };
        }
        match self.current {
            ScreenKind::Sonar => Screen::Sonar,
            ScreenKind::Descent => Screen::Descent,
            ScreenKind::Archive => Screen::Archive,
            ScreenKind::System => Screen::System,
        }
    }

    fn reveal(&self, now: Instant, started: Instant) -> usize {
        if self.reduce_motion {
            return BOOT_STAGES;
        }
        let ms = now.saturating_duration_since(started).as_millis() as u64;
        BOOT_STAGE_MS.iter().filter(|&&at| ms >= at).count()
    }

    /// Whether the boot sequence has completed.
    pub fn boot_done(&self, now: Instant) -> bool {
        match self.boot {
            Boot::Done => true,
            Boot::Existing { until } => now >= until,
            Boot::New { started } => {
                let total = if self.reduce_motion {
                    Duration::from_millis(BOOT_REDUCE_MS)
                } else {
                    Duration::from_millis(BOOT_TOTAL_MS)
                };
                now.saturating_duration_since(started) >= total
            }
        }
    }

    /// Advance the boot state if it has finished.
    pub fn advance_boot(&mut self, now: Instant) {
        if !matches!(self.boot, Boot::Done) && self.boot_done(now) {
            self.boot = Boot::Done;
        }
    }

    /// Skip the boot sequence.
    pub fn skip_boot(&mut self) {
        self.boot = Boot::Done;
    }

    fn settings_view(&self) -> SettingsView {
        SettingsView {
            theme: self.theme,
            reduce_motion: self.reduce_motion,
            ascii: self.ascii,
            mouse: self.mouse,
            bell: self.bell,
            color_depth: self.color_depth,
        }
    }

    fn ui_anim(&self, now: Instant) -> Anim {
        self.anim.snapshot(now, self.reduce_motion)
    }

    /// Apply one command. Returns whether a redraw is needed.
    pub fn apply_command(&mut self, command: Command, now_unix: u64) -> bool {
        let now = Instant::now();
        match command {
            Command::Pulse => {
                if self.current == ScreenKind::System {
                    self.activate_system_focus(now_unix);
                } else if self.engine.apply(Action::Pulse).is_ok() {
                    self.anim.pulse_started = Some(now);
                }
            }
            Command::TuneNext => {
                let next = (self.engine.view().tuned_band() + 1) % 6;
                let _ = self.engine.apply(Action::SetTuning { band: next });
            }
            Command::Buy(id, quantity) => {
                if self
                    .engine
                    .apply(Action::BuyGenerator { id, quantity })
                    .is_ok()
                {
                    self.anim.row_flash_until[id.index()] =
                        Some(now + Duration::from_millis(ROW_FLASH_MS));
                }
            }
            Command::Save => {
                self.save_manual(now_unix);
            }
            Command::Quit => {
                self.should_quit = true;
                self.save_now(now_unix);
            }
            Command::OpenSonar => self.switch_tab(Tab::Sonar),
            Command::OpenDescent => self.switch_tab(Tab::Descent),
            Command::OpenArchive => self.switch_tab(Tab::Archive),
            Command::OpenSystem => self.switch_tab(Tab::System),
            Command::NextTab => self.cycle_tab(1),
            Command::PrevTab => self.cycle_tab(-1),
            Command::DescentConfirm => {
                if self.current == ScreenKind::Descent {
                    self.request_descend();
                } else {
                    self.switch_tab(Tab::Descent);
                }
            }
            Command::ConfirmDescent => self.try_descend(now_unix, now),
            Command::Cancel => {
                if self.confirm_descent {
                    self.confirm_descent = false;
                } else if self.help {
                    self.help = false;
                    self.record_tutorial_ui(TutorialUiEvent::ManualClosed);
                } else if self.edit.is_some() {
                    self.edit = None;
                } else if matches!(self.current, ScreenKind::Archive | ScreenKind::System) {
                    self.current = ScreenKind::Sonar;
                } else if self.focus != Focus::None {
                    self.focus = Focus::None;
                } else if self.current == ScreenKind::Descent {
                    self.current = ScreenKind::Sonar;
                }
                self.notice = None;
            }
            Command::FocusUpgrades => {
                if self.current == ScreenKind::Sonar {
                    self.focus = Focus::Upgrade(self.first_chain_index());
                    self.notice = None;
                }
            }
            Command::FocusPrev => self.previous(),
            Command::FocusNext => self.next(),
            Command::FocusActivate => self.activate_focus(now_unix),
            Command::GovernorAdjust(delta) => {
                if self.current == ScreenKind::Descent {
                    self.adjust_governor(delta);
                }
            }
            Command::TutorialSkip => {
                if self.tutorial_active()
                    && !self.help
                    && self.edit.is_none()
                    && !self.offline_visible
                    && !self.confirm_descent
                    && !self.confirm_tutorial_replay
                {
                    self.confirm_tutorial_skip = true;
                }
            }
            Command::Help => {
                if self.help {
                    self.help = false;
                    self.record_tutorial_ui(TutorialUiEvent::ManualClosed);
                } else {
                    self.help = true;
                    self.help_scroll = 0;
                    self.record_tutorial_ui(TutorialUiEvent::ManualOpened);
                }
            }
            Command::PageUp => self.page(-1),
            Command::PageDown => self.page(1),
            Command::StartExport => {
                if self.current == ScreenKind::System {
                    self.open_edit(EditKind::Export, now_unix);
                }
            }
            Command::StartImport => {
                if self.current == ScreenKind::System {
                    self.open_edit(EditKind::Import, now_unix);
                }
            }
            Command::StartReset => {
                if self.current == ScreenKind::System {
                    self.open_edit(EditKind::Reset, now_unix);
                }
            }
            Command::ToggleFocused => {
                if self.current == ScreenKind::System {
                    self.activate_system_focus(now_unix);
                } else if matches!(self.focus, Focus::Upgrade(_) | Focus::Hull(_)) {
                    self.activate_focus(now_unix);
                }
            }
            Command::FocusUpgradeAt(index) => {
                self.current = ScreenKind::Sonar;
                self.focus = Focus::Upgrade(index);
                self.activate_focus(now_unix);
            }
            Command::FocusHullAt(index) => {
                let double = self.register_hull_click(index, now);
                self.current = ScreenKind::Descent;
                self.focus = Focus::Hull(index);
                if double {
                    self.activate_focus(now_unix);
                }
            }
            Command::SystemFocusAt(index) => {
                self.current = ScreenKind::System;
                self.focus = Focus::System(index);
                self.activate_system_focus(now_unix);
            }
        }
        true
    }

    fn tutorial_active(&self) -> bool {
        let tutorial = self.engine.tutorial();
        !tutorial.completed && !tutorial.skipped
    }

    fn tutorial_offline_step(&self) -> bool {
        let tutorial = self.engine.tutorial();
        !tutorial.completed && !tutorial.skipped && tutorial.step.id == TutorialStepId::Offline
    }

    /// True when the tutorial is active and forbids `cmd`.
    fn tutorial_blocks(&self, cmd: Command) -> bool {
        if !self.tutorial_active() {
            return false;
        }
        let tutorial = self.engine.tutorial();
        !crate::input::tutorial_allows(tutorial.step.id, cmd)
    }

    /// Set the coach nudge for a wrong input on the current step.
    fn nudge_wrong(&mut self, now: Instant) {
        if !self.tutorial_active() {
            return;
        }
        let key = match self.engine.tutorial().step.id {
            TutorialStepId::Pulse => "SPACE",
            TutorialStepId::FirstGenerator | TutorialStepId::SecondGenerator => "h",
            TutorialStepId::Drifter => {
                if self.engine.view().generator_owned(GeneratorId::Hydrophone) >= 10 {
                    "d"
                } else {
                    "h"
                }
            }
            TutorialStepId::Upgrade => {
                if matches!(self.focus, Focus::Upgrade(_)) {
                    "Enter"
                } else {
                    "u"
                }
            }
            TutorialStepId::Tune => "f",
            TutorialStepId::Peak => {
                if self.engine.view().can_descend() {
                    "2"
                } else {
                    "h"
                }
            }
            TutorialStepId::Descend => "2",
            TutorialStepId::Hull => "Enter",
            TutorialStepId::SecondDescent => "2",
            TutorialStepId::System => "4",
            TutorialStepId::Manual => "?",
            TutorialStepId::Offline => "Enter",
        };
        self.tutorial_nudge = Some((format!("PRESS {key}"), now + Duration::from_millis(500)));
    }

    fn record_tutorial_ui(&mut self, event: TutorialUiEvent) {
        if self.tutorial_active() {
            self.engine.tutorial_ui(event);
        }
    }

    fn sync_tutorial(&mut self, now: Instant, now_unix: u64) {
        let tutorial = self.engine.tutorial();
        if tutorial.completed || tutorial.skipped {
            if !self.tutorial_finished {
                self.tutorial_finished = true;
                if tutorial.completed && !tutorial.skipped {
                    self.push_toast(
                        model::ascii_text(
                            "TUTORIAL COMPLETE — free play. ? manual · 3 archive · 4 settings",
                            self.ascii,
                        ),
                        now,
                    );
                }
                self.save_now(now_unix);
            }
            return;
        }
        self.tutorial_finished = false;
        if tutorial.index != self.tutorial_step {
            self.tutorial_step = tutorial.index;
            if tutorial.index > 0 {
                let id = STEPS[tutorial.index as usize - 1].id;
                let message = match id {
                    TutorialStepId::Pulse => "pulse received",
                    TutorialStepId::FirstGenerator | TutorialStepId::SecondGenerator => {
                        "hydrophone online"
                    }
                    TutorialStepId::Drifter => "drifter online",
                    TutorialStepId::Upgrade => "impedance ×2",
                    TutorialStepId::Tune => "carrier matched ×1.5",
                    TutorialStepId::Peak => "threshold reached",
                    TutorialStepId::Descend => "Fathoms are permanent",
                    TutorialStepId::Hull => "Auto-Pulse online",
                    TutorialStepId::SecondDescent => "second descent",
                    TutorialStepId::System => "console map learned",
                    TutorialStepId::Manual => "manual read",
                    TutorialStepId::Offline => "",
                };
                if !message.is_empty() {
                    self.push_toast(model::ascii_text(message, self.ascii), now);
                }
            }
            self.save_now(now_unix);
        }
    }

    pub fn tutorial_acknowledge(&mut self, now_unix: u64) {
        if self.tutorial_offline_step() {
            self.engine.tutorial_acknowledge();
            self.save_now(now_unix);
        }
    }

    fn skip_tutorial(&mut self, now_unix: u64) {
        self.confirm_tutorial_skip = false;
        if self.tutorial_active() {
            self.engine.tutorial_skip();
            self.tutorial_finished = true;
            self.save_now(now_unix);
        }
    }

    fn replay_tutorial(&mut self, now_unix: u64) {
        // Reset the tutorial state so the coach reappears; observe_tutorial then
        // advances to the first unsatisfied step, which the confirmation copy
        // now describes honestly as "resume from your next unfinished step".
        self.engine.tutorial_replay();
        self.tutorial_step = 0;
        self.tutorial_finished = false;
        self.confirm_tutorial_replay = false;
        self.confirm_tutorial_skip = false;
        self.current = ScreenKind::Sonar;
        self.focus = Focus::None;
        self.save_now(now_unix);
    }

    fn tutorial_system_action(&mut self, now_unix: u64) {
        if self.demo {
            self.notice = Some("Demo: tutorial is disabled.".to_owned());
            return;
        }
        if self.tutorial_active() {
            self.replay_tutorial(now_unix);
        } else if self.engine.view().descents() == 0 {
            self.confirm_tutorial_replay = true;
        } else {
            self.notice = Some("Start a new save to replay the tutorial.".to_owned());
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        let entering_system = tab == Tab::System && self.current != ScreenKind::System;
        match tab {
            Tab::Sonar => {
                self.current = ScreenKind::Sonar;
                self.focus = Focus::None;
            }
            Tab::Descent => {
                self.current = ScreenKind::Descent;
                self.focus = Focus::Hull(self.first_affordable_hull());
            }
            Tab::Archive => {
                self.current = ScreenKind::Archive;
                self.focus = Focus::None;
                self.archive_scroll = 0;
            }
            Tab::System => {
                self.current = ScreenKind::System;
                self.focus = Focus::System(0);
            }
        }
        self.confirm_descent = false;
        self.notice = None;
        self.hovered_generator = None;
        if entering_system && self.tutorial_active() {
            self.record_tutorial_ui(TutorialUiEvent::SystemOpened);
        }
    }

    fn cycle_tab(&mut self, direction: isize) {
        let order = [
            ScreenKind::Sonar,
            ScreenKind::Descent,
            ScreenKind::Archive,
            ScreenKind::System,
        ];
        let index = order
            .iter()
            .position(|screen| *screen == self.current)
            .unwrap_or(0) as isize;
        let next = (index + direction).rem_euclid(order.len() as isize) as usize;
        let tab = match order[next] {
            ScreenKind::Sonar => Tab::Sonar,
            ScreenKind::Descent => Tab::Descent,
            ScreenKind::Archive => Tab::Archive,
            ScreenKind::System => Tab::System,
        };
        self.switch_tab(tab);
    }

    fn first_chain_index(&self) -> usize {
        let view = self.engine.view();
        UpgradeId::ALL
            .iter()
            .position(|id| !view.upgrade_owned(*id) && view.upgrade_unlocked(*id))
            .unwrap_or(0)
    }

    fn first_affordable_hull(&self) -> usize {
        let view = self.engine.view();
        let flat = model::flattened_hulls();
        if let Some(index) = flat.iter().position(|id| view.hull_affordable(*id)) {
            return index;
        }
        flat.iter()
            .position(|id| !view.hull_owned(*id))
            .unwrap_or(0)
    }

    fn previous(&mut self) {
        if self.help {
            self.scroll_help(-1);
            return;
        }
        match self.current {
            ScreenKind::Archive => self.scroll_archive(-1),
            ScreenKind::System => self.move_system_focus(-1),
            _ => self.move_focus(-1),
        }
    }

    fn next(&mut self) {
        if self.help {
            self.scroll_help(1);
            return;
        }
        match self.current {
            ScreenKind::Archive => self.scroll_archive(1),
            ScreenKind::System => self.move_system_focus(1),
            _ => self.move_focus(1),
        }
    }

    fn page(&mut self, direction: isize) {
        if self.help {
            let page = self.help_visible.saturating_sub(1).max(1) as isize;
            self.scroll_help(direction * page);
            return;
        }
        match self.current {
            ScreenKind::Archive => {
                let page = self.archive_visible.saturating_sub(1).max(1) as isize;
                self.scroll_archive(direction * page);
            }
            ScreenKind::System => self.move_system_focus(direction * 5),
            _ => {}
        }
    }

    fn scroll_help(&mut self, delta: isize) {
        let max = self.help_total.saturating_sub(self.help_visible);
        self.help_scroll = (self.help_scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    fn scroll_archive(&mut self, delta: isize) {
        self.archive_scroll = crate::render::archive::clamp_scroll(
            self.archive_scroll,
            delta,
            self.archive_total,
            self.archive_visible,
        );
    }

    fn move_system_focus(&mut self, delta: isize) {
        let index = match self.focus {
            Focus::System(index) => index,
            _ => 0,
        };
        let next = (index as isize + delta).clamp(0, 10) as usize;
        self.focus = Focus::System(next);
    }

    fn move_focus(&mut self, delta: isize) {
        match self.focus {
            Focus::Upgrade(index) if self.current == ScreenKind::Sonar => {
                let count = UpgradeId::ALL.len() as isize;
                self.focus = Focus::Upgrade((index as isize + delta).clamp(0, count - 1) as usize);
            }
            Focus::Hull(index) if self.current == ScreenKind::Descent => {
                let count = model::flattened_hulls().len() as isize;
                self.focus = Focus::Hull((index as isize + delta).clamp(0, count - 1) as usize);
            }
            _ => {}
        }
    }

    fn register_hull_click(&mut self, index: usize, now: Instant) -> bool {
        let double = self.last_hull_click.is_some_and(|(last_index, at)| {
            last_index == index && now.saturating_duration_since(at) <= DOUBLE_CLICK
        });
        self.last_hull_click = Some((index, now));
        double
    }

    fn activate_focus(&mut self, now_unix: u64) {
        self.notice = None;
        match self.focus {
            Focus::Upgrade(index) if self.current == ScreenKind::Sonar => {
                let Some(id) = UpgradeId::ALL.get(index).copied() else {
                    return;
                };
                if let Err(err) = self.engine.apply(Action::BuyUpgrade(id)) {
                    self.notice = Some(format!("{}: {err}", model::upgrade_name(id)));
                }
            }
            Focus::Hull(index) if self.current == ScreenKind::Descent => {
                let flat = model::flattened_hulls();
                let Some(id) = flat.get(index).copied() else {
                    return;
                };
                if let Err(err) = self.engine.apply(Action::BuyHull(id)) {
                    let unmet = if self.engine.view().hull_prereq_met(id) {
                        None
                    } else {
                        model::hull_requirement(id)
                    };
                    self.notice = Some(match unmet {
                        Some(requirement) => {
                            format!("{}: needs {requirement}", model::hull_name(id))
                        }
                        None => format!("{}: {err}", model::hull_name(id)),
                    });
                }
            }
            Focus::System(_) if self.current == ScreenKind::System => {
                self.activate_system_focus(now_unix);
            }
            _ => {}
        }
    }

    fn activate_system_focus(&mut self, now_unix: u64) {
        let index = match self.focus {
            Focus::System(index) => index,
            _ => return,
        };
        match index {
            0 => {
                self.theme = self.theme.next();
                self.apply_setting();
            }
            1 => {
                self.reduce_motion = !self.reduce_motion;
                self.apply_setting();
            }
            2 => {
                self.ascii = !self.ascii;
                self.apply_setting();
            }
            3 => {
                self.mouse = !self.mouse;
                if let Err(err) = crate::terminal::set_mouse(self.mouse) {
                    self.notice = Some(format!("MOUSE: {}", sanitize(&err.to_string())));
                }
                self.apply_setting();
            }
            4 => {
                self.color_depth = self.color_depth.next();
                self.config.color_depth = self.color_depth.into();
                self.apply_setting();
            }
            5 => {
                self.bell = !self.bell;
                self.apply_setting();
            }
            10 => self.tutorial_system_action(now_unix),
            6..=9 => {
                let id = GeneratorId::ALL[index - 6];
                let enabled = !self.engine.view().auto_buy(id);
                if let Err(err) = self.engine.apply(Action::SetAutoBuy { id, enabled }) {
                    self.notice = Some(format!("AUTO-BUY {}: {err}", model::generator_name(id)));
                } else {
                    self.save_now(now_unix);
                }
            }
            _ => {}
        }
    }

    fn apply_setting(&mut self) {
        self.palette = Palette::resolve(self.theme, self.color_depth);
        self.persist_config();
    }

    fn persist_config(&mut self) {
        if self.demo {
            return;
        }
        self.config.theme = self.theme.into();
        self.config.reduce_motion = self.reduce_motion;
        self.config.ascii_only = self.ascii;
        self.config.mouse = self.mouse;
        self.config.bell = self.bell;

        let path = SavePaths::new(&self.data_dir).config();
        match save_config(&path, &self.config) {
            Ok(()) => self.config_error = None,
            Err(err) => {
                self.config_error = Some(format!(
                    "CONFIG SAVE FAILED: {} — change kept in memory",
                    sanitize(&err.to_string())
                ));
            }
        }
    }

    fn adjust_governor(&mut self, delta: i64) {
        self.notice = None;
        let owned = self.engine.view().hull_owned(HullId::BallastGovernor);
        let current = self.engine.view().auto_descend_at();
        if !owned {
            self.notice =
                Some("BALLAST GOVERNOR: buy the hull before setting a target.".to_owned());
            return;
        }
        match model::propose_governor_target(current, delta) {
            Ok(target) => {
                if let Err(err) = self
                    .engine
                    .apply(Action::SetAutoDescendTarget { fathoms: target })
                {
                    self.notice = Some(format!("BALLAST GOVERNOR: {err}"));
                }
            }
            Err(message) => self.notice = Some(format!("BALLAST GOVERNOR: {message}")),
        }
    }

    fn request_descend(&mut self) {
        if self.engine.view().can_descend() {
            self.confirm_descent = true;
            self.notice = None;
        } else {
            self.notice = Some("DESCENT: peak Echo must reach 1.00M.".to_owned());
        }
    }

    fn try_descend(&mut self, now_unix: u64, now: Instant) {
        if !self.confirm_descent {
            return;
        }
        self.confirm_descent = false;
        if self.dive.is_some() {
            return;
        }
        if !self.engine.view().can_descend() {
            self.notice = Some("DESCENT: peak Echo must reach 1.00M.".to_owned());
            return;
        }
        let from_m = self.engine.view().depth_m();
        match self.engine.apply(Action::Descend) {
            Ok(()) => {
                let to_m = self.engine.view().depth_m();
                self.pending_events.clear();
                self.pending_events.extend(self.engine.drain_events());
                self.save_now(now_unix);
                self.current = ScreenKind::Sonar;
                self.focus = Focus::None;
                self.notice = None;
                if self.reduce_motion {
                    self.finish_dive(now);
                } else {
                    self.dive = Some(DiveState::new(from_m, to_m, now, false));
                }
            }
            Err(err) => self.notice = Some(format!("DESCENT: {err}")),
        }
    }

    /// Skip the dive transition, draining its buffered events.
    pub fn skip_dive(&mut self, now: Instant) {
        if self.dive.is_some() {
            self.finish_dive(now);
        }
    }

    fn finish_dive(&mut self, now: Instant) {
        self.dive = None;
        let events = std::mem::take(&mut self.pending_events);
        self.process_events(events, now);
    }

    fn process_events(&mut self, events: Vec<Event>, now: Instant) {
        for event in events {
            match event {
                Event::CarrierChanged { band } => {
                    self.anim.carrier_flash_until =
                        Some(now + Duration::from_millis(CARRIER_FLASH_MS));
                    self.push_toast(format!("CARRIER {:02}", band + 1), now);
                }
                Event::AchievementUnlocked(id) => {
                    self.push_toast(format!("ACHIEVEMENT: {}", achievement_def(id).name), now);
                    self.bell_once = true;
                }
                Event::LogRecorded(id) => {
                    let text = sanitize(log_text(id));
                    self.push_toast(truncate(&text, 72), now);
                }
                Event::Descended { gain } => {
                    self.push_toast(format!("DESCENT  +{gain} FATHOMS"), now);
                    self.bell_once = true;
                }
                Event::DescentAvailable => {
                    self.push_toast("DESCENT AVAILABLE  press 2".to_owned(), now);
                }
                Event::PurchasedGenerator { .. } => {
                    self.purchase_generators = self.purchase_generators.saturating_add(1);
                }
                Event::PurchasedUpgrade { .. } => {
                    self.purchase_upgrades = self.purchase_upgrades.saturating_add(1);
                }
                Event::PurchasedHull { .. } => {
                    self.purchase_hulls = self.purchase_hulls.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    fn flush_purchases(&mut self, now: Instant) {
        if self.purchase_generators == 0 && self.purchase_upgrades == 0 && self.purchase_hulls == 0
        {
            return;
        }
        if self
            .last_purchase_toast
            .is_some_and(|last| now.saturating_duration_since(last) < PURCHASE_TOAST_GAP)
        {
            return;
        }
        let mut parts = Vec::new();
        if self.purchase_generators > 0 {
            parts.push(format!(
                "{} generator{}",
                self.purchase_generators,
                if self.purchase_generators == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        if self.purchase_upgrades > 0 {
            parts.push(format!(
                "{} upgrade{}",
                self.purchase_upgrades,
                if self.purchase_upgrades == 1 { "" } else { "s" }
            ));
        }
        if self.purchase_hulls > 0 {
            parts.push(format!(
                "{} hull{}",
                self.purchase_hulls,
                if self.purchase_hulls == 1 { "" } else { "s" }
            ));
        }
        self.push_toast(format!("BOUGHT {}", parts.join(" · ")), now);
        self.last_purchase_toast = Some(now);
        self.purchase_generators = 0;
        self.purchase_upgrades = 0;
        self.purchase_hulls = 0;
    }

    fn push_toast(&mut self, text: String, now: Instant) {
        self.toasts.push(text);
        self.toast_until.push(now + TOAST_TTL);
        while self.toasts.len() > TOAST_CAP {
            self.toasts.remove(0);
            self.toast_until.remove(0);
        }
    }

    fn expire_toasts(&mut self, now: Instant) {
        let mut index = 0;
        while index < self.toast_until.len() {
            if now >= self.toast_until[index] {
                self.toast_until.remove(index);
                self.toasts.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// Manual save with a toast.
    pub fn save_manual(&mut self, now_unix: u64) -> bool {
        let now = Instant::now();
        if self.demo {
            self.push_toast("DEMO MODE - NOT SAVED".to_owned(), now);
            return false;
        }
        let ok = self.save_now(now_unix);
        self.push_toast(if ok { "SAVED" } else { "SAVE FAILED" }.to_owned(), now);
        ok
    }

    /// Save immediately. Returns whether the save succeeded.
    ///
    /// Demo sessions never persist and always report success.
    pub fn save_now(&mut self, now_unix: u64) -> bool {
        if self.demo {
            return true;
        }
        let Some(store) = self.store.as_mut() else {
            return true;
        };
        match store.save(self.engine.state(), now_unix) {
            Ok(_) => {
                self.last_save_unix = Some(now_unix);
                self.save_error = None;
                true
            }
            Err(err) => {
                self.save_error = Some(format!(
                    "SAVE FAILED: {} — retrying on next autosave",
                    sanitize(&err.to_string())
                ));
                false
            }
        }
    }

    fn open_edit(&mut self, kind: EditKind, now_unix: u64) {
        let buffer = match kind {
            EditKind::Export => {
                let path = SavePaths::new(&self.data_dir)
                    .exports()
                    .join(format!("save-{now_unix}.json"));
                sanitize(&path.to_string_lossy())
            }
            EditKind::Import | EditKind::Reset => String::new(),
        };
        self.edit = Some(EditState {
            kind,
            buffer,
            error: None,
        });
    }

    /// Feed a key to the open text-entry overlay.
    pub fn edit_key(&mut self, key: KeyEvent, now_unix: u64) {
        if self.edit.is_none() {
            return;
        }
        debug_assert!(
            map_key(key, Mode::TextEntry).is_none(),
            "text entry must not emit game commands"
        );
        match key.code {
            KeyCode::Esc => self.edit = None,
            KeyCode::Enter => self.edit_submit(now_unix),
            KeyCode::Backspace => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.push(c);
                }
            }
            _ => {}
        }
    }

    /// Paste into the open text-entry overlay.
    pub fn edit_paste(&mut self, text: &str) {
        if let Some(edit) = self.edit.as_mut() {
            edit.paste(text);
        }
    }

    /// Submit the text-entry overlay.
    pub fn edit_submit(&mut self, now_unix: u64) {
        let Some((kind, input)) = self
            .edit
            .as_ref()
            .map(|edit| (edit.kind, edit.buffer.trim().to_owned()))
        else {
            return;
        };
        let now = Instant::now();
        let result: Result<Option<String>, String> = match kind {
            EditKind::Export => {
                if input.is_empty() {
                    Err("Enter an export path.".to_owned())
                } else {
                    self.export_save(&input, now_unix)
                        .map(|()| Some(format!("EXPORTED {}", sanitize(&input))))
                }
            }
            EditKind::Import => {
                if input.is_empty() {
                    Err("Enter an import path.".to_owned())
                } else {
                    self.import_save(&input, now_unix)
                        .map(|()| Some("IMPORTED SAVE".to_owned()))
                }
            }
            EditKind::Reset => {
                if input != "DISCARD" {
                    Err("Type DISCARD exactly (case-sensitive).".to_owned())
                } else {
                    self.reset_game(now_unix, now)
                        .map(|()| Some("STATION RESET".to_owned()))
                }
            }
        };
        match result {
            Ok(toast) => {
                self.edit = None;
                if let Some(text) = toast {
                    self.push_toast(text, now);
                }
            }
            Err(message) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.error = Some(sanitize(&message));
                }
            }
        }
    }

    fn export_save(&mut self, path: &str, now_unix: u64) -> Result<(), String> {
        let Some(store) = self.store.as_mut() else {
            return Err("demo mode has no save store".to_owned());
        };
        store
            .export_to_path(self.engine.state(), Path::new(path), now_unix)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    fn import_save(&mut self, path: &str, now_unix: u64) -> Result<(), String> {
        let Some(mut store) = self.store.take() else {
            return Err("demo mode has no save store".to_owned());
        };
        let result = store.import_from_path(Path::new(path), now_unix);
        self.store = Some(store);
        match result {
            Ok(state) => {
                self.engine = Engine::from_state(state);
                self.anim = AnimState::new(self.engine.view().echo());
                self.current = ScreenKind::Sonar;
                self.focus = Focus::None;
                self.confirm_descent = false;
                self.dive = None;
                self.pending_events.clear();
                self.offline = None;
                self.offline_visible = false;
                self.archive_scroll = 0;
                self.notice = None;
                self.hovered_generator = None;
                self.last_save_unix = Some(now_unix);
                self.save_error = None;
                Ok(())
            }
            Err(err) => Err(err.to_string()),
        }
    }

    fn reset_game(&mut self, now_unix: u64, now: Instant) -> Result<(), String> {
        let paths = SavePaths::new(&self.data_dir);
        let primary = paths.save();
        let backup = paths.backup();
        let primary_tmp = primary.with_extension("reset-tmp");
        let backup_tmp = backup.with_extension("reset-tmp");

        let mut moved_primary = false;
        if primary.exists() {
            std::fs::rename(&primary, &primary_tmp)
                .map_err(|err| format!("cannot move save aside: {err}"))?;
            moved_primary = true;
        }
        if backup.exists()
            && let Err(err) = std::fs::rename(&backup, &backup_tmp)
        {
            if moved_primary {
                let _ = std::fs::rename(&primary_tmp, &primary);
            }
            return Err(format!("cannot move backup aside: {err}"));
        }
        let _ = std::fs::remove_file(&primary_tmp);
        let _ = std::fs::remove_file(&backup_tmp);

        self.engine = Engine::new();
        self.current = ScreenKind::Sonar;
        self.focus = Focus::None;
        self.confirm_descent = false;
        self.dive = None;
        self.pending_events.clear();
        self.offline = None;
        self.offline_visible = false;
        self.archive_scroll = 0;
        self.notice = None;
        self.hovered_generator = None;
        self.help = false;
        self.anim = AnimState::new(self.engine.view().echo());
        self.last_save_unix = None;
        self.boot = Boot::New { started: now };
        self.save_now(now_unix);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn truncate(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
}

fn startup_lock_error(err: LockError) -> StartupError {
    match &err {
        LockError::AlreadyRunning => StartupError::Lock(err.to_string()),
        LockError::NotADirectory { path } => StartupError::DataDir(format!(
            "cannot use data directory {}: path is not a directory",
            sanitize(&path.display().to_string())
        )),
        LockError::CreateDir { path, source } => StartupError::DataDir(format!(
            "cannot use data directory {}: {}",
            sanitize(&path.display().to_string()),
            sanitize(&source.to_string())
        )),
        LockError::Io(_) => StartupError::Lock(err.to_string()),
    }
}

fn is_missing(err: &SaveError) -> bool {
    match err {
        SaveError::NoSave { .. } => true,
        SaveError::Io { source, .. } => source.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

fn recovered_warning(warning: &Option<LoadWarning>) -> Option<String> {
    warning.as_ref().map(|warning| {
        let LoadWarning::RecoveredFromBackup { corrupt_path } = warning;
        match corrupt_path {
            Some(path) => format!(
                "primary save was corrupt; recovered from backup (preserved as {})",
                sanitize(&path.display().to_string())
            ),
            None => "recovered from backup save".to_owned(),
        }
    })
}

fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Current Unix time in seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Emit a terminal bell (best effort; ignore write errors).
fn ring() {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

// ---------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------

/// A stateful driver around a [`Session`].
///
/// The production event loop and the end-to-end test both drive this type, so
/// the tested path is the production path.
pub struct App {
    /// The game session.
    pub session: Session,
    queue: InputQueue,
    last_time: Instant,
    accumulator: Duration,
    last_draw: Instant,
    last_autosave: Instant,
    dirty: bool,
}

impl App {
    /// Create an app starting at a caller-supplied instant.
    pub fn new_at(session: Session, now: Instant) -> Self {
        Self {
            session,
            queue: InputQueue::new(),
            last_time: now,
            accumulator: Duration::ZERO,
            last_draw: now.checked_sub(FRAME).unwrap_or(now),
            last_autosave: now,
            dirty: true,
        }
    }

    /// Create an app starting now.
    pub fn new(session: Session) -> Self {
        Self::new_at(session, Instant::now())
    }

    /// Whether the app has been asked to quit.
    pub fn should_quit(&self) -> bool {
        self.session.should_quit
    }

    /// Whether a redraw is due (dirty and at least one frame elapsed).
    pub fn should_redraw(&self, now: Instant) -> bool {
        self.dirty && now.saturating_duration_since(self.last_draw) >= FRAME
    }

    /// Route one terminal event into the session and command queue.
    pub fn handle_event(&mut self, event: CrosstermEvent, now: Instant) {
        match event {
            CrosstermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));
                if ctrl_c {
                    self.session.should_quit = true;
                    self.dirty = true;
                } else if self.session.dive.is_some() {
                    self.session.skip_dive(now);
                    self.dirty = true;
                } else if !self.session.boot_done(now) {
                    self.session.skip_boot();
                    self.dirty = true;
                } else if self.session.confirm_tutorial_skip {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.session.skip_tutorial(now_unix())
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.session.confirm_tutorial_skip = false;
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            self.session.confirm_tutorial_skip = false;
                            self.session.should_quit = true;
                            self.session.save_now(now_unix());
                        }
                        _ => {}
                    }
                    self.dirty = true;
                } else if self.session.confirm_tutorial_replay {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.session.replay_tutorial(now_unix())
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.session.confirm_tutorial_replay = false;
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            self.session.confirm_tutorial_replay = false;
                            self.session.should_quit = true;
                            self.session.save_now(now_unix());
                        }
                        _ => {}
                    }
                    self.dirty = true;
                } else if self.session.help {
                    let page = self.session.help_visible.saturating_sub(1).max(1) as isize;
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            self.session.help = false;
                            self.session.should_quit = true;
                            self.session.save_now(now_unix());
                        }
                        KeyCode::Esc => {
                            self.session.help = false;
                            self.session
                                .record_tutorial_ui(TutorialUiEvent::ManualClosed);
                        }
                        KeyCode::Up => self.session.scroll_help(-1),
                        KeyCode::Down => self.session.scroll_help(1),
                        KeyCode::PageUp => self.session.scroll_help(-page),
                        KeyCode::PageDown => self.session.scroll_help(page),
                        _ => {}
                    }
                    self.dirty = true;
                } else if self.session.edit.is_some() {
                    self.session.edit_key(key, now_unix());
                    self.dirty = true;
                } else if self.session.offline_visible {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            self.session.offline_visible = false;
                            self.session.should_quit = true;
                            self.session.save_now(now_unix());
                        }
                        _ => {
                            self.session.offline_visible = false;
                        }
                    }
                    self.dirty = true;
                } else if self.session.confirm_descent {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.session.try_descend(now_unix(), now)
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.session.confirm_descent = false;
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            self.session.confirm_descent = false;
                            self.session.should_quit = true;
                            self.session.save_now(now_unix());
                        }
                        _ => {}
                    }
                    self.dirty = true;
                } else if key.code == KeyCode::Enter && self.session.tutorial_offline_step() {
                    self.session.tutorial_acknowledge(now_unix());
                    self.dirty = true;
                } else if let Some(command) = map_key(key, Mode::Normal) {
                    if self.session.tutorial_blocks(command) {
                        self.session.nudge_wrong(now);
                        self.dirty = true;
                    } else {
                        let logical_ms = self.session.started_at.elapsed().as_millis() as u64;
                        if self.queue.push(command, logical_ms) {
                            self.dirty = true;
                        }
                    }
                }
            }
            CrosstermEvent::Mouse(mouse) => {
                if mouse.kind == MouseEventKind::Moved {
                    if self.session.mouse
                        && self.session.boot_done(now)
                        && self.session.dive.is_none()
                        && !self.session.help
                        && self.session.edit.is_none()
                        && !self.session.confirm_descent
                        && !self.session.offline_visible
                    {
                        let hovered = self.session.hit_map.generator_at(mouse.column, mouse.row);
                        if hovered != self.session.hovered_generator {
                            self.session.hovered_generator = hovered;
                            self.dirty = true;
                        }
                    }
                } else if self.session.boot_done(now)
                    && self.session.dive.is_none()
                    && !self.session.help
                    && self.session.edit.is_none()
                    && !self.session.confirm_descent
                    && !self.session.offline_visible
                    && let MouseEventKind::Down(button) = mouse.kind
                    && let Some(command) = command_for_click(
                        &self.session.hit_map,
                        mouse.column,
                        mouse.row,
                        button,
                        mouse.modifiers.contains(KeyModifiers::SHIFT),
                    )
                {
                    if self.session.tutorial_blocks(command) {
                        self.session.nudge_wrong(now);
                        self.dirty = true;
                    } else {
                        let logical_ms = self.session.started_at.elapsed().as_millis() as u64;
                        if self.queue.push(command, logical_ms) {
                            self.dirty = true;
                        }
                    }
                }
            }
            CrosstermEvent::Resize(_, _) => {
                self.dirty = true;
            }
            CrosstermEvent::Paste(text) if self.session.edit.is_some() => {
                self.session.edit_paste(&text);
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// Drain commands, advance time/automation/events, and update animations.
    pub fn tick(&mut self, now: Instant) {
        for command in self.queue.drain_frame() {
            let _ = self.session.apply_command(command, now_unix());
            self.dirty = true;
        }
        if self.session.should_quit {
            return;
        }

        let delta = now.saturating_duration_since(self.last_time);
        self.last_time = now;
        self.session.advance_boot(now);

        // The boot intro animates (radar sweep, wave, beat stages) so it must
        // redraw every frame while active, independent of the gameplay block.
        if !self.session.boot_done(now) {
            self.dirty = true;
        }

        if self.session.boot_done(now) {
            if delta > GAP {
                if self.session.dive.is_some() {
                    self.session.finish_dive(now);
                }
                let seconds = delta.as_secs();
                let report = simulate_offline(&mut self.session.engine, seconds);
                self.session.push_toast(
                    format!(
                        "caught up {} (+{} echo)",
                        fmt_duration(report.credited_secs),
                        amount(report.echo_delta)
                    ),
                    now,
                );
                self.session.save_now(now_unix());
                self.dirty = true;
            } else {
                self.accumulator += delta;
                let mut steps = 0u32;
                while self.accumulator >= STEP && steps < MAX_STEPS {
                    self.session.engine.tick();
                    self.accumulator -= STEP;
                    steps += 1;
                }
                if steps == MAX_STEPS {
                    self.accumulator = Duration::ZERO;
                }
                if steps > 0 {
                    self.dirty = true;
                }
            }

            if self
                .session
                .dive
                .as_ref()
                .is_some_and(|dive| dive.finished(now))
            {
                self.session.finish_dive(now);
                self.dirty = true;
            }

            let events: Vec<Event> = self.session.engine.drain_events().collect();
            if !events.is_empty() {
                if self.session.dive.is_some() {
                    self.session.pending_events.extend(events);
                } else {
                    self.session.process_events(events, now);
                }
                self.dirty = true;
            }
            self.session.flush_purchases(now);

            let echo_now = self.session.engine.view().echo();
            self.session.anim.observe_echo(echo_now, now);
            self.session
                .anim
                .sample_rate(self.session.engine.view().rate(), now);

            if now.saturating_duration_since(self.last_autosave) >= AUTOSAVE {
                self.session.save_now(now_unix());
                self.last_autosave = now;
                self.dirty = true;
            }
        }

        self.session.sync_tutorial(now, now_unix());

        if self.session.bell && self.session.bell_once {
            ring();
            self.session.bell_once = false;
        }

        self.session.expire_toasts(now);

        // Expire stale nudge and force a redraw so the coach status row updates.
        if let Some((_, until)) = self.session.tutorial_nudge
            && now >= until
        {
            self.session.tutorial_nudge = None;
            self.dirty = true;
        }
    }

    /// Draw one frame and refresh the click map.
    ///
    /// # Errors
    ///
    /// Returns the backend's error.
    pub fn render<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        now: Instant,
    ) -> Result<(), B::Error> {
        let screen = self.session.screen(now);
        let anim = self.session.ui_anim(now);
        let settings = self.session.settings_view();
        let banner = self.session.banner();
        let offline = if self.session.offline_visible {
            self.session.offline.as_ref()
        } else {
            None
        };
        let notice = self.session.notice.as_deref();
        let edit = self.session.edit.as_ref().map(|edit| EditView {
            kind: edit.kind,
            buffer: &edit.buffer,
            error: edit.error.as_deref(),
        });
        let now_secs = now_unix();
        let mut hits = HitMap::default();
        {
            let ui = UiState {
                view: self.session.engine.view(),
                palette: self.session.palette,
                ascii: self.session.ascii,
                screen,
                banner,
                offline,
                toasts: &self.session.toasts,
                anim: &anim,
                focus: self.session.focus,
                confirm_descent: self.session.confirm_descent,
                notice,
                archive_scroll: self.session.archive_scroll,
                settings,
                save_path: &self.session.save_path,
                last_save: self.session.last_save_unix,
                now_unix: now_secs,
                edit,
                help: self.session.help,
                help_scroll: self.session.help_scroll,
                demo: self.session.demo,
                hovered: self.session.hovered_generator,
                tutorial: Some(self.session.engine.tutorial()),
                confirm_skip: self.session.confirm_tutorial_skip,
                confirm_replay: self.session.confirm_tutorial_replay,
                tutorial_nudge: self
                    .session
                    .tutorial_nudge
                    .as_ref()
                    .and_then(|(text, until)| (now < *until).then_some(text.as_str())),
            };
            terminal.draw(|frame| {
                hits = render(frame, &ui);
            })?;
        }
        if self.session.help {
            self.session.help_total = hits.help_total;
            self.session.help_visible = hits.help_visible;
            self.session.help_scroll = self.session.help_scroll.min(
                self.session
                    .help_total
                    .saturating_sub(self.session.help_visible),
            );
        }
        if self.session.current == ScreenKind::Archive && hits.archive_total > 0 {
            self.session.archive_total = hits.archive_total;
            self.session.archive_visible = hits.archive_visible;
            self.session.archive_scroll = self.session.archive_scroll.min(
                self.session
                    .archive_total
                    .saturating_sub(self.session.archive_visible),
            );
        }
        self.session.hit_map = hits;
        self.last_draw = now;
        self.dirty = false;
        Ok(())
    }

    /// Tick and render when due (the loop's per-interval work).
    #[cfg(test)]
    pub fn step<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        now: Instant,
    ) -> Result<(), B::Error> {
        self.tick(now);
        if self.should_redraw(now) {
            self.render(terminal, now)?;
        }
        Ok(())
    }
}

/// Drive the session until quit.
///
/// # Errors
///
/// Returns an I/O error from the terminal backend.
pub fn run(
    session: Session,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    _bell: bool,
) -> std::io::Result<()> {
    let mut app = App::new(session);
    loop {
        if crossterm::event::poll(POLL)? {
            let event = crossterm::event::read()?;
            app.handle_event(event, Instant::now());
        }
        let now = Instant::now();
        app.tick(now);
        if app.should_quit() {
            break;
        }
        if app.should_redraw(now) {
            app.render(terminal, now)?;
        }
    }
    app.session.save_now(now_unix());
    Ok(())
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use crossterm::event::KeyEvent;
    use crossterm::event::MouseButton;
    use hadal_core::GameState;
    use hadal_core::content::ids::{AchievementId, LogId};
    use hadal_core::save::SavePaths;
    use ratatui::backend::TestBackend;
    use std::fs;

    fn options(dir: &Path) -> SessionOptions {
        SessionOptions {
            data_dir: dir.to_path_buf(),
            demo: false,
            theme: Theme::Abyss,
            ascii: false,
            mouse: false,
            color: ColorDepth::Auto,
            reduce_motion: true,
            bell: false,
            config: Config::default(),
            config_warning: None,
        }
    }

    /// Build a headless session around an engine and an optional store.
    fn headless(engine: Engine, dir: PathBuf, store: Option<SaveStore>) -> Session {
        let anim = AnimState::new(engine.view().echo());
        let palette = Palette::resolve(Theme::Abyss, ColorDepth::Auto);
        let save_path = sanitize(&SavePaths::new(&dir).save().display().to_string());
        let mut session = Session {
            engine,
            ascii: false,
            mouse: false,
            bell: false,
            palette,
            theme: Theme::Abyss,
            color_depth: ColorDepth::Auto,
            reduce_motion: true,
            config: Config::default(),
            data_dir: dir,
            save_path,
            demo: store.is_none(),
            offline: None,
            offline_visible: false,
            warning: None,
            save_error: None,
            config_error: None,
            should_quit: false,
            started_at: Instant::now(),
            boot: Boot::Done,
            current: ScreenKind::Sonar,
            focus: Focus::None,
            confirm_descent: false,
            notice: None,
            toasts: Vec::new(),
            toast_until: Vec::new(),
            anim,
            dive: None,
            pending_events: Vec::new(),
            hit_map: HitMap::default(),
            archive_scroll: 0,
            archive_total: 0,
            archive_visible: 0,
            edit: None,
            help: false,
            help_scroll: 0,
            help_total: 0,
            help_visible: 0,
            hovered_generator: None,
            tutorial_step: 0,
            tutorial_finished: false,
            confirm_tutorial_skip: false,
            confirm_tutorial_replay: false,
            last_save_unix: None,
            last_hull_click: None,
            bell_once: false,
            tutorial_nudge: None,
            purchase_generators: 0,
            purchase_upgrades: 0,
            purchase_hulls: 0,
            last_purchase_toast: None,
            store,
            _lock: None,
        };
        let tutorial = session.engine.tutorial();
        session.tutorial_step = tutorial.index;
        session.tutorial_finished = tutorial.completed || tutorial.skipped;
        session
    }

    fn demo_with(mutate: impl FnOnce(&mut serde_json::Value)) -> Engine {
        let demo = Engine::demo();
        let mut value = serde_json::to_value(demo.state()).expect("state json");
        mutate(&mut value);
        let state: GameState = serde_json::from_value(value).expect("state");
        Engine::from_state(state)
    }

    #[test]
    fn demo_never_locks_or_saves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let mut session = Session::start(&opts, 1_000).expect("demo start");
        assert!(session.demo);
        assert!(session.save_now(2_000));
        assert!(!SavePaths::new(dir.path()).save().exists());
        assert_eq!(session.engine.view().hull_count(), 15);
        assert_eq!(session.screen(Instant::now()), Screen::Sonar);
        assert!(session.engine.tutorial().completed);
        let _ = session.apply_command(Command::Pulse, 2_000);
    }

    #[test]
    fn offline_credits_cap_then_no_double_credit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_700_000_000u64;
        let saved_at = now - 8 * 3600;

        let fixture = Engine::new();
        let mut seed = SaveStore::new(dir.path());
        seed.save(fixture.state(), saved_at).expect("seed save");
        drop(seed);

        let session = Session::start(&options(dir.path()), now).expect("start");
        let report = session.offline.as_ref().expect("offline report");
        let cap = u64::from(session.engine.view().offline_cap_hours()) * 3600;
        assert_eq!(cap, 8 * 3600, "base cap is 8h");
        assert_eq!(report.credited_secs, cap);
        assert_eq!(report.cap_secs, cap);
        assert!(session.offline_visible);

        let raw = fs::read_to_string(SavePaths::new(dir.path()).save()).expect("read save");
        assert!(raw.contains(&format!("\"saved_at_unix\":{now}")));
        drop(session);

        let again = Session::start(&options(dir.path()), now + 1).expect("restart");
        let report = again.offline.as_ref().expect("report");
        assert!(
            report.credited_secs <= 1,
            "credited {}",
            report.credited_secs
        );
        assert!(!again.offline_visible);
    }

    #[test]
    fn save_failure_sets_banner_and_retry_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"not a directory").expect("write blocker");
        let nested = blocked.join("sub");

        let engine = Engine::new();
        let store = SaveStore::new(&nested);
        let mut session = headless(engine, nested.clone(), Some(store));

        assert!(!session.save_now(1_000), "save must fail");
        assert!(session.banner().is_some(), "banner set");
        assert!(
            session
                .save_error
                .as_deref()
                .unwrap_or("")
                .contains("SAVE FAILED")
        );

        fs::remove_file(&blocked).expect("remove blocker");
        fs::create_dir_all(&blocked).expect("create dir");
        assert!(session.save_now(2_000), "retry succeeds once fixed");
        assert!(session.save_error.is_none(), "banner cleared");
    }

    #[test]
    fn enter_buys_focused_hull() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = demo_with(|value| {
            value["meta"]["fathoms"] = serde_json::json!(2000);
        });
        let mut session = headless(engine, dir.path().to_path_buf(), None);
        session.current = ScreenKind::Descent;
        let index = model::hull_flat_index(HullId::DeepField).expect("deep field index");
        session.focus = Focus::Hull(index);
        let before = session.engine.view().fathoms();

        let command = map_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Mode::Normal,
        )
        .expect("Enter maps");
        assert!(session.apply_command(command, 1_000));
        assert!(session.engine.view().hull_owned(HullId::DeepField));
        assert_eq!(session.engine.view().fathoms(), before - 600);
        assert!(session.notice.is_none());
    }

    #[test]
    fn u_focus_buys_upgrade_via_keymap_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = demo_with(|value| {
            value["run"]["upgrades"] = serde_json::json!([]);
            value["run"]["echo"] = serde_json::json!("5000000000");
        });
        let mut session = headless(engine, dir.path().to_path_buf(), None);
        session.current = ScreenKind::Sonar;

        let focus_command = map_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
            Mode::Normal,
        )
        .expect("u maps");
        assert!(session.apply_command(focus_command, 1_000));
        assert_eq!(session.focus, Focus::Upgrade(0));
        assert!(
            !session
                .engine
                .view()
                .upgrade_owned(UpgradeId::ImpedanceMatch)
        );
        let buy_command = map_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Mode::Normal,
        )
        .expect("Enter maps");
        assert!(session.apply_command(buy_command, 1_000));
        assert!(
            session
                .engine
                .view()
                .upgrade_owned(UpgradeId::ImpedanceMatch)
        );
    }

    #[test]
    fn governor_rejects_invalid_and_requires_hull() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::demo(), dir.path().to_path_buf(), None);
        session.current = ScreenKind::Descent;

        assert!(session.apply_command(Command::GovernorAdjust(1), 1_000));
        assert_eq!(session.engine.view().auto_descend_at(), 0);
        assert!(
            session.notice.as_deref().unwrap_or("").contains("OFF or 3"),
            "{:?}",
            session.notice
        );

        assert!(session.apply_command(Command::GovernorAdjust(10), 1_000));
        assert_eq!(session.engine.view().auto_descend_at(), 10);
        assert!(session.notice.is_none());

        assert!(session.apply_command(Command::GovernorAdjust(-7), 1_000));
        assert_eq!(session.engine.view().auto_descend_at(), 3);

        assert!(session.apply_command(Command::GovernorAdjust(-1), 1_000));
        assert_eq!(session.engine.view().auto_descend_at(), 3);
        assert!(session.notice.is_some());

        let engine = demo_with(|value| {
            value["meta"]["hulls"] = serde_json::json!([]);
        });
        let mut bare = headless(engine, dir.path().to_path_buf(), None);
        bare.current = ScreenKind::Descent;
        assert!(bare.apply_command(Command::GovernorAdjust(10), 1_000));
        assert_eq!(bare.engine.view().auto_descend_at(), 0);
        assert!(
            bare.notice
                .as_deref()
                .unwrap_or("")
                .contains("buy the hull")
        );
    }

    #[test]
    fn dive_duration_progress_and_reduce_motion() {
        let start = Instant::now();
        let dive = DiveState::new(7_912, 8_412, start, false);
        assert!(!dive.finished(start));
        assert!(!dive.finished(start + Duration::from_millis(899)));
        assert!(dive.finished(start + Duration::from_millis(900)));
        assert!((dive.progress(start + Duration::from_millis(450)) - 0.5).abs() < 0.01);
        assert_eq!(
            dive.screen(start + Duration::from_millis(900)),
            Screen::Dive {
                from_m: 7_912,
                to_m: 8_412,
                progress: 1000
            }
        );

        let instant = DiveState::new(7_912, 8_412, start, true);
        assert!(instant.finished(start));
        assert_eq!(instant.progress(start), 1.0);
    }

    #[test]
    fn descent_starts_dive_and_skip_lands_on_sonar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::demo(), dir.path().to_path_buf(), None);
        session.reduce_motion = false;
        session.current = ScreenKind::Descent;
        session.confirm_descent = true;
        let before_descents = session.engine.view().descents();
        let before_depth = session.engine.view().depth_m();

        session.try_descend(1_000, Instant::now());
        assert!(session.dive.is_some(), "dive started");
        assert_eq!(session.engine.view().descents(), before_descents + 1);
        assert_eq!(session.engine.view().depth_m(), before_depth + 500);
        assert_eq!(session.current, ScreenKind::Sonar);

        session.skip_dive(Instant::now());
        assert!(session.dive.is_none());
        assert_eq!(session.current, ScreenKind::Sonar);
        assert!(
            session.toasts.iter().any(|toast| toast.contains("DESCENT")),
            "dive events drained into a toast"
        );
    }

    #[test]
    fn reduce_motion_descent_is_instant() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::demo(), dir.path().to_path_buf(), None);
        session.reduce_motion = true;
        session.current = ScreenKind::Descent;
        session.confirm_descent = true;

        session.try_descend(1_000, Instant::now());
        assert!(session.dive.is_none());
        assert_eq!(session.current, ScreenKind::Sonar);
        assert_eq!(session.engine.view().depth_m(), 9_912);
    }

    #[test]
    fn bell_defaults_off() {
        assert!(!Config::default().bell);
        assert!(!SettingsView::default().bell);
        let dir = tempfile::tempdir().expect("tempdir");
        let session = Session::start(&options(dir.path()), 1_000).expect("start");
        assert!(!session.bell);
    }

    #[test]
    fn double_click_hull_buys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = demo_with(|value| {
            value["meta"]["fathoms"] = serde_json::json!(2000);
        });
        let mut session = headless(engine, dir.path().to_path_buf(), None);
        let index = model::hull_flat_index(HullId::DeepField).expect("deep field index");
        assert!(!session.engine.view().hull_owned(HullId::DeepField));

        assert!(session.apply_command(Command::FocusHullAt(index), 1_000));
        assert!(
            !session.engine.view().hull_owned(HullId::DeepField),
            "single click only focuses"
        );
        assert!(session.apply_command(Command::FocusHullAt(index), 1_000));
        assert!(
            session.engine.view().hull_owned(HullId::DeepField),
            "double click buys"
        );
    }

    #[test]
    fn archive_scroll_clamps_through_commands() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::demo(), dir.path().to_path_buf(), None);
        session.current = ScreenKind::Archive;
        session.archive_total = 50;
        session.archive_visible = 20;

        assert!(session.apply_command(Command::FocusPrev, 1_000));
        assert_eq!(session.archive_scroll, 0);
        for _ in 0..100 {
            let _ = session.apply_command(Command::FocusNext, 1_000);
        }
        assert_eq!(session.archive_scroll, 30);
        let _ = session.apply_command(Command::PageUp, 1_000);
        assert_eq!(session.archive_scroll, 11);
        let _ = session.apply_command(Command::PageDown, 1_000);
        assert_eq!(session.archive_scroll, 30);
    }

    #[test]
    fn settings_toggle_live_and_persist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.reduce_motion = false;
        let mut session = Session::start(&opts, 1_000).expect("start");
        session.current = ScreenKind::System;

        let config_path = SavePaths::new(dir.path()).config();
        let (before, _) = load_config(&config_path);
        assert_eq!(before.theme, hadal_core::save::config::Theme::Abyss);

        session.focus = Focus::System(0);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert_eq!(session.theme, Theme::HighContrast);
        let (after, _) = load_config(&config_path);
        assert_eq!(after.theme, hadal_core::save::config::Theme::HighContrast);

        session.focus = Focus::System(1);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert!(session.reduce_motion);
        let (after, _) = load_config(&config_path);
        assert!(after.reduce_motion);

        session.focus = Focus::System(2);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert!(session.ascii);
        let (after, _) = load_config(&config_path);
        assert!(after.ascii_only);

        session.focus = Focus::System(5);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert!(session.bell);
        let (after, _) = load_config(&config_path);
        assert!(after.bell);

        session.focus = Focus::System(4);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert_eq!(session.color_depth, ColorDepth::TrueColor);
        let (after, _) = load_config(&config_path);
        assert_eq!(
            after.color_depth,
            hadal_core::save::config::ColorDepth::TrueColor
        );
        // Cycle through 256, 16, none and persist none.
        for _ in 0..3 {
            assert!(session.apply_command(Command::FocusActivate, 1_000));
        }
        assert_eq!(session.color_depth, ColorDepth::None);
        let (after, _) = load_config(&config_path);
        assert_eq!(
            after.color_depth,
            hadal_core::save::config::ColorDepth::None
        );
    }

    #[test]
    fn auto_buy_toggle_updates_view_and_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = Session::start(&options(dir.path()), 1_000).expect("start");
        session.current = ScreenKind::System;
        session.focus = Focus::System(6);
        let id = GeneratorId::Hydrophone;
        assert!(session.engine.view().auto_buy(id));
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert!(!session.engine.view().auto_buy(id));

        // Persisted in the game save and visible after restart.
        drop(session);
        let reopened = Session::start(&options(dir.path()), 1_001).expect("restart");
        assert!(!reopened.engine.view().auto_buy(id));
    }

    #[test]
    fn invalid_import_leaves_save_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = Session::start(&options(dir.path()), 1_000).expect("start");
        let save_path = SavePaths::new(dir.path()).save();
        let before = fs::read(&save_path).expect("read save");
        let bad_path = dir.path().join("bad.json");
        fs::write(&bad_path, b"{}").expect("write bad");

        session.current = ScreenKind::System;
        session.open_edit(EditKind::Import, 1_000);
        if let Some(edit) = session.edit.as_mut() {
            edit.buffer = bad_path.to_string_lossy().to_string();
        }
        session.edit_submit(2_000);
        assert!(session.edit.is_some(), "overlay stays open");
        assert!(
            session
                .edit
                .as_ref()
                .and_then(|e| e.error.as_ref())
                .is_some()
        );
        let after = fs::read(&save_path).expect("read save");
        assert_eq!(before, after, "invalid import must not touch the save");
    }

    #[test]
    fn discard_mismatch_is_rejected_then_resets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = Session::start(&options(dir.path()), 1_000).expect("start");
        session.save_manual(1_000);
        dsync(&mut session);
        let save_path = SavePaths::new(dir.path()).save();
        assert!(save_path.exists());

        session.current = ScreenKind::System;
        session.open_edit(EditKind::Reset, 1_000);
        if let Some(edit) = session.edit.as_mut() {
            edit.buffer = "discard".to_owned();
        }
        session.edit_submit(2_000);
        assert!(session.edit.is_some(), "mismatch rejected");
        assert!(
            session
                .edit
                .as_ref()
                .and_then(|e| e.error.as_ref())
                .is_some()
        );
        assert!(save_path.exists(), "save untouched");

        if let Some(edit) = session.edit.as_mut() {
            edit.buffer = "DISCARD".to_owned();
        }
        session.edit_submit(3_000);
        assert!(session.edit.is_none());
        assert_eq!(session.engine.view().descents(), 0);
        assert_eq!(session.engine.view().hull_count(), 0);
        assert!(matches!(
            session.screen(Instant::now()),
            Screen::Boot { .. }
        ));
        assert!(
            !SavePaths::new(dir.path()).backup().exists(),
            "backup removed"
        );
    }

    fn dsync(session: &mut Session) {
        // Force a deterministic save before reset tests.
        let _ = session.save_now(1_000);
    }

    #[test]
    fn toasts_cap_and_expire() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        let now = Instant::now();
        for index in 0..5 {
            session.push_toast(format!("toast {index}"), now);
        }
        assert_eq!(session.toasts.len(), TOAST_CAP);
        assert_eq!(session.toasts[0], "toast 2");
        session.expire_toasts(now + TOAST_TTL + Duration::from_millis(1));
        assert!(session.toasts.is_empty());
    }

    #[test]
    fn bell_only_on_major_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        let now = Instant::now();
        session.process_events(
            vec![
                Event::CarrierChanged { band: 1 },
                Event::PurchasedGenerator {
                    id: GeneratorId::Hydrophone,
                    count: 1,
                    cost: Decimal::from_u64(1),
                },
            ],
            now,
        );
        assert!(!session.bell_once);
        session.process_events(
            vec![Event::AchievementUnlocked(
                hadal_core::content::ids::AchievementId::FirstContact,
            )],
            now,
        );
        assert!(session.bell_once);
        session.bell_once = false;
        session.process_events(vec![Event::Descended { gain: 3 }], now);
        assert!(session.bell_once);
    }

    #[test]
    fn help_opens_and_closes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        assert!(session.apply_command(Command::Help, 1_000));
        assert!(session.help);
        assert!(session.apply_command(Command::Cancel, 1_000));
        assert!(!session.help);
    }

    #[test]
    fn config_write_failure_sets_banner_and_recovers() {
        let root = tempfile::tempdir().expect("tempdir");
        let blocked = root.path().join("blocked");
        fs::write(&blocked, b"not a directory").expect("write blocker");
        let nested = blocked.join("sub");

        let engine = Engine::new();
        let mut session = headless(engine, nested.clone(), None);
        // Config persistence is a real-session concern (demo is a no-op).
        session.demo = false;
        session.current = ScreenKind::System;
        session.focus = Focus::System(0);
        assert!(session.apply_command(Command::FocusActivate, 1_000));
        assert!(session.config_error.is_some());
        assert!(session.banner().is_some());

        fs::remove_file(&blocked).expect("remove blocker");
        fs::create_dir_all(&blocked).expect("create dir");
        assert!(session.apply_command(Command::FocusActivate, 2_000));
        assert!(session.config_error.is_none());
    }

    #[test]
    fn text_entry_consumes_and_edits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        session.open_edit(EditKind::Import, 1_000);
        assert!(
            session
                .edit
                .as_ref()
                .is_some_and(|edit| edit.buffer.is_empty())
        );
        session.edit_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), 1_000);
        session.edit_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE), 1_000);
        session.edit_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), 1_000);
        assert_eq!(
            session.edit.as_ref().map(|edit| edit.buffer.as_str()),
            Some("a")
        );
        // Control characters are filtered.
        session.edit_key(
            KeyEvent::new(KeyCode::Char('\u{1b}'), KeyModifiers::NONE),
            1_000,
        );
        assert_eq!(
            session.edit.as_ref().map(|edit| edit.buffer.as_str()),
            Some("a")
        );
        session.edit_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 1_000);
        assert!(session.edit.is_none());
    }

    // -----------------------------------------------------------------
    // End-to-end session (the plan's manual playthrough, automated)
    // -----------------------------------------------------------------

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
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

    struct Harness {
        app: App,
        terminal: Terminal<TestBackend>,
        now: Instant,
    }

    impl Harness {
        fn new(app: App, now: Instant) -> Self {
            let terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
            Self { app, terminal, now }
        }

        fn key(&mut self, code: KeyCode) {
            self.app.handle_event(
                CrosstermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)),
                self.now,
            );
            self.app.tick(self.now);
        }

        fn ctrl(&mut self, code: KeyCode) {
            self.app.handle_event(
                CrosstermEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL)),
                self.now,
            );
            self.app.tick(self.now);
        }

        fn advance(&mut self, delta: Duration) {
            self.now += delta;
            self.app.step(&mut self.terminal, self.now).expect("step");
        }

        fn render(&mut self) {
            self.app
                .render(&mut self.terminal, self.now)
                .expect("render");
        }

        fn text(&self) -> String {
            buffer_text(self.terminal.backend().buffer())
        }

        fn view(&self) -> hadal_core::StateView<'_> {
            self.app.session.engine.view()
        }
    }

    #[test]
    fn end_to_end_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.reduce_motion = true;
        let start = Instant::now();
        let session = Session::start(&opts, 1_000).expect("fresh session");
        let mut h = Harness::new(App::new_at(session, start), start);

        assert_eq!(h.view().hull_count(), 0);
        assert_eq!(h.view().descents(), 0);
        assert_eq!(h.view().echo(), Decimal::ZERO);
        assert_eq!(h.view().generator_owned(GeneratorId::Hydrophone), 0);

        // Boot renders, then any key skips it.
        h.render();
        assert!(h.text().contains("RELAY K-9"), "boot frame: {}", h.text());
        h.key(KeyCode::Char('x'));
        assert!(h.app.session.boot_done(h.now));
        h.render();
        assert!(h.text().contains("SONAR"), "sonar frame: {}", h.text());
        assert_eq!(h.app.session.engine.tutorial().index, 0);
        eprintln!("step 0: boot skipped");

        // Pulse.
        h.key(KeyCode::Char(' '));
        let echo_after_pulse = h.view().echo();
        assert_eq!(h.view().pulses_manual(), 1);
        assert!(h.view().achievement_unlocked(AchievementId::FirstContact));
        assert!(h.view().log_recorded(LogId::FirstPulse));
        assert!(echo_after_pulse >= Decimal::from_u64(10));
        eprintln!("step 1 pulse: echo={echo_after_pulse:?} pulses=1 first_contact=yes");

        // Buy a Hydrophone (H buys max; 10 Echo affords one).
        h.key(KeyCode::Char('H'));
        assert_eq!(h.view().generator_owned(GeneratorId::Hydrophone), 1);
        let rate = h.view().rate();
        assert!(rate > Decimal::ZERO);
        eprintln!("step 2 hydrophone: owned=1 rate={rate:?}");

        // Earn a gap, unlock the Drifter (10 Hydrophones), then buy one.
        h.advance(Duration::from_secs(8 * 3600));
        let gap_echo = h.view().echo();
        assert!(gap_echo > Decimal::from_u64(1_000));
        h.key(KeyCode::Char('H'));
        assert!(h.view().generator_owned(GeneratorId::Hydrophone) >= 10);
        assert!(h.view().generator_unlocked(GeneratorId::Drifter));
        h.key(KeyCode::Char('d'));
        assert_eq!(h.view().generator_owned(GeneratorId::Drifter), 1);
        let bands_before = h.view().bands_matched();
        h.key(KeyCode::Char('u'));
        assert_eq!(h.app.session.focus, Focus::Upgrade(0));
        h.key(KeyCode::Enter);
        assert!(h.view().upgrade_owned(UpgradeId::ImpedanceMatch));
        eprintln!("step 3 upgrade: gap_echo={gap_echo:?} drifter=1 impedance=owned");

        // Tune: leave the initial match, then cycle to the carrier.
        h.key(KeyCode::Char('f'));
        for _ in 0..6 {
            if h.view().carrier_matched() {
                break;
            }
            h.key(KeyCode::Char('f'));
        }
        assert!(h.view().carrier_matched(), "tuned to carrier");
        assert!(
            h.view().bands_matched() > bands_before,
            "manual match recorded"
        );
        h.render();
        assert!(h.text().contains("TUNED"));
        eprintln!(
            "step 4 tune: tuned={} carrier={} bands={}",
            h.view().tuned_band(),
            h.view().carrier_band(),
            h.view().bands_matched()
        );

        // Advance to the threshold through the loop's gap/tick path.
        let mut iterations = 0;
        while !h.view().can_descend() {
            iterations += 1;
            assert!(iterations < 20, "descent threshold not reached");
            h.advance(Duration::from_secs(8 * 3600));
            if h.view().can_descend() {
                break;
            }
            for code in [
                KeyCode::Char('C'),
                KeyCode::Char('M'),
                KeyCode::Char('D'),
                KeyCode::Char('H'),
            ] {
                h.key(code);
            }
        }
        assert!(h.view().can_descend());
        eprintln!(
            "step 5 threshold: peak={:?} rate={:?} iterations={iterations}",
            h.view().peak(),
            h.view().rate()
        );

        // Descent screen, confirmation, confirm.
        h.key(KeyCode::Char('2'));
        h.render();
        assert!(h.text().contains("HULL TREE"));
        h.key(KeyCode::Char('p'));
        assert!(h.app.session.confirm_descent);
        h.render();
        assert!(h.text().contains("CONFIRM DESCENT"));
        let fathoms_before = h.view().fathoms();
        h.key(KeyCode::Char('y'));
        assert!(!h.app.session.confirm_descent);
        assert_eq!(h.view().echo(), Decimal::ZERO);
        assert!(h.view().fathoms() >= 3);
        assert!(h.view().fathoms() > fathoms_before);
        assert_eq!(h.view().descents(), 1);
        eprintln!(
            "step 6 descent: fathoms={} descents=1 echo=0",
            h.view().fathoms()
        );

        // Buy Auto-Pulse (cost 1 Fathom, affordable after the first Descent).
        h.key(KeyCode::Char('2'));
        assert_eq!(h.app.session.focus, Focus::Hull(0));
        assert!(h.view().hull_affordable(HullId::AutoPulse));
        h.render();
        assert!(h.text().contains("Auto-Pulse"));
        h.key(KeyCode::Enter);
        assert!(h.view().hull_owned(HullId::AutoPulse));
        eprintln!("step 7 hull: auto-pulse owned");

        // Leave the tutorial before free-play screen exploration; the gate
        // blocks cross-tab navigation mid-step.
        h.key(KeyCode::Char('S'));
        h.key(KeyCode::Char('y'));
        assert!(h.app.session.engine.tutorial().skipped);

        // Render ARCHIVE and SYSTEM.
        h.key(KeyCode::Char('3'));
        h.render();
        assert!(h.text().contains("LOGS"));
        h.key(KeyCode::Char('4'));
        h.render();
        assert!(h.text().contains("SETTINGS"));
        h.key(KeyCode::Char('1'));
        h.render();

        // Save and validate the checksum.
        let hash = h.app.session.engine.state_hash();
        h.ctrl(KeyCode::Char('s'));
        assert!(h.app.session.save_error.is_none(), "manual save succeeded");
        let save_path = SavePaths::new(dir.path()).save();
        let bytes = std::fs::read(&save_path).expect("save file");
        let envelope: hadal_core::Envelope = serde_json::from_slice(&bytes).expect("envelope");
        assert_eq!(
            hadal_core::save::checksum_of(&envelope),
            envelope.checksum,
            "save checksum"
        );
        eprintln!(
            "step 8 save: checksum valid counter={}",
            envelope.save_counter
        );
        let saved_at = envelope.saved_at_unix;

        // Reload: identical state hash (zero elapsed).
        drop(h);
        let second = Session::start(&opts, saved_at).expect("reload");
        assert_eq!(second.engine.state_hash(), hash, "reload hash");
        let before_gap_ticks = second.engine.view().total_ticks();
        drop(second);

        // Rewrite saved_at 8 h into the past and load again.
        let bytes = std::fs::read(&save_path).expect("save file");
        let mut envelope: hadal_core::Envelope = serde_json::from_slice(&bytes).expect("envelope");
        let now = now_unix();
        envelope.saved_at_unix = now.saturating_sub(8 * 3600);
        envelope.checksum = hadal_core::save::checksum_of(&envelope);
        std::fs::write(&save_path, serde_json::to_vec(&envelope).expect("json")).expect("write");
        let third = Session::start(&opts, now).expect("gap load");
        let report = third.offline.as_ref().expect("offline report");
        assert_eq!(report.credited_secs, 8 * 3600);
        assert_eq!(report.cap_secs, 8 * 3600);
        assert!(third.offline_visible);
        assert!(
            third.engine.view().total_ticks() > before_gap_ticks,
            "state advanced"
        );
        let raw = std::fs::read(&save_path).expect("save");
        let saved: hadal_core::Envelope = serde_json::from_slice(&raw).expect("envelope");
        assert_eq!(saved.saved_at_unix, now, "immediate save");
        eprintln!("step 9 offline: credited=28800 cap=28800 summary=true");

        // Render the offline summary once (after the short boot completes).
        let mut h2 = Harness::new(App::new_at(third, Instant::now()), Instant::now());
        h2.app.session.skip_boot();
        h2.render();
        assert!(h2.text().contains("WELCOME BACK"), "offline summary");
        assert!(h2.text().contains("cap 8h 0m"), "cap reported");
        drop(h2);

        // Immediate third load credits ~0.
        let fourth = Session::start(&opts, now + 1).expect("quick load");
        let report = fourth.offline.as_ref().expect("report");
        assert!(
            report.credited_secs <= 1,
            "credited {}",
            report.credited_secs
        );
        assert!(!fourth.offline_visible);
        eprintln!("step 10 reload: credited={}", report.credited_secs);
    }

    // -----------------------------------------------------------------
    // Robustness through the app boundary
    // -----------------------------------------------------------------

    #[test]
    fn corrupt_primary_loads_backup_and_recovers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = Engine::demo();
        let mut seed = SaveStore::new(dir.path());
        seed.save(engine.state(), 1_000).expect("seed 1");
        seed.save(engine.state(), 1_001).expect("seed 2");
        drop(seed);

        let primary = SavePaths::new(dir.path()).save();
        std::fs::write(&primary, b"not a hadal save").expect("corrupt");
        let mut session = Session::start(&options(dir.path()), 2_000).expect("load backup");
        assert!(
            session
                .warning
                .as_deref()
                .unwrap_or("")
                .contains("recovered"),
            "warning: {:?}",
            session.warning
        );
        let corrupt_preserved = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("save.corrupt-")
            });
        assert!(corrupt_preserved, "corrupt primary preserved");
        assert!(session.save_now(2_001), "can save again");
    }

    #[test]
    fn future_schema_is_refused_without_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut envelope = hadal_core::Envelope::sealed(&GameState::new(), 1_000, 1);
        envelope.schema_version = 99;
        envelope.checksum = hadal_core::save::checksum_of(&envelope);
        let path = SavePaths::new(dir.path()).save();
        std::fs::write(&path, serde_json::to_vec(&envelope).expect("json")).expect("write");
        let before = std::fs::read(&path).expect("read");

        let err = Session::start(&options(dir.path()), 2_000)
            .err()
            .expect("must refuse");
        assert!(matches!(&err, StartupError::FutureSchema(_)));
        assert!(err.to_string().contains("newer than supported"));
        let after = std::fs::read(&path).expect("read");
        assert_eq!(before, after, "file unchanged");
    }

    #[test]
    fn second_instance_reports_friendly_lock_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = Engine::new();
        let mut seed = SaveStore::new(dir.path());
        seed.save(engine.state(), 1_000).expect("seed");
        drop(seed);
        let path = SavePaths::new(dir.path()).save();
        let before = std::fs::read(&path).expect("read");

        let _lock = SaveLock::acquire(dir.path()).expect("pre-lock");
        let err = Session::start(&options(dir.path()), 2_000)
            .err()
            .expect("must fail");
        assert!(matches!(&err, StartupError::Lock(_)));
        assert_eq!(err.to_string(), "another HADAL instance is already running");
        assert!(!err.to_string().contains("cannot use data directory"));
        let after = std::fs::read(&path).expect("read");
        assert_eq!(before, after, "save untouched");
    }

    #[test]
    fn non_utf8_save_bytes_are_preserved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = SavePaths::new(dir.path()).save();
        let bytes = b"{\xff\xfe\xfd".to_vec();
        std::fs::write(&path, &bytes).expect("write");

        let err = Session::start(&options(dir.path()), 2_000)
            .err()
            .expect("must fail");
        assert!(matches!(&err, StartupError::Load(_)));
        let primary = std::fs::read(&path).ok();
        let preserved = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("save.corrupt-")
                    && std::fs::read(entry.path()).ok().as_deref() == Some(bytes.as_slice())
            });
        assert!(
            primary.as_deref() == Some(bytes.as_slice()) || preserved,
            "non-UTF8 bytes preserved"
        );
    }

    #[test]
    fn control_characters_in_data_dir_are_sanitized() {
        let dir = PathBuf::from("base/\x1b[31mred\r\n\0tail");
        let session = headless(Engine::new(), dir, None);
        assert!(!session.save_path.contains('\x1b'));
        assert!(!session.save_path.contains('\r'));
        assert!(!session.save_path.contains('\n'));
        assert!(!session.save_path.contains('\0'));
        assert!(session.save_path.contains('\u{FFFD}'));
    }

    #[test]
    fn reduce_motion_disables_animations_not_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        session.reduce_motion = true;
        assert!(session.apply_command(Command::Pulse, 1_000));
        let now = Instant::now();
        session.anim.carrier_flash_until = Some(now + Duration::from_secs(1));
        session.anim.row_flash_until[0] = Some(now + Duration::from_secs(1));

        let anim = session.ui_anim(now);
        assert!(anim.pulse_phase.is_none());
        assert!(!anim.carrier_flash);
        assert_eq!(anim.row_flash, [false; 4]);
        assert_eq!(session.engine.view().pulses_manual(), 1);
        assert!(session.engine.view().echo() > Decimal::ZERO);
    }

    // -----------------------------------------------------------------
    // Measurements / redraw policy
    // -----------------------------------------------------------------

    #[test]
    fn idle_loop_skips_redraws() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = headless(Engine::new(), dir.path().to_path_buf(), None);
        let start = Instant::now();
        let mut app = App::new_at(session, start);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        app.render(&mut terminal, start).expect("render");

        let after_render = start + Duration::from_millis(40);
        app.tick(after_render);
        // The first tick drains any events buffered at startup; clear them.
        app.render(&mut terminal, after_render).expect("render");
        app.tick(after_render + Duration::from_millis(40));
        assert!(
            !app.should_redraw(after_render + Duration::from_millis(40)),
            "idle redraw skipped"
        );
        let after_idle = after_render + Duration::from_millis(40);

        app.handle_event(
            CrosstermEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
            after_idle,
        );
        app.tick(after_idle);
        assert!(
            app.should_redraw(after_idle + Duration::from_millis(40)),
            "state change dirties the UI"
        );
        assert_eq!(app.session.engine.view().pulses_manual(), 1);
    }

    /// Measured ~18 ms (debug). Release budget (AGENTS.md §12): 150 ms.
    /// Bound: 250 ms debug (15× measured), 150 ms release (matches §12 budget).
    #[test]
    fn demo_startup_first_frame_measurement() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let start = Instant::now();
        let session = Session::start(&opts, 1_000).expect("demo");
        let mut app = App::new_at(session, start);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        app.render(&mut terminal, Instant::now()).expect("render");
        let elapsed = start.elapsed();
        eprintln!("measure: demo startup to first frame = {elapsed:?}");
        #[cfg(debug_assertions)]
        assert!(
            elapsed < Duration::from_millis(250),
            "startup {elapsed:?} (debug)"
        );
        #[cfg(not(debug_assertions))]
        assert!(
            elapsed < Duration::from_millis(150),
            "startup {elapsed:?} (release)"
        );
    }

    #[test]
    fn sonar_render_600_frames_measurement() {
        let engine = Engine::demo();
        let anim = Anim::settled(engine.view().echo());
        let ui = UiState::settled(
            engine.view(),
            Palette::resolve(Theme::Abyss, ColorDepth::Auto),
            false,
            Screen::Sonar,
            &anim,
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        let start = Instant::now();
        for _ in 0..600 {
            terminal
                .draw(|frame| {
                    let _ = render(frame, &ui);
                })
                .expect("draw");
        }
        let elapsed = start.elapsed();
        let per = elapsed / 600;
        eprintln!("measure: SONAR 600 frames total={elapsed:?} avg={per:?}");
        assert!(per < Duration::from_millis(33), "render {per:?}/frame");
    }

    #[test]
    fn offline_8h_simulation_measurement() {
        let mut engine = Engine::demo();
        let start = Instant::now();
        let report = simulate_offline(&mut engine, 8 * 3600);
        let elapsed = start.elapsed();
        eprintln!(
            "measure: 8h offline = {elapsed:?} credited={} ticks={}",
            report.credited_secs, report.ticks
        );
        assert_eq!(report.credited_secs, 8 * 3600);
        if cfg!(debug_assertions) {
            // Unoptimized Decimal ticking is ~20x slower; keep a sanity bound.
            assert!(elapsed < Duration::from_secs(5), "offline {elapsed:?}");
        } else {
            assert!(elapsed < Duration::from_millis(25), "offline {elapsed:?}");
        }
    }

    // -----------------------------------------------------------------
    // Mid-run failure recovery
    // -----------------------------------------------------------------

    #[test]
    fn autosave_failure_banner_and_retry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let session = Session::start(&opts, 1_000).expect("start");
        let start = Instant::now();
        let mut app = App::new_at(session, start);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

        let path = SavePaths::new(dir.path()).save();
        fs::remove_file(&path).expect("remove save");
        fs::create_dir(&path).expect("directory in place of save");

        app.step(&mut terminal, start + Duration::from_secs(31))
            .expect("step");
        assert!(app.session.save_error.is_some(), "save error recorded");
        assert!(app.session.banner().is_some(), "banner shown");

        fs::remove_dir(&path).expect("remove directory");
        app.step(&mut terminal, start + Duration::from_secs(62))
            .expect("step");
        assert!(app.session.save_error.is_none(), "retry succeeds");
        assert!(
            fs::metadata(&path).expect("metadata").is_file(),
            "save file written"
        );
    }

    fn draw_engine(engine: &Engine) -> String {
        let anim = Anim::settled(engine.view().echo());
        let ui = UiState::settled(
            engine.view(),
            Palette::resolve(Theme::Abyss, ColorDepth::Auto),
            false,
            Screen::Sonar,
            &anim,
        );
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let _ = render(frame, &ui);
            })
            .expect("draw");
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn sonar_objective_line_three_states() {
        let no_hydro = demo_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([0, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
        });
        assert!(
            draw_engine(&no_hydro).contains("pulse until you can afford a Hydrophone"),
            "no-hydro objective"
        );

        let mismatch = demo_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
            value["run"]["carrier_band"] = serde_json::json!(2);
            value["run"]["tuned_band"] = serde_json::json!(0);
        });
        assert!(
            draw_engine(&mismatch).contains("tune to CARRIER 03"),
            "mismatch objective"
        );

        let descend = demo_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!(["impedance_match"]);
            value["run"]["carrier_band"] = serde_json::json!(0);
            value["run"]["tuned_band"] = serde_json::json!(0);
            value["run"]["peak_echo"] = serde_json::json!("5000000");
        });
        assert!(
            draw_engine(&descend).contains("2 > p > y"),
            "descent-available objective"
        );
    }

    fn tutorial_engine(step: u8, descents: u32) -> Engine {
        demo_with(|value| {
            value["meta"]["tutorial"] = serde_json::json!({
                "step": step,
                "completed": false,
                "skipped": false,
                "seen_carrier_change": true,
                "tuned_since_change": true,
                "manual_opened": false,
                "manual_closed": false,
                "system_opened": false,
                "acknowledged": false,
            });
            value["meta"]["stats"]["descents"] = serde_json::json!(descents);
        })
    }

    #[test]
    fn tutorial_coaches_new_save_and_resumes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let start = Instant::now();
        let session = Session::start(&opts, now_unix()).expect("new save");
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.render();
        let text = h.text();
        assert!(text.contains("TUTORIAL 01/13"), "{text}");
        assert!(text.contains("▶ PULSE"), "pulse highlight: {text}");
        assert!(!text.contains("FIRST RUN"), "briefing must be gone");

        h.key(KeyCode::Char(' '));
        assert_eq!(h.app.session.engine.tutorial().index, 1);
        assert!(
            h.app
                .session
                .toasts
                .iter()
                .any(|toast| toast.contains("pulse received"))
        );
        h.render();
        assert!(h.text().contains("TUTORIAL 02/13"));

        h.key(KeyCode::Char('h'));
        assert_eq!(h.view().generator_owned(GeneratorId::Hydrophone), 1);
        assert_eq!(h.view().echo(), Decimal::ZERO);
        assert_eq!(h.app.session.engine.tutorial().index, 2);
        h.render();
        assert!(h.text().contains("TUTORIAL 03/13"));
        drop(h);

        let session = Session::start(&opts, now_unix() + 1).expect("reload");
        assert_eq!(session.engine.tutorial().index, 2);
        let start = Instant::now();
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.render();
        assert!(h.text().contains("TUTORIAL 03/13"), "{}", h.text());
    }

    #[test]
    fn tutorial_skip_persists_and_system_replay_restarts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let start = Instant::now();
        let session = Session::start(&opts, now_unix()).expect("new save");
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('S'));
        assert!(h.app.session.confirm_tutorial_skip);
        h.render();
        assert!(h.text().contains("SKIP TUTORIAL?"), "{}", h.text());
        h.key(KeyCode::Char('y'));
        assert!(h.app.session.engine.tutorial().skipped);
        assert!(!h.app.session.confirm_tutorial_skip);
        h.render();
        assert!(!h.text().contains("TUTORIAL 01/13"));
        drop(h);

        let session = Session::start(&opts, now_unix() + 1).expect("reload");
        assert!(session.engine.tutorial().skipped);
        let start = Instant::now();
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.render();
        assert!(!h.text().contains("TUTORIAL 01/13"));

        h.key(KeyCode::Char('4'));
        for _ in 0..10 {
            h.key(KeyCode::Down);
        }
        assert_eq!(h.app.session.focus, Focus::System(10));
        h.key(KeyCode::Enter);
        assert!(h.app.session.confirm_tutorial_replay);
        h.render();
        assert!(h.text().contains("REPLAY TUTORIAL?"), "{}", h.text());
        h.key(KeyCode::Char('y'));
        assert_eq!(h.app.session.engine.tutorial().index, 0);
        assert!(!h.app.session.engine.tutorial().skipped);
        h.render();
        assert!(h.text().contains("TUTORIAL 01/13"));
    }

    #[test]
    fn tutorial_active_system_replay_is_immediate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let engine = tutorial_engine(6, 0);
        // Capture the clock once so save and reload share the same second and
        // offline catch-up sees elapsed == 0. Two separate `now_unix()` calls
        // can straddle a second boundary, run offline ticks, and auto-advance
        // the Peak step (6 -> 7).
        let now = now_unix();
        let mut store = SaveStore::new(dir.path());
        store.save(engine.state(), now).expect("seed");
        drop(store);

        let mut session = Session::start(&opts, now).expect("load");
        assert_eq!(session.engine.tutorial().index, 6);
        assert!(session.tutorial_active());
        // The input gate keeps the tutorial active on early steps, so the
        // SYSTEM replay row is not reachable by keyboard mid-tutorial; exercise
        // the immediate-replay branch directly.
        session.tutorial_system_action(now);
        assert!(!session.confirm_tutorial_replay);
        assert_eq!(session.engine.tutorial().index, 0);
    }

    #[test]
    fn tutorial_replay_refused_after_first_descent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let mut engine = tutorial_engine(12, 1);
        engine.tutorial_acknowledge();
        let tutorial = engine.tutorial();
        assert!(tutorial.completed);
        let mut store = SaveStore::new(dir.path());
        store.save(engine.state(), now_unix()).expect("seed");
        drop(store);

        let session = Session::start(&opts, now_unix()).expect("load");
        let start = Instant::now();
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('4'));
        for _ in 0..10 {
            h.key(KeyCode::Down);
        }
        h.key(KeyCode::Enter);
        assert!(!h.app.session.confirm_tutorial_replay);
        assert!(
            h.app
                .session
                .notice
                .as_deref()
                .unwrap_or("")
                .contains("Start a new save to replay the tutorial."),
            "{:?}",
            h.app.session.notice
        );
    }

    #[test]
    fn tutorial_demo_has_no_coach() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let session = Session::start(&opts, 1_000).expect("demo");
        assert!(session.engine.tutorial().completed);
        let start = Instant::now();
        let mut h = Harness::new(App::new_at(session, start), start);
        h.render();
        assert!(!h.text().contains("TUTORIAL 01/13"), "{}", h.text());
        assert!(h.text().contains("DEMO // NOTHING IS SAVED"));
    }

    #[test]
    fn tutorial_ui_events_complete_steps_10_11_and_12() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let engine = tutorial_engine(10, 0);
        let mut store = SaveStore::new(dir.path());
        store.save(engine.state(), now_unix()).expect("seed");
        drop(store);

        let session = Session::start(&opts, now_unix()).expect("load");
        let start = Instant::now();
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        assert_eq!(h.app.session.engine.tutorial().index, 10);

        h.key(KeyCode::Char('4'));
        assert_eq!(h.app.session.engine.tutorial().index, 11);
        h.render();
        assert!(h.text().contains("TUTORIAL 12/13"), "{}", h.text());

        h.key(KeyCode::Char('?'));
        assert!(h.app.session.help);
        h.key(KeyCode::Esc);
        assert!(!h.app.session.help);
        assert_eq!(h.app.session.engine.tutorial().index, 12);
        h.render();
        assert!(h.text().contains("TUTORIAL 13/13"), "{}", h.text());

        h.key(KeyCode::Enter);
        assert!(h.app.session.engine.tutorial().completed);
        assert!(
            h.app
                .session
                .toasts
                .iter()
                .any(|toast| toast.contains("TUTORIAL COMPLETE"))
        );
        h.key(KeyCode::Char('1'));
        h.render();
        assert!(!h.text().contains("TUTORIAL 13/13"));
    }

    #[test]
    fn blocked_key_sets_nudge_and_does_not_act() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let start = Instant::now();
        let session = Session::start(&opts, 1_000).expect("fresh session");
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x')); // skip boot
        assert_eq!(h.app.session.engine.tutorial().index, 0);

        // 'h' (buy Hydrophone) is not the step-0 action: no buy, step stable.
        h.key(KeyCode::Char('h'));
        assert_eq!(
            h.view().generator_owned(GeneratorId::Hydrophone),
            0,
            "blocked key must not buy"
        );
        assert_eq!(h.app.session.engine.tutorial().index, 0);
        let (text, _until) = h.app.session.tutorial_nudge.clone().expect("nudge set");
        assert_eq!(text, "PRESS SPACE", "nudge names the expected key");
    }

    #[test]
    fn blocked_mouse_click_sets_nudge() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.mouse = true;
        let start = Instant::now();
        let session = Session::start(&opts, 1_000).expect("fresh session");
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        h.render();
        let rect = h
            .app
            .session
            .hit_map
            .generators
            .first()
            .expect("generator hit region")
            .0;
        let event = CrosstermEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        h.app.handle_event(event, h.now);
        h.app.tick(h.now);
        assert_eq!(
            h.view().generator_owned(GeneratorId::Hydrophone),
            0,
            "blocked click must not buy"
        );
        let (text, _until) = h.app.session.tutorial_nudge.clone().expect("nudge set");
        assert_eq!(text, "PRESS SPACE");
    }

    #[test]
    fn blocked_screen_key_does_not_switch_screens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = Instant::now();
        let session = headless(tutorial_engine(4, 0), dir.path().to_path_buf(), None);
        let mut h = Harness::new(App::new_at(session, start), start);
        assert_eq!(h.app.session.engine.tutorial().index, 4);
        assert_eq!(h.app.session.screen(h.now), Screen::Sonar);

        // '4' (OpenSystem) is not allowed on the Upgrade step.
        h.key(KeyCode::Char('4'));
        assert_eq!(
            h.app.session.screen(h.now),
            Screen::Sonar,
            "blocked screen key must not switch"
        );
        assert_eq!(h.app.session.engine.tutorial().index, 4);
        let (text, _until) = h.app.session.tutorial_nudge.clone().expect("nudge set");
        assert_eq!(text, "PRESS u", "step 4 expects u (Enter when focused)");
    }

    #[test]
    fn boot_stages_redraw_and_reveal_beats() {
        use hadal_core::content::story::BOOT_BEATS;

        let dir = tempfile::tempdir().expect("tempdir");
        let start = Instant::now();
        let mut session = Session::start(&options(dir.path()), 1_000).expect("fresh session");
        session.reduce_motion = false;
        session.boot = Boot::New { started: start };
        let mut h = Harness::new(App::new_at(session, start), start);

        // While the boot intro is active the frame is dirty even before the
        // first draw, so the animating intro repaints every tick.
        assert!(
            h.app.should_redraw(start),
            "boot must request a redraw before the first frame"
        );

        // ~900 ms: stage 1 — beat 0 only. tick() then render() must show it.
        h.now = start + Duration::from_millis(900);
        h.app.tick(h.now);
        assert!(
            h.app.should_redraw(h.now),
            "boot tick must mark the frame dirty"
        );
        h.render();
        let text = h.text();
        assert!(text.contains(BOOT_BEATS[0]), "stage 1 beat 0: {text}");
        assert!(
            !text.contains(BOOT_BEATS[1]),
            "stage 1 hides beat 1: {text}"
        );

        // ~1500 ms: stage 2 — beats 0 and 1, not 2.
        h.now = start + Duration::from_millis(1500);
        h.app.tick(h.now);
        h.render();
        let text = h.text();
        assert!(text.contains(BOOT_BEATS[0]), "stage 2 beat 0: {text}");
        assert!(text.contains(BOOT_BEATS[1]), "stage 2 beat 1: {text}");
        assert!(
            !text.contains(BOOT_BEATS[2]),
            "stage 2 hides beat 2: {text}"
        );

        // ~2100 ms: stage 3 — all three beats.
        h.now = start + Duration::from_millis(2100);
        h.app.tick(h.now);
        h.render();
        let text = h.text();
        for (i, beat) in BOOT_BEATS.iter().enumerate() {
            assert!(text.contains(beat), "stage 3 beat {i}: {text}");
        }

        // ~2600 ms: the boot is over and SONAR is shown.
        h.now = start + Duration::from_millis(2600);
        h.app.tick(h.now);
        assert_eq!(h.app.session.screen(h.now), Screen::Sonar);
    }

    #[test]
    fn boot_is_animated_and_skippable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = Instant::now();
        let mut session = Session::start(&options(dir.path()), 1_000).expect("fresh session");
        session.reduce_motion = false;
        session.boot = Boot::New { started: start };

        // Stage thresholds: 800 / 1400 / 2000 ms.
        assert_eq!(session.screen(start), Screen::Boot { revealed: 0 });
        assert_eq!(
            session.screen(start + Duration::from_millis(900)),
            Screen::Boot { revealed: 1 }
        );
        assert_eq!(
            session.screen(start + Duration::from_millis(1500)),
            Screen::Boot { revealed: 2 }
        );
        assert_eq!(
            session.screen(start + Duration::from_millis(2100)),
            Screen::Boot { revealed: 3 }
        );
        assert!(!session.boot_done(start + Duration::from_millis(2499)));
        assert!(session.boot_done(start + Duration::from_millis(2500)));

        // Reduce motion finishes at 800 ms with all beats revealed.
        session.reduce_motion = true;
        session.boot = Boot::New { started: start };
        assert_eq!(session.screen(start), Screen::Boot { revealed: 3 });
        assert!(!session.boot_done(start + Duration::from_millis(799)));
        assert!(session.boot_done(start + Duration::from_millis(800)));

        // Any key at 100 ms skips straight to SONAR with the coach alive.
        let mut h = Harness::new(App::new_at(session, start), start);
        h.now = start + Duration::from_millis(100);
        h.key(KeyCode::Char('x'));
        assert!(h.app.session.boot_done(h.now));
        assert_eq!(h.app.session.screen(h.now), Screen::Sonar);
        let tutorial = h.app.session.engine.tutorial();
        assert_eq!(tutorial.index, 0);
        assert!(
            !tutorial.completed && !tutorial.skipped,
            "coach is active on SONAR"
        );
    }

    #[test]
    fn nested_nonexistent_data_dir_starts_fresh() {
        let root = tempfile::tempdir().expect("tempdir");
        let nested = root.path().join("a").join("b").join("c");
        let mut session = Session::start(&options(&nested), 1_000).expect("nested start");
        assert!(nested.is_dir(), "lock must create the data directory");
        assert_eq!(session.engine.view().echo(), Decimal::ZERO);
        assert_eq!(session.engine.view().descents(), 0);
        assert_eq!(session.engine.view().hull_count(), 0);
        assert!(matches!(session.boot, Boot::New { .. }));
        assert!(session.banner().is_none(), "{:?}", session.banner());
        assert!(!session.banner().unwrap_or("").contains("lock i/o error"));
        session.skip_boot();
        assert_eq!(session.screen(Instant::now()), Screen::Sonar);
        assert_eq!(session.engine.tutorial().index, 0);
        assert!(
            SavePaths::new(&nested).save().exists(),
            "immediate save in the new directory"
        );
    }

    #[test]
    fn empty_data_dir_stays_fresh_install() {
        let root = tempfile::tempdir().expect("tempdir");
        let empty = root.path().join("empty");
        fs::create_dir(&empty).expect("empty dir");
        let mut session = Session::start(&options(&empty), 1_000).expect("empty start");
        assert!(matches!(session.boot, Boot::New { .. }));
        assert_eq!(session.engine.view().descents(), 0);
        assert!(session.banner().is_none(), "{:?}", session.banner());
        session.skip_boot();
        assert_eq!(session.screen(Instant::now()), Screen::Sonar);
        assert_eq!(session.engine.tutorial().index, 0);
    }

    #[test]
    fn demo_does_not_create_data_dir_or_lock() {
        let root = tempfile::tempdir().expect("tempdir");
        let demo_dir = root.path().join("demo-data");
        let mut opts = options(&demo_dir);
        opts.demo = true;
        let mut session = Session::start(&opts, 1_000).expect("demo start");
        assert!(session.demo);
        assert!(session._lock.is_none());
        assert!(session.store.is_none());
        assert!(!demo_dir.exists(), "demo must not create the data dir");
        assert!(session.save_now(2_000));
        assert!(!demo_dir.exists(), "demo save must not create the data dir");
        assert!(!SavePaths::new(&demo_dir).save().exists());
        assert_eq!(session.engine.view().hull_count(), 15);
        assert!(matches!(session.boot, Boot::Done));
        assert!(session.engine.tutorial().completed);
        assert_eq!(session.screen(Instant::now()), Screen::Sonar);
    }

    #[test]
    fn regular_file_data_dir_is_a_friendly_error() {
        let root = tempfile::tempdir().expect("tempdir");
        let file = root.path().join("data-file");
        fs::write(&file, b"not a directory").expect("blocker file");
        let err = Session::start(&options(&file), 1_000)
            .err()
            .expect("regular file must fail");
        let path = sanitize(&file.display().to_string());
        assert!(matches!(&err, StartupError::DataDir(_)), "{err:?}");
        assert_eq!(
            err.to_string(),
            format!("cannot use data directory {path}: path is not a directory")
        );
        assert!(!err.to_string().contains("another HADAL instance"));
        assert!(!err.to_string().contains("lock i/o error"));
        eprintln!("regular-file data-dir error: {err}");
    }

    #[test]
    fn create_dir_failure_is_a_friendly_error() {
        let root = tempfile::tempdir().expect("tempdir");
        let blocker = root.path().join("blocker");
        fs::write(&blocker, b"not a directory").expect("blocker file");
        let nested = blocker.join("data");
        let err = Session::start(&options(&nested), 1_000)
            .err()
            .expect("nested under a file must fail");
        let path = sanitize(&nested.display().to_string());
        assert!(matches!(&err, StartupError::DataDir(_)), "{err:?}");
        let message = err.to_string();
        assert!(
            message.starts_with(&format!("cannot use data directory {path}: ")),
            "{message}"
        );
        assert!(!message.contains("another HADAL instance"));
        assert!(!message.contains("lock i/o error"));
        assert!(!nested.exists());
        eprintln!("create-dir data-dir error: {message}");
    }

    #[test]
    fn data_dir_error_paths_are_sanitized() {
        let err = startup_lock_error(LockError::NotADirectory {
            path: PathBuf::from("bad\x1bname"),
        });
        assert!(matches!(&err, StartupError::DataDir(_)));
        let message = err.to_string();
        assert_eq!(
            message,
            "cannot use data directory bad\u{FFFD}name: path is not a directory"
        );
        assert!(!message.contains('\u{1b}'));
    }

    #[test]
    fn no_save_and_missing_files_are_fresh_install() {
        let path = PathBuf::from("some/dir");
        assert!(is_missing(&SaveError::NoSave { path: path.clone() }));
        assert!(is_missing(&SaveError::Io {
            path,
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        }));
        assert!(!is_missing(&SaveError::ChecksumMismatch));
    }

    #[test]
    fn unknown_config_fields_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = SavePaths::new(dir.path()).config();
        fs::write(
            &path,
            br#"{"theme":"amber_crt","ascii_only":true,"evil":{"nested":true},"__proto__":"x"}"#,
        )
        .expect("write config");
        let (config, warning) = load_config(&path);
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(config.theme, hadal_core::save::config::Theme::AmberCrt);
        assert!(config.ascii_only);
    }

    fn sys_path(dir: &Path) -> String {
        dir.to_string_lossy().to_string()
    }

    fn system_auto_buy(text: &str, name: &str) -> String {
        text.lines()
            .find(|line| line.contains(&format!("AUTO-BUY {name}")))
            .and_then(|line| {
                line.split_whitespace()
                    .find(|token| matches!(*token, "ON" | "OFF"))
            })
            .map(str::to_owned)
            .unwrap_or_default()
    }

    #[test]
    fn system_auto_buy_reads_engine_state_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());

        // Establish a save with Drifter automation OFF through the UI.
        let start = Instant::now();
        let session = Session::start(&opts, 1_000).expect("fresh");
        let mut first = Harness::new(App::new_at(session, start), start);
        first.key(KeyCode::Char('x'));
        // SYSTEM navigation is gated during the tutorial; leave it first.
        first.key(KeyCode::Char('S'));
        first.key(KeyCode::Char('y'));
        first.key(KeyCode::Char('4'));
        for _ in 0..7 {
            first.key(KeyCode::Down);
        }
        assert_eq!(first.app.session.focus, Focus::System(7));
        first.key(KeyCode::Enter);
        assert!(
            !first
                .app
                .session
                .engine
                .view()
                .auto_buy(GeneratorId::Drifter)
        );
        first.render();
        assert_eq!(system_auto_buy(&first.text(), "Drifter"), "OFF");
        drop(first);

        // A fresh App must show the loaded engine's value, not a default.
        let reloaded = Session::start(&opts, now_unix()).expect("reload off");
        let mut second = Harness::new(App::new_at(reloaded, Instant::now()), Instant::now());
        second.key(KeyCode::Char('x'));
        second.key(KeyCode::Char('4'));
        second.render();
        assert_eq!(
            system_auto_buy(&second.text(), "Drifter"),
            "OFF",
            "SYSTEM must read view.auto_buy, not a shadow"
        );
        for _ in 0..7 {
            second.key(KeyCode::Down);
        }
        second.key(KeyCode::Enter);
        assert!(
            second
                .app
                .session
                .engine
                .view()
                .auto_buy(GeneratorId::Drifter)
        );
        second.render();
        assert_eq!(system_auto_buy(&second.text(), "Drifter"), "ON");
        drop(second);

        let reloaded = Session::start(&opts, now_unix()).expect("reload on");
        let mut third = Harness::new(App::new_at(reloaded, Instant::now()), Instant::now());
        third.key(KeyCode::Char('x'));
        third.key(KeyCode::Char('4'));
        third.render();
        assert_eq!(system_auto_buy(&third.text(), "Drifter"), "ON");
    }

    #[test]
    fn system_color_depth_none_persists_and_cli_stays_session_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let config_path = SavePaths::new(dir.path()).config();
        let start = Instant::now();
        // Save at the real clock so the reload is not a huge offline gap.
        let session = Session::start(&opts, now_unix()).expect("start");
        let mut h = Harness::new(App::new_at(session, start), start);
        h.key(KeyCode::Char('x'));
        // SYSTEM navigation is gated during the tutorial; leave it first.
        h.key(KeyCode::Char('S'));
        h.key(KeyCode::Char('y'));
        h.key(KeyCode::Char('4'));
        for _ in 0..4 {
            h.key(KeyCode::Down);
        }
        assert_eq!(h.app.session.focus, Focus::System(4));
        for _ in 0..4 {
            h.key(KeyCode::Enter);
        }
        assert_eq!(h.app.session.color_depth, ColorDepth::None);
        let raw = fs::read_to_string(&config_path).expect("config json");
        assert!(raw.contains("\"none\""), "config.json: {raw}");
        h.render();
        let row = h
            .text()
            .lines()
            .find(|line| line.contains("COLOR DEPTH"))
            .map(str::to_owned)
            .unwrap_or_default();
        assert!(
            row.split_whitespace().any(|token| token == "none"),
            "row: {row}"
        );
        drop(h);

        let path_text = sys_path(dir.path());
        let cli = Cli::parse_from(["hadal", "--data-dir", path_text.as_str()]);
        let reload_opts = resolve_options(&cli);
        assert_eq!(reload_opts.color, ColorDepth::None);
        let reloaded = Session::start(&reload_opts, now_unix()).expect("reload");
        assert_eq!(reloaded.color_depth, ColorDepth::None);
        let mut h2 = Harness::new(App::new_at(reloaded, Instant::now()), Instant::now());
        h2.key(KeyCode::Char('x'));
        h2.key(KeyCode::Char('4'));
        h2.render();
        let row = h2
            .text()
            .lines()
            .find(|line| line.contains("COLOR DEPTH"))
            .map(str::to_owned)
            .unwrap_or_default();
        assert!(
            row.split_whitespace().any(|token| token == "none"),
            "row after reload: {row}"
        );
        drop(h2);

        // The CLI override is session-only: config.json keeps none.
        let cli = Cli::parse_from([
            "hadal",
            "--data-dir",
            path_text.as_str(),
            "--color",
            "truecolor",
        ]);
        let override_opts = resolve_options(&cli);
        assert_eq!(override_opts.color, ColorDepth::TrueColor);
        assert_eq!(
            override_opts.config.color_depth,
            hadal_core::save::config::ColorDepth::None
        );
        let mut overridden = Session::start(&override_opts, now_unix()).expect("override");
        assert_eq!(overridden.color_depth, ColorDepth::TrueColor);
        // Changing an unrelated setting must not write the CLI override.
        overridden.current = ScreenKind::System;
        overridden.focus = Focus::System(0);
        assert!(overridden.apply_command(Command::FocusActivate, now_unix()));
        assert_eq!(overridden.color_depth, ColorDepth::TrueColor);
        let raw = fs::read_to_string(&config_path).expect("config json");
        assert!(raw.contains("\"none\""), "override must not persist: {raw}");
    }

    #[test]
    fn replay_tutorial_restores_sonar_and_clears_focus() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = Instant::now();
        let session = headless(tutorial_engine(4, 0), dir.path().to_path_buf(), None);
        let mut h = Harness::new(App::new_at(session, start), start);
        assert_eq!(h.app.session.engine.tutorial().index, 4);

        // Put the player on SYSTEM with a focused row before replaying.
        h.app.session.current = ScreenKind::System;
        h.app.session.focus = Focus::System(10);

        h.app.session.replay_tutorial(now_unix());
        h.app.dirty = true;

        assert_eq!(
            h.app.session.current,
            ScreenKind::Sonar,
            "replay must switch to SONAR"
        );
        assert_eq!(h.app.session.focus, Focus::None, "replay must clear focus");
        assert_eq!(
            h.app.session.engine.tutorial().index,
            0,
            "replay resets tutorial to step 0"
        );
    }

    #[test]
    fn replay_then_navigate_sonar_not_softlocked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let start = Instant::now();
        let session = headless(tutorial_engine(4, 0), dir.path().to_path_buf(), None);
        let mut h = Harness::new(App::new_at(session, start), start);

        h.app.session.replay_tutorial(now_unix());
        h.app.dirty = true;
        assert_eq!(h.app.session.current, ScreenKind::Sonar);
        assert_eq!(h.app.session.engine.tutorial().index, 0);

        // Key '1' (OpenSonar) is now allowed on step 0 (Pulse) after the fix.
        h.key(KeyCode::Char('1'));
        h.app.tick(h.now);
        assert_eq!(
            h.app.session.current,
            ScreenKind::Sonar,
            "must stay on SONAR"
        );
        assert_eq!(h.app.session.engine.tutorial().index, 0, "step unchanged");

        // Tab click (OpenSonar via hit map) must also work.
        h.render();
        let rect = h
            .app
            .session
            .hit_map
            .tabs
            .iter()
            .find(|(_, tab)| *tab == Tab::Sonar)
            .expect("sonar tab")
            .0;
        let event = CrosstermEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        h.app.handle_event(event, h.now);
        h.app.tick(h.now);
        assert_eq!(h.app.session.current, ScreenKind::Sonar);
    }
    // --- Regression tests for v0.2.1 audit fixes ---

    #[test]
    fn r1_exports_dir_is_provisioned_on_start() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let _session = Session::start(&opts, 1_000).expect("start");
        let exports = SavePaths::new(dir.path()).exports();
        assert!(exports.is_dir(), "exports dir must exist after start");
    }

    #[test]
    fn r1_demo_never_creates_exports_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let _session = Session::start(&opts, 1_000).expect("demo start");
        let exports = SavePaths::new(dir.path()).exports();
        assert!(!exports.exists(), "demo must not create exports dir");
    }

    #[test]
    fn r1_export_default_path_writes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let start = Instant::now();
        let session = Session::start(&opts, now_unix()).expect("start");
        let mut h = Harness::new(App::new_at(session, start), start);

        h.key(KeyCode::Char('x')); // skip boot
        h.key(KeyCode::Char('S')); // skip tutorial
        h.key(KeyCode::Char('y'));
        h.key(KeyCode::Char('4')); // SYSTEM
        h.key(KeyCode::Char('e')); // open EXPORT SAVE

        assert!(h.app.session.edit.is_some(), "export overlay absent");
        // Submit the pre-filled default path with Enter.
        h.key(KeyCode::Enter);

        assert!(
            h.app.session.edit.is_none(),
            "overlay must close on success: {:?}",
            h.app.session.edit.map(|e| e.error)
        );
        assert!(
            h.app
                .session
                .toasts
                .iter()
                .any(|t| t.starts_with("EXPORTED")),
            "success toast missing: {:?}",
            h.app.session.toasts
        );
        let exports = SavePaths::new(dir.path()).exports();
        let count = std::fs::read_dir(&exports).expect("exports dir").count();
        assert!(count >= 1, "export file must be written");
    }

    #[test]
    fn l3_demo_persist_config_is_noop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let mut session = Session::start(&opts, 1_000).expect("demo start");
        // Toggle settings — in demo this should not write config.json.
        session.theme = Theme::Mono;
        session.reduce_motion = false;
        session.apply_setting();
        let config_path = SavePaths::new(dir.path()).config();
        assert!(
            !config_path.exists(),
            "demo persist_config must not write config.json"
        );
        // No files at all should be created in the data dir.
        let count = std::fs::read_dir(dir.path()).expect("read_dir").count();
        assert_eq!(count, 0, "demo must not create files on settings change");
    }

    #[test]
    fn l2_skip_accepts_uppercase_y() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let session = Session::start(&opts, 1_000).expect("start");
        let mut h = Harness::new(App::new_at(session, Instant::now()), Instant::now());
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('S'));
        assert!(h.app.session.confirm_tutorial_skip);
        // Uppercase N cancels.
        h.key(KeyCode::Char('N'));
        assert!(!h.app.session.confirm_tutorial_skip);
        assert!(!h.app.session.engine.tutorial().skipped);
        // Reopen and confirm with uppercase Y.
        h.key(KeyCode::Char('S'));
        assert!(h.app.session.confirm_tutorial_skip);
        h.key(KeyCode::Char('Y'));
        assert!(!h.app.session.confirm_tutorial_skip);
        assert!(h.app.session.engine.tutorial().skipped);
    }

    #[test]
    fn l2_replay_accepts_uppercase_y() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        session.confirm_tutorial_replay = true;
        let mut app = App::new(session);
        app.handle_event(
            CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE)),
            Instant::now(),
        );
        assert!(!app.session.confirm_tutorial_replay);
    }

    #[test]
    fn l2_descent_accepts_uppercase_y() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = demo_with(|value| {
            value["run"]["echo"] = serde_json::json!("1000000000");
        });
        let mut session = headless(engine, dir.path().to_path_buf(), None);
        session.current = ScreenKind::Descent;
        session.confirm_descent = true;
        // Simulate the handle_event path for uppercase Y.
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE));
        let mut app = App::new(session);
        app.handle_event(event, Instant::now());
        // The handler should have processed the descent confirm.
        assert!(!app.session.confirm_descent);
    }

    #[test]
    fn r2_help_q_quits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let session = Session::start(&opts, 1_000).expect("start");
        let mut h = Harness::new(App::new_at(session, Instant::now()), Instant::now());
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('?'));
        assert!(h.app.session.help);
        h.app.session.last_save_unix = None;
        h.key(KeyCode::Char('q'));
        assert!(!h.app.session.help);
        assert!(h.app.session.should_quit);
        assert!(h.app.session.last_save_unix.is_some(), "save attempted");
    }

    #[test]
    fn r2_offline_q_quits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = 1_700_000_000u64;
        let saved_at = now - 8 * 3600;
        let fixture = Engine::new();
        let mut seed = SaveStore::new(dir.path());
        seed.save(fixture.state(), saved_at).expect("seed");
        drop(seed);
        let session = Session::start(&options(dir.path()), now).expect("start");
        assert!(session.offline_visible);
        let mut h = Harness::new(App::new_at(session, Instant::now()), Instant::now());
        // The offline modal is deferred until the short boot completes.
        h.app.session.skip_boot();
        assert!(h.app.session.offline_visible);
        h.app.session.last_save_unix = None;
        h.key(KeyCode::Char('q'));
        assert!(!h.app.session.offline_visible);
        assert!(h.app.session.should_quit);
        assert!(h.app.session.last_save_unix.is_some(), "save attempted");
    }

    #[test]
    fn r2_descent_confirm_q_quits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = demo_with(|value| {
            value["run"]["echo"] = serde_json::json!("1000000000");
        });
        let mut session = headless(engine, dir.path().to_path_buf(), None);
        session.current = ScreenKind::Descent;
        session.confirm_descent = true;
        let mut app = App::new(session);
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::NONE));
        app.handle_event(event, Instant::now());
        assert!(!app.session.confirm_descent);
        assert!(app.session.should_quit);
    }

    #[test]
    fn r2_skip_confirm_q_quits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = options(dir.path());
        let session = Session::start(&opts, 1_000).expect("start");
        let mut h = Harness::new(App::new_at(session, Instant::now()), Instant::now());
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('S'));
        assert!(h.app.session.confirm_tutorial_skip);
        h.app.session.last_save_unix = None;
        h.key(KeyCode::Char('Q'));
        assert!(!h.app.session.confirm_tutorial_skip);
        assert!(h.app.session.should_quit);
        assert!(h.app.session.last_save_unix.is_some(), "save attempted");
    }

    #[test]
    fn r2_edit_q_types_not_quits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = options(dir.path());
        opts.demo = true;
        let session = Session::start(&opts, 1_000).expect("demo start");
        let mut h = Harness::new(App::new_at(session, Instant::now()), Instant::now());
        h.key(KeyCode::Char('x'));
        h.key(KeyCode::Char('4'));
        h.key(KeyCode::Char('e'));
        assert!(h.app.session.edit.is_some());
        h.key(KeyCode::Char('q'));
        // q should be typed into the buffer, not quit.
        assert!(h.app.session.edit.is_some());
        assert!(!h.app.session.should_quit);
    }

    #[test]
    fn r4_skipped_tutorial_replay_restores_coach() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = headless(Engine::new(), dir.path().to_path_buf(), None);
        session.engine.tutorial_skip();
        assert!(session.engine.tutorial().skipped);
        session.replay_tutorial(now_unix());
        // Replay clears the skipped flag so the coach returns; the run and its
        // satisfied steps are untouched.
        assert!(!session.engine.tutorial().skipped);
        assert_eq!(session.engine.tutorial().index, 0);
        assert!(!session.tutorial_finished);
    }

    #[test]
    fn p1_03_stale_save_tmp_removed_on_start() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Plant a stale temp file.
        let save_tmp = SavePaths::new(dir.path()).temp();
        std::fs::write(&save_tmp, "stale").expect("write temp");
        assert!(save_tmp.exists());
        let opts = options(dir.path());
        let _session = Session::start(&opts, 1_000).expect("start");
        assert!(
            !save_tmp.exists(),
            "stale save.json.tmp must be removed on start"
        );
    }

    #[test]
    fn p1_03_live_save_unaffected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = Engine::new();
        let mut seed = SaveStore::new(dir.path());
        seed.save(engine.state(), 1_000).expect("seed");
        drop(seed);
        // Plant a stale temp.
        let save_tmp = SavePaths::new(dir.path()).temp();
        std::fs::write(&save_tmp, "stale").expect("write temp");
        let opts = options(dir.path());
        let session = Session::start(&opts, 2_000).expect("start");
        // Live save must still be loadable.
        assert!(session.engine.view().total_ticks() > 0);
    }
}
