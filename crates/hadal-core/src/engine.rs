//! The core engine: action validation, purchases, descent, events, state hash.

use std::collections::VecDeque;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::content::ids::{AchievementId, GeneratorId, HullId, LogId, UpgradeId};
use crate::content::tables::{self, UpgradeEffect};
use crate::content::tutorial::{COMPLETED_STEP, STEPS, TutorialStep, TutorialStepId};
use crate::decimal::Decimal;
use crate::state::{GameState, TutorialState};
use crate::view::StateView;

/// Maximum buffered events. Oldest are dropped when full.
pub const EVENT_CAPACITY: usize = 64;

/// Upper bound on the Max-buy search, guarding against pathological inputs.
pub const MAX_BUY_SEARCH: u32 = 1_000_000;

/// How many units to buy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuyQuantity {
    /// One unit.
    One,
    /// Ten units.
    Ten,
    /// As many as affordable (must be at least one).
    Max,
}

/// A player or automation action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Fire the pulse capacitor.
    Pulse,
    /// Set the tuned carrier band (`0..=5`).
    SetTuning {
        /// Band index.
        band: u8,
    },
    /// Buy generators.
    BuyGenerator {
        /// Generator to buy.
        id: GeneratorId,
        /// Quantity selector.
        quantity: BuyQuantity,
    },
    /// Buy an in-run upgrade.
    BuyUpgrade(UpgradeId),
    /// Buy a permanent hull.
    BuyHull(HullId),
    /// Enable/disable auto-buy for a generator.
    SetAutoBuy {
        /// Generator whose toggle changes.
        id: GeneratorId,
        /// New toggle value.
        enabled: bool,
    },
    /// Set the Ballast Governor target (0 = off, else 3..=1_000_000).
    SetAutoDescendTarget {
        /// Target Fathom gain.
        fathoms: u32,
    },
    /// Perform a Descent if available.
    Descend,
}

/// Typed action failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ActionError {
    /// Not enough Echo or Fathoms.
    #[error("insufficient funds")]
    InsufficientFunds,
    /// The item is not yet unlocked.
    #[error("locked")]
    Locked,
    /// The item is already owned.
    #[error("already owned")]
    AlreadyOwned,
    /// A required prerequisite hull is not owned.
    #[error("prerequisite not met")]
    PrereqMissing,
    /// The requested quantity is invalid (e.g. Max resolves to zero).
    #[error("invalid quantity")]
    InvalidQuantity,
    /// The requested governor target is invalid.
    #[error("invalid target")]
    InvalidTarget,
    /// The carrier band is out of range.
    #[error("invalid carrier band")]
    InvalidBand,
    /// Peak Echo is below the Descent threshold.
    #[error("descent unavailable")]
    DescentUnavailable,
    /// The pulse capacitor is not full.
    #[error("pulse capacitor not charged")]
    PulseNotCharged,
}

/// A game event for the presentation layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A manual pulse fired.
    Pulse {
        /// Echo granted.
        amount: Decimal,
    },
    /// Generators were purchased.
    PurchasedGenerator {
        /// Generator bought.
        id: GeneratorId,
        /// Units bought.
        count: u32,
        /// Total Echo spent.
        cost: Decimal,
    },
    /// An upgrade was purchased.
    PurchasedUpgrade {
        /// Upgrade bought.
        id: UpgradeId,
        /// Echo spent.
        cost: Decimal,
    },
    /// A hull was purchased.
    PurchasedHull {
        /// Hull bought.
        id: HullId,
        /// Fathoms spent.
        cost: u32,
    },
    /// The tuned band changed.
    Tuned {
        /// New band.
        band: u8,
        /// Whether it matches the carrier.
        matched: bool,
    },
    /// The carrier moved to a new band.
    CarrierChanged {
        /// New carrier band.
        band: u8,
    },
    /// A Descent became available.
    DescentAvailable,
    /// A Descent completed.
    Descended {
        /// Fathoms gained.
        gain: u32,
    },
    /// An achievement unlocked.
    AchievementUnlocked(AchievementId),
    /// A log entry was recorded.
    LogRecorded(LogId),
}

/// UI interactions the tutorial observes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialUiEvent {
    /// The SYSTEM tab was opened.
    SystemOpened,
    /// The field manual was opened.
    ManualOpened,
    /// The field manual was closed.
    ManualClosed,
}

/// Numeric progress for the current tutorial step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TutorialProgress {
    /// Short label for the tracked quantity.
    pub label: &'static str,
    /// Current value.
    pub current: Decimal,
    /// Target value.
    pub target: Decimal,
}

/// Read-only view of the current tutorial step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TutorialView<'a> {
    /// The step definition.
    pub step: &'a TutorialStep,
    /// Step index (clamped to the last step when completed).
    pub index: u8,
    /// Chapter number.
    pub chapter: u8,
    /// Whether the tutorial is finished.
    pub completed: bool,
    /// Whether the player skipped it.
    pub skipped: bool,
    /// Optional numeric progress.
    pub progress: Option<TutorialProgress>,
}

/// The core game engine.
pub struct Engine {
    pub(crate) state: GameState,
    events: VecDeque<Event>,
    /// Edge latch for `DescentAvailable`; reset when a Descent executes.
    descent_available: bool,
    /// Cached production rate, recomputed only when a structural change occurs.
    cached_rate: Decimal,
    dirty: bool,
}

impl Engine {
    /// Creates a fresh engine at the start of a new run.
    pub fn new() -> Self {
        let state = GameState::new();
        let mut events = VecDeque::new();
        events.push_back(Event::LogRecorded(LogId::Boot));
        Self {
            state,
            events,
            descent_available: false,
            cached_rate: Decimal::ZERO,
            dirty: true,
        }
    }

    /// Creates a deterministic late-game demo state for the TUI's `--demo` mode.
    ///
    /// The fixture is built in memory only: it is never saved or loaded, holds
    /// no wall-clock or random input, and produces the same state hash on every
    /// call. Achievements and logs are historically consistent (they persist
    /// across descents even when current counts later fall below a threshold).
    pub fn demo() -> Self {
        let mut state = GameState::new();
        state.meta.tutorial = crate::state::TutorialState {
            step: COMPLETED_STEP,
            ..Default::default()
        };

        state.meta.fathoms = 140;
        state.meta.era_fathoms = 120;
        state.meta.lifetime_fathoms = 300;
        state.meta.depth_m = 9_412;
        state.meta.auto_descend_at = 0;
        for id in [
            HullId::AutoPulse,
            HullId::BallastRecall,
            HullId::TunedBallast,
            HullId::AutoBuyI,
            HullId::AutoBuyII,
            HullId::AutoBuyIII,
            HullId::ChainAutomator,
            HullId::DeepMemory,
            HullId::CoiledHull,
            HullId::NullArray,
            HullId::LongStorage,
            HullId::ExtendedBallast,
            HullId::TidalLoop,
            HullId::AbyssalReservoir,
            HullId::BallastGovernor,
        ] {
            state.meta.hulls.insert(id);
        }
        for id in [
            AchievementId::FirstContact,
            AchievementId::Hydrophile,
            AchievementId::Chorus,
            AchievementId::Drifter,
            AchievementId::Mast,
            AchievementId::Core,
            AchievementId::FirstDescent,
            AchievementId::SixDeep,
            AchievementId::Century,
            AchievementId::AllBands,
        ] {
            state.meta.achievements.insert(id);
        }
        for id in [
            LogId::Boot,
            LogId::FirstPulse,
            LogId::FirstHydro,
            LogId::CarrierLock,
            LogId::FirstDescent,
            LogId::Depth9000,
            LogId::Peak1e9,
            LogId::Fathoms100,
            LogId::AllBands,
            LogId::AutoPulse,
        ] {
            state.meta.logs.insert(id);
        }
        state.meta.bands_seen = [true; 6];
        state.meta.auto_buy = [true; 4];

        state.meta.stats.lifetime_echo = dec_f64(1.2e10);
        state.meta.stats.best_peak = dec_f64(8.4e11);
        state.meta.stats.pulses_manual = 210;
        state.meta.stats.pulses_auto = 900;
        state.meta.stats.purchases = 640;
        state.meta.stats.descents = 12;
        state.meta.stats.total_ticks = 3_600_000;
        state.meta.stats.bands_matched = 180;

        state.run.echo = dec_f64(5.2e8);
        state.run.peak_echo = dec_f64(9.1e8);
        state.run.generators = [46, 28, 14, 6];
        for id in UpgradeId::ALL {
            state.run.upgrades.insert(*id);
        }
        state.run.charge_ticks = tables::PULSE_CAPACITOR_TICKS;
        state.run.carrier_band = 2;
        state.run.tuned_band = 2;
        state.run.carrier_ticks = 300;
        state.run.run_ticks = 42_000;

        Self {
            state,
            events: VecDeque::new(),
            // The peak already exceeds the threshold, so the edge has fired.
            descent_available: true,
            cached_rate: Decimal::ZERO,
            dirty: true,
        }
    }

    /// Applies an action, validating it against current state.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ActionError`] when the action is not currently legal.
    pub fn apply(&mut self, action: Action) -> Result<(), ActionError> {
        let result = match action {
            Action::Pulse => self.do_pulse(),
            Action::SetTuning { band } => self.do_set_tuning(band),
            Action::BuyGenerator { id, quantity } => self.do_buy_generator(id, quantity),
            Action::BuyUpgrade(id) => self.do_buy_upgrade(id),
            Action::BuyHull(id) => self.do_buy_hull(id),
            Action::SetAutoBuy { id, enabled } => self.do_set_auto_buy(id, enabled),
            Action::SetAutoDescendTarget { fathoms } => self.do_set_auto_descend_target(fathoms),
            Action::Descend => self.do_descend(),
        };
        if result.is_ok() {
            self.observe_tutorial();
        }
        result
    }

    /// Returns a read-only view of current state.
    pub fn view(&self) -> StateView<'_> {
        StateView { state: &self.state }
    }

    /// Returns a read-only reference to the full game state.
    ///
    /// This is the persistence boundary: it exists only for saving and
    /// snapshotting the exact state. Presentation code must use
    /// [`Engine::view`] instead. No gameplay may branch on state read through
    /// this method that [`StateView`] does not already expose.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Load/demo constructor that rebuilds an engine from a saved [`GameState`].
    ///
    /// Performs **no** validation: the caller must only pass a state that has
    /// already passed checksum and schema validation (or a trusted in-memory
    /// fixture such as [`Engine::demo`]). The event buffer starts empty, the
    /// production-rate cache is invalidated, and the Descent edge latch is
    /// seeded from the loaded peak so no spurious `DescentAvailable` fires on
    /// the first tick.
    pub fn from_state(state: GameState) -> Self {
        let available = !state.run.peak_echo.is_zero()
            && state.run.peak_echo.exponent10() >= tables::DESCENT_THRESHOLD_EXP;
        Self {
            state,
            events: VecDeque::new(),
            descent_available: available,
            cached_rate: Decimal::ZERO,
            dirty: true,
        }
    }

    /// Returns the current tutorial step view.
    pub fn tutorial(&self) -> TutorialView<'_> {
        let tutorial = &self.state.meta.tutorial;
        let completed = tutorial.completed || tutorial.step >= COMPLETED_STEP;
        let index = tutorial.step.min((STEPS.len() - 1) as u8);
        let step = &STEPS[index as usize];
        let progress = if completed || tutorial.skipped {
            None
        } else {
            tutorial_progress(&self.state, tutorial.step)
        };
        TutorialView {
            step,
            index,
            chapter: step.chapter,
            completed,
            skipped: tutorial.skipped,
            progress,
        }
    }

    /// Records a UI interaction and advances the tutorial if it satisfies the
    /// current step.
    pub fn tutorial_ui(&mut self, event: TutorialUiEvent) {
        {
            let tutorial = &mut self.state.meta.tutorial;
            match event {
                TutorialUiEvent::SystemOpened => tutorial.system_opened = true,
                TutorialUiEvent::ManualOpened => tutorial.manual_opened = true,
                TutorialUiEvent::ManualClosed => tutorial.manual_closed = true,
            }
        }
        self.observe_tutorial();
    }

    /// Acknowledges the final (Offline) step and completes the tutorial.
    pub fn tutorial_acknowledge(&mut self) {
        let tutorial = &mut self.state.meta.tutorial;
        if !tutorial.skipped && !tutorial.completed && tutorial.step == 12 {
            tutorial.acknowledged = true;
            tutorial.step = COMPLETED_STEP;
            tutorial.completed = true;
        }
    }

    /// Skips the tutorial. Idempotent.
    pub fn tutorial_skip(&mut self) {
        let tutorial = &mut self.state.meta.tutorial;
        if !tutorial.completed {
            tutorial.skipped = true;
            tutorial.completed = false;
        }
    }

    /// Restarts the tutorial from step 0 with all helper flags cleared.
    /// Idempotent.
    pub fn tutorial_replay(&mut self) {
        self.state.meta.tutorial = TutorialState::new();
    }

    /// Drains buffered events in order.
    pub fn drain_events(&mut self) -> impl Iterator<Item = Event> + '_ {
        self.events.drain(..)
    }

    /// Returns the compact canonical SHA-256 hex hash of the game state.
    pub fn state_hash(&self) -> String {
        let bytes = serde_json::to_vec(&self.state).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        let digest_bytes: &[u8] = digest.as_ref();
        let mut out = String::with_capacity(64);
        for byte in digest_bytes {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// Advances the simulation exactly one tick (100 ms) in the §5.5 order.
    ///
    /// No wall clock, RNG, filesystem, or formatting is used. Auto actions are
    /// coalesced (no per-pulse/per-purchase events) so long runs cannot flood
    /// the bounded event buffer.
    pub fn tick(&mut self) {
        // 1. Carrier timer.
        self.state.run.carrier_ticks = self.state.run.carrier_ticks.saturating_add(1);
        if self.state.run.carrier_ticks >= tables::CARRIER_CYCLE_TICKS {
            self.state.run.carrier_ticks = 0;
            let count = crate::content::ids::CarrierBand::COUNT;
            let band = (self.state.run.carrier_band + 1) % count;
            self.state.run.carrier_band = band;
            self.state.meta.tutorial.seen_carrier_change = true;
            self.state.meta.tutorial.tuned_since_change = false;
            self.dirty = true;
            self.push_event(Event::CarrierChanged { band });
            if self.state.meta.hulls.contains(&HullId::TunedBallast) {
                // Auto-tune to the carrier: records the band and counts a match.
                self.state.run.tuned_band = band;
                self.state.meta.bands_seen[band as usize] = true;
                self.state.meta.stats.bands_matched =
                    self.state.meta.stats.bands_matched.saturating_add(1);
            }
        }

        // 2. Capacitor refill.
        self.state.run.charge_ticks = self
            .state
            .run
            .charge_ticks
            .saturating_add(1)
            .min(tables::PULSE_CAPACITOR_TICKS);

        // 3. Automation, once per second.
        if self
            .state
            .run
            .run_ticks
            .is_multiple_of(crate::TICKS_PER_SECOND)
        {
            self.auto_buy_one();
            self.chain_automator();
        }

        // 4. Production rate (single shared implementation).
        let rate = self.current_rate();

        // 5. One tick of production.
        let per_tick = rate
            .div(dec_u32(crate::TICKS_PER_SECOND as u32))
            .unwrap_or(Decimal::ZERO);
        self.state.run.echo = self.state.run.echo.add(per_tick);
        self.state.meta.stats.lifetime_echo = self.state.meta.stats.lifetime_echo.add(per_tick);

        // 6. Auto-pulse (at most once per tick).
        if self.state.meta.hulls.contains(&HullId::AutoPulse)
            && self.state.run.charge_ticks >= tables::PULSE_CAPACITOR_TICKS
        {
            let amount = pulse_yield_from_rate(rate, &self.state);
            self.state.run.echo = self.state.run.echo.add(amount);
            self.state.meta.stats.lifetime_echo = self.state.meta.stats.lifetime_echo.add(amount);
            self.state.meta.stats.pulses_auto = self.state.meta.stats.pulses_auto.saturating_add(1);
            self.state.run.charge_ticks = 0;
        }

        // 7. Peak tracking.
        if self.state.run.echo > self.state.run.peak_echo {
            self.state.run.peak_echo = self.state.run.echo;
        }
        if self.state.run.echo > self.state.meta.stats.best_peak {
            self.state.meta.stats.best_peak = self.state.run.echo;
        }

        // 8. Achievements/logs, then the DescentAvailable edge.
        self.refresh_achievements_and_logs();
        let available = !self.state.run.peak_echo.is_zero()
            && self.state.run.peak_echo.exponent10() >= tables::DESCENT_THRESHOLD_EXP;
        if available && !self.descent_available {
            self.descent_available = true;
            self.push_event(Event::DescentAvailable);
        }

        // 9. Ballast Governor.
        if self.state.meta.hulls.contains(&HullId::BallastGovernor)
            && self.state.meta.auto_descend_at > 0
        {
            let tidal = self.state.meta.hulls.contains(&HullId::TidalLoop);
            let fractal = self.state.meta.hulls.contains(&HullId::FractalHull);
            let gain = tables::descent_gain(self.state.run.peak_echo, tidal, fractal);
            if gain > 0 && gain >= self.state.meta.auto_descend_at {
                let _ = self.do_descend();
            }
        }

        // 10. Counters.
        self.state.run.run_ticks = self.state.run.run_ticks.saturating_add(1);
        self.state.meta.stats.total_ticks = self.state.meta.stats.total_ticks.saturating_add(1);

        self.observe_tutorial();
    }

    /// Advances `ticks` fixed steps; equivalent to calling [`Engine::tick`]
    /// that many times.
    pub fn advance(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    /// Returns the current production rate, recomputing only when dirty.
    fn current_rate(&mut self) -> Decimal {
        if self.dirty {
            self.cached_rate = production_rate(&self.state);
            self.dirty = false;
        }
        self.cached_rate
    }

    /// Advances the tutorial while the current step is satisfied, at most 16
    /// steps per call. The final (Offline) step waits for acknowledgement.
    fn observe_tutorial(&mut self) {
        let mut advanced = 0u8;
        loop {
            let tutorial = &self.state.meta.tutorial;
            if tutorial.completed || tutorial.skipped {
                break;
            }
            let step = tutorial.step;
            if step >= 12 || step >= COMPLETED_STEP {
                break;
            }
            if !tutorial_step_satisfied(&self.state, step) {
                break;
            }
            self.state.meta.tutorial.step = step + 1;
            advanced += 1;
            if advanced >= 16 {
                break;
            }
        }
    }

    // -----------------------------------------------------------------
    // Internal actions
    // -----------------------------------------------------------------

    fn do_pulse(&mut self) -> Result<(), ActionError> {
        if self.state.run.charge_ticks < tables::PULSE_CAPACITOR_TICKS {
            return Err(ActionError::PulseNotCharged);
        }
        let amount = pulse_yield(&self.state);
        self.state.run.echo = self.state.run.echo.add(amount);
        if self.state.run.echo > self.state.run.peak_echo {
            self.state.run.peak_echo = self.state.run.echo;
        }
        if self.state.run.peak_echo > self.state.meta.stats.best_peak {
            self.state.meta.stats.best_peak = self.state.run.peak_echo;
        }
        self.state.meta.stats.lifetime_echo = self.state.meta.stats.lifetime_echo.add(amount);
        self.state.meta.stats.pulses_manual = self.state.meta.stats.pulses_manual.saturating_add(1);
        self.state.run.charge_ticks = 0;
        self.push_event(Event::Pulse { amount });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    fn do_set_tuning(&mut self, band: u8) -> Result<(), ActionError> {
        if band > 5 {
            return Err(ActionError::InvalidBand);
        }
        self.state.run.tuned_band = band;
        self.dirty = true;
        let matched = band == self.state.run.carrier_band;
        if matched {
            self.state.meta.bands_seen[band as usize] = true;
            self.state.meta.stats.bands_matched =
                self.state.meta.stats.bands_matched.saturating_add(1);
        }
        let tutorial = &self.state.meta.tutorial;
        let on_tune_step = !tutorial.completed
            && !tutorial.skipped
            && STEPS
                .get(tutorial.step as usize)
                .map(|step| step.id == TutorialStepId::Tune)
                .unwrap_or(false);
        if on_tune_step {
            self.state.meta.tutorial.tuned_since_change = true;
        }
        self.push_event(Event::Tuned { band, matched });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    /// Buys `count` units of a generator without emitting an event.
    fn buy_generator_units(&mut self, id: GeneratorId, count: u32) -> Result<(), ActionError> {
        if !tables::generator_def(id)
            .unlock
            .met(&self.state.run.generators)
        {
            return Err(ActionError::Locked);
        }
        if count == 0 {
            return Err(ActionError::InsufficientFunds);
        }
        let index = id.index();
        let owned = self.state.run.generators[index];
        let cost = generator_cumulative_cost(id, owned, count);
        if cost > self.state.run.echo {
            return Err(ActionError::InsufficientFunds);
        }
        self.state.run.echo = self.state.run.echo.sub(cost);
        self.state.run.generators[index] = owned.saturating_add(count);
        self.state.meta.stats.purchases = self.state.meta.stats.purchases.saturating_add(1);
        self.dirty = true;
        Ok(())
    }

    fn do_buy_generator(
        &mut self,
        id: GeneratorId,
        quantity: BuyQuantity,
    ) -> Result<(), ActionError> {
        if !tables::generator_def(id)
            .unlock
            .met(&self.state.run.generators)
        {
            return Err(ActionError::Locked);
        }
        let owned = self.state.run.generators[id.index()];
        let count = match quantity {
            BuyQuantity::One => 1,
            BuyQuantity::Ten => 10,
            BuyQuantity::Max => max_affordable(id, owned, self.state.run.echo),
        };
        if count == 0 {
            return Err(ActionError::InsufficientFunds);
        }
        let cost = generator_cumulative_cost(id, owned, count);
        if cost > self.state.run.echo {
            return Err(ActionError::InsufficientFunds);
        }
        self.buy_generator_units(id, count)?;
        self.push_event(Event::PurchasedGenerator { id, count, cost });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    /// Buys one upgrade without emitting an event.
    fn buy_upgrade_units(&mut self, id: UpgradeId) -> Result<(), ActionError> {
        if self.state.run.upgrades.contains(&id) {
            return Err(ActionError::AlreadyOwned);
        }
        let def = tables::upgrade_def(id);
        if !def.unlock.met(&self.state.run.generators) {
            return Err(ActionError::Locked);
        }
        let cost = Decimal::from_u64(def.cost as u64);
        if self.state.run.echo < cost {
            return Err(ActionError::InsufficientFunds);
        }
        self.state.run.echo = self.state.run.echo.sub(cost);
        self.state.run.upgrades.insert(id);
        self.state.meta.stats.purchases = self.state.meta.stats.purchases.saturating_add(1);
        self.dirty = true;
        Ok(())
    }

    fn do_buy_upgrade(&mut self, id: UpgradeId) -> Result<(), ActionError> {
        let cost = Decimal::from_u64(tables::upgrade_def(id).cost as u64);
        self.buy_upgrade_units(id)?;
        self.push_event(Event::PurchasedUpgrade { id, cost });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    fn do_buy_hull(&mut self, id: HullId) -> Result<(), ActionError> {
        if self.state.meta.hulls.contains(&id) {
            return Err(ActionError::AlreadyOwned);
        }
        let def = tables::hull_def(id);
        if let Some(prereq) = def.prereq
            && !self.state.meta.hulls.contains(&prereq)
        {
            return Err(ActionError::PrereqMissing);
        }
        let cost = Decimal::from_u64(def.cost as u64);
        if Decimal::from_u64(self.state.meta.fathoms as u64) < cost {
            return Err(ActionError::InsufficientFunds);
        }
        self.state.meta.fathoms -= def.cost;
        self.state.meta.hulls.insert(id);
        self.state.meta.stats.purchases = self.state.meta.stats.purchases.saturating_add(1);
        self.dirty = true;
        self.push_event(Event::PurchasedHull { id, cost: def.cost });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    fn do_set_auto_buy(&mut self, id: GeneratorId, enabled: bool) -> Result<(), ActionError> {
        self.state.meta.auto_buy[id.index()] = enabled;
        Ok(())
    }

    fn do_set_auto_descend_target(&mut self, fathoms: u32) -> Result<(), ActionError> {
        let valid = fathoms == 0
            || (tables::GOVERNOR_TARGET_MIN..=tables::GOVERNOR_TARGET_MAX).contains(&fathoms);
        if !valid {
            return Err(ActionError::InvalidTarget);
        }
        self.state.meta.auto_descend_at = fathoms;
        Ok(())
    }

    /// Performs a Descent: grants Fathoms, then resets the run.
    ///
    /// The run resets Echo/Peak to zero, generators to 5 Hydrophones when
    /// Ballast Recall is owned (else zero), clears upgrades, refills the
    /// capacitor, and sets both `carrier_band` and `tuned_band` to 0. Because
    /// they are equal, a new run begins matched (×1.5) — the literal §6.4 reset.
    fn do_descend(&mut self) -> Result<(), ActionError> {
        if self.state.run.peak_echo.is_zero()
            || self.state.run.peak_echo.exponent10() < tables::DESCENT_THRESHOLD_EXP
        {
            return Err(ActionError::DescentUnavailable);
        }
        let tidal = self.state.meta.hulls.contains(&HullId::TidalLoop);
        let fractal = self.state.meta.hulls.contains(&HullId::FractalHull);
        let gain = tables::descent_gain(self.state.run.peak_echo, tidal, fractal);
        if gain == 0 {
            return Err(ActionError::DescentUnavailable);
        }

        let meta = &mut self.state.meta;
        meta.fathoms = meta.fathoms.saturating_add(gain);
        meta.era_fathoms = meta.era_fathoms.saturating_add(gain);
        meta.lifetime_fathoms = meta.lifetime_fathoms.saturating_add(gain);
        meta.stats.descents = meta.stats.descents.saturating_add(1);
        meta.depth_m = meta.depth_m.saturating_add(tables::DEPTH_PER_DESCENT_M);

        let recall = meta.hulls.contains(&HullId::BallastRecall);
        let run = &mut self.state.run;
        run.echo = Decimal::ZERO;
        run.peak_echo = Decimal::ZERO;
        run.generators = if recall {
            [tables::BALLAST_RECALL_HYDROPHONES, 0, 0, 0]
        } else {
            [0; 4]
        };
        run.upgrades.clear();
        run.charge_ticks = tables::PULSE_CAPACITOR_TICKS;
        run.carrier_band = 0;
        run.tuned_band = 0;
        run.carrier_ticks = 0;
        run.run_ticks = 0;

        self.descent_available = false;
        self.dirty = true;
        self.push_event(Event::Descended { gain });
        self.refresh_achievements_and_logs();
        Ok(())
    }

    /// Auto-buy one best-value generator among enabled, unlocked, affordable
    /// tiers permitted by the owned Auto-Buy hulls.
    fn auto_buy_one(&mut self) {
        let limit = auto_buy_tier_limit(&self.state.meta.hulls);
        if limit == 0 {
            return;
        }
        let mut best: Option<(GeneratorId, Decimal)> = None;
        for id in GeneratorId::ALL.iter().take(limit) {
            let index = id.index();
            if !self.state.meta.auto_buy[index] {
                continue;
            }
            if !tables::generator_def(*id)
                .unlock
                .met(&self.state.run.generators)
            {
                continue;
            }
            let cost = generator_unit_cost(*id, self.state.run.generators[index]);
            if cost > self.state.run.echo {
                continue;
            }
            let gain = dec_f64(tables::generator_def(*id).base_rate)
                .mul(generator_tier_mult(&self.state, *id));
            let value = gain.div(cost).unwrap_or(Decimal::ZERO);
            match &best {
                None => best = Some((*id, value)),
                Some((_, best_value)) if value > *best_value => best = Some((*id, value)),
                Some(_) => {}
            }
        }
        if let Some((id, _)) = best {
            let _ = self.buy_generator_units(id, 1);
        }
    }

    /// Chain Automator: buys the cheapest affordable, unlocked, unowned
    /// upgrade, at most one per invocation.
    fn chain_automator(&mut self) {
        if !self.state.meta.hulls.contains(&HullId::ChainAutomator) {
            return;
        }
        let mut best: Option<(UpgradeId, u32)> = None;
        for id in UpgradeId::ALL {
            if self.state.run.upgrades.contains(id) {
                continue;
            }
            let def = tables::upgrade_def(*id);
            if !def.unlock.met(&self.state.run.generators) {
                continue;
            }
            let cost = Decimal::from_u64(def.cost as u64);
            if cost > self.state.run.echo {
                continue;
            }
            match best {
                None => best = Some((*id, def.cost)),
                Some((_, best_cost)) if def.cost < best_cost => best = Some((*id, def.cost)),
                Some(_) => {}
            }
        }
        if let Some((id, _)) = best {
            let _ = self.buy_upgrade_units(id);
        }
    }

    fn push_event(&mut self, event: Event) {
        if self.events.len() >= EVENT_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn refresh_achievements_and_logs(&mut self) {
        if self.state.meta.achievements.len() == AchievementId::ALL.len()
            && self.state.meta.logs.len() == LogId::ALL.len()
        {
            return;
        }
        for id in AchievementId::ALL {
            if !self.state.meta.achievements.contains(id)
                && tables::achievement_reached(*id, &self.state)
            {
                self.state.meta.achievements.insert(*id);
                // The +1% bonus applies from the next tick; force a rate recompute.
                self.dirty = true;
                self.push_event(Event::AchievementUnlocked(*id));
            }
        }
        for id in LogId::ALL {
            if !self.state.meta.logs.contains(id) && tables::log_triggered(*id, &self.state) {
                self.state.meta.logs.insert(*id);
                self.push_event(Event::LogRecorded(*id));
            }
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------
// Derived mechanics (shared with the view)
// ---------------------------------------------------------------------

/// Converts a finite content `f64` into a [`Decimal`].
pub(crate) fn dec_f64(v: f64) -> Decimal {
    Decimal::try_from_f64(v).unwrap_or(Decimal::ZERO)
}

/// Converts a `u32` into a [`Decimal`].
pub(crate) fn dec_u32(v: u32) -> Decimal {
    Decimal::from_u64(v as u64)
}

/// Cost of one generator at the given owned count: `base_cost * ratio^owned`.
///
/// Uses integer-exponent `powi` (repeated squaring) so cost is O(log owned).
pub(crate) fn generator_unit_cost(id: GeneratorId, owned: u32) -> Decimal {
    let def = tables::generator_def(id);
    dec_f64(def.base_cost).mul(dec_f64(def.cost_ratio).powi(owned))
}

/// Total cost of buying `n` successive generators starting at `owned`.
///
/// The sum of the individual one-unit costs, so a Ten-buy equals exactly
/// ten single buys.
pub(crate) fn generator_cumulative_cost(id: GeneratorId, owned: u32, n: u32) -> Decimal {
    let mut total = Decimal::ZERO;
    let mut k = 0u32;
    while k < n {
        total = total.add(generator_unit_cost(id, owned.saturating_add(k)));
        k += 1;
    }
    total
}

/// Highest generator tier unlocked for auto-buy by owned Auto-Buy hulls.
fn auto_buy_tier_limit(hulls: &std::collections::BTreeSet<HullId>) -> usize {
    if hulls.contains(&HullId::AutoBuyIII) {
        4
    } else if hulls.contains(&HullId::AutoBuyII) {
        3
    } else if hulls.contains(&HullId::AutoBuyI) {
        2
    } else {
        0
    }
}

/// Largest `n` such that the cumulative cost is affordable with `echo`.
pub(crate) fn max_affordable(id: GeneratorId, owned: u32, echo: Decimal) -> u32 {
    let mut hi = 1u32;
    while hi < MAX_BUY_SEARCH && generator_cumulative_cost(id, owned, hi) <= echo {
        hi = hi.saturating_mul(2);
    }
    if hi > MAX_BUY_SEARCH {
        hi = MAX_BUY_SEARCH;
    }
    let mut lo = 0u32;
    let mut high = hi;
    while lo < high {
        let mid = lo + (high - lo).div_ceil(2);
        if generator_cumulative_cost(id, owned, mid) <= echo {
            lo = mid;
        } else {
            high = mid - 1;
        }
    }
    lo
}

/// Product of all "all production" upgrade multipliers currently owned.
#[cfg(test)]
pub(crate) fn global_upgrade_mult(state: &GameState) -> Decimal {
    let mut mult = Decimal::ONE;
    for id in UpgradeId::ALL {
        if !state.run.upgrades.contains(id) {
            continue;
        }
        if let UpgradeEffect::All(factor) = tables::upgrade_def(*id).effect {
            mult = mult.mul(dec_f64(factor));
        }
    }
    mult
}

/// Tier multiplier for one generator from owned tier upgrades.
pub(crate) fn generator_tier_mult(state: &GameState, id: GeneratorId) -> Decimal {
    let mut mult = Decimal::ONE;
    for upgrade in UpgradeId::ALL {
        if !state.run.upgrades.contains(upgrade) {
            continue;
        }
        if let UpgradeEffect::Tier(target, factor) = tables::upgrade_def(*upgrade).effect
            && target == id
        {
            mult = mult.mul(dec_f64(factor));
        }
    }
    mult
}

/// Product of owned hull global multipliers.
pub(crate) fn hull_global_mult(state: &GameState) -> Decimal {
    let mut mult = Decimal::ONE;
    for id in HullId::ALL {
        if !state.meta.hulls.contains(id) {
            continue;
        }
        if let Some(factor) = tables::hull_def(*id).global_mult {
            mult = mult.mul(dec_f64(factor));
        }
    }
    mult
}

/// Pulse yield multiplier from owned hulls (Coiled Hull ×2).
pub(crate) fn hull_pulse_mult(state: &GameState) -> Decimal {
    let mut mult = Decimal::ONE;
    for id in HullId::ALL {
        if !state.meta.hulls.contains(id) {
            continue;
        }
        if let Some(factor) = tables::hull_def(*id).pulse_mult {
            mult = mult.mul(dec_f64(factor));
        }
    }
    mult
}

/// Era Fathom passive: `1 + 0.05 × era_fathoms`.
pub(crate) fn era_mult(state: &GameState) -> Decimal {
    dec_f64(1.0 + tables::ERA_FATHOM_BONUS * state.meta.era_fathoms as f64)
}

/// Achievement passive: `1 + 0.01 × achievement_count`.
pub(crate) fn achievement_mult(state: &GameState) -> Decimal {
    dec_f64(1.0 + tables::ACHIEVEMENT_BONUS * state.meta.achievements.len() as f64)
}

/// Carrier multiplier: `1.5` when the tuned band matches, else `1`.
pub(crate) fn carrier_mult(state: &GameState) -> Decimal {
    if state.run.tuned_band == state.run.carrier_band {
        dec_f64(tables::CARRIER_MATCH_MULT)
    } else {
        Decimal::ONE
    }
}

/// Global multiplier: carrier × hull globals × era × achievements.
pub(crate) fn global_multiplier(state: &GameState) -> Decimal {
    carrier_mult(state)
        .mul(hull_global_mult(state))
        .mul(era_mult(state))
        .mul(achievement_mult(state))
}

/// Current Echo production rate per second.
///
/// Single shared implementation: the tick uses this through its dirty cache,
/// and [`crate::view::StateView::rate`] calls it directly.
pub(crate) fn production_rate(state: &GameState) -> Decimal {
    // Fold all upgrade effects in one pass.
    let mut global_upgrades = Decimal::ONE;
    let mut tiers = [Decimal::ONE; 4];
    for upgrade in UpgradeId::ALL {
        if !state.run.upgrades.contains(upgrade) {
            continue;
        }
        match tables::upgrade_def(*upgrade).effect {
            UpgradeEffect::All(factor) => global_upgrades = global_upgrades.mul(dec_f64(factor)),
            UpgradeEffect::Tier(target, factor) => {
                tiers[target.index()] = tiers[target.index()].mul(dec_f64(factor));
            }
        }
    }

    let mut base = dec_f64(tables::STATION_DRAW_PER_SEC);
    for id in GeneratorId::ALL {
        let owned = state.run.generators[id.index()];
        if owned == 0 {
            continue;
        }
        let def = tables::generator_def(*id);
        let term = dec_f64(def.base_rate)
            .mul(dec_u32(owned))
            .mul(tiers[id.index()]);
        base = base.add(term);
    }

    base.mul(global_upgrades).mul(global_multiplier(state))
}

/// Pulse yield given a precomputed production rate.
pub(crate) fn pulse_yield_from_rate(rate: Decimal, state: &GameState) -> Decimal {
    let floor = dec_f64(tables::PULSE_FLOOR);
    let mut amount = rate.mul(dec_f64(tables::PULSE_SECONDS));
    if amount < floor {
        amount = floor;
    }
    amount.mul(hull_pulse_mult(state))
}

/// Echo granted by one full-capacitor pulse.
pub(crate) fn pulse_yield(state: &GameState) -> Decimal {
    pulse_yield_from_rate(production_rate(state), state)
}

fn tutorial_step_satisfied(state: &GameState, step: u8) -> bool {
    let Some(def) = STEPS.get(step as usize) else {
        return false;
    };
    let run = &state.run;
    let meta = &state.meta;
    let owned = |id: GeneratorId| run.generators[id.index()];
    match def.id {
        TutorialStepId::Pulse => meta.stats.pulses_manual >= 1,
        TutorialStepId::FirstGenerator => owned(GeneratorId::Hydrophone) >= 1,
        TutorialStepId::SecondGenerator => owned(GeneratorId::Hydrophone) >= 2,
        TutorialStepId::Upgrade => run.upgrades.contains(&UpgradeId::ImpedanceMatch),
        TutorialStepId::Tune => {
            meta.tutorial.seen_carrier_change
                && meta.tutorial.tuned_since_change
                && run.tuned_band == run.carrier_band
        }
        TutorialStepId::Drifter => owned(GeneratorId::Drifter) >= 1,
        TutorialStepId::Peak => {
            !run.peak_echo.is_zero() && run.peak_echo.exponent10() >= tables::DESCENT_THRESHOLD_EXP
        }
        TutorialStepId::Descend => meta.stats.descents >= 1,
        TutorialStepId::Hull => meta.hulls.contains(&HullId::AutoPulse),
        TutorialStepId::SecondDescent => meta.stats.descents >= 2,
        TutorialStepId::System => meta.tutorial.system_opened,
        TutorialStepId::Manual => meta.tutorial.manual_opened && meta.tutorial.manual_closed,
        TutorialStepId::Offline => meta.tutorial.acknowledged,
    }
}

fn tutorial_progress(state: &GameState, step: u8) -> Option<TutorialProgress> {
    let def = STEPS.get(step as usize)?;
    let run = &state.run;
    let meta = &state.meta;
    let hydro = dec_u32(run.generators[GeneratorId::Hydrophone.index()]);
    let (label, current, target) = match def.id {
        TutorialStepId::FirstGenerator => ("HYDROPHONES", hydro, dec_u32(1)),
        TutorialStepId::SecondGenerator => ("HYDROPHONES", hydro, dec_u32(2)),
        TutorialStepId::Upgrade => ("ECHO", run.echo, dec_u32(100)),
        TutorialStepId::Drifter => ("HYDROPHONES", hydro, dec_u32(10)),
        TutorialStepId::Peak => ("PEAK ECHO", run.peak_echo, dec_f64(1_000_000.0)),
        TutorialStepId::Descend => ("DESCENTS", dec_u32(meta.stats.descents), dec_u32(1)),
        TutorialStepId::SecondDescent => ("DESCENTS", dec_u32(meta.stats.descents), dec_u32(2)),
        _ => return None,
    };
    Some(TutorialProgress {
        label,
        current,
        target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ids::{AchievementId, GeneratorId, HullId, LogId, UpgradeId};
    use crate::content::tables;

    fn f(d: Decimal) -> f64 {
        if d.is_zero() {
            0.0
        } else {
            d.mantissa() * 10.0_f64.powi(d.exponent10() as i32)
        }
    }

    fn approx(got: Decimal, want: f64) {
        let g = f(got);
        let tol = 1e-9 * want.abs().max(1.0);
        assert!((g - want).abs() <= tol, "got {g}, want {want}");
    }

    fn give_echo(engine: &mut Engine, v: u64) {
        engine.state.run.echo = Decimal::from_u64(v);
    }

    fn give_fathoms(engine: &mut Engine, v: u32) {
        engine.state.meta.fathoms = v;
    }

    #[test]
    fn test_new_engine_basics() {
        let engine = Engine::new();
        let view = engine.view();
        assert!(view.echo().is_zero());
        assert!(view.peak().is_zero());
        assert_eq!(view.fathoms(), 0);
        assert_eq!(view.depth_m(), 7912);
        assert!(view.can_pulse());
        assert!(view.carrier_matched());
        assert_eq!(view.pulse_capacity(), 250);
        approx(view.rate(), 0.15); // 0.1 draw × carrier 1.5
        assert_eq!(engine.state_hash().len(), 64);
    }

    #[test]
    fn test_pulse_full_only_floor_and_refill() {
        let mut engine = Engine::new();
        assert_eq!(engine.apply(Action::Pulse), Ok(()));
        // Rate is 0.15/s, so 10 s is 1.5 -> floor of 10.
        approx(engine.view().echo(), 10.0);
        approx(engine.view().peak(), 10.0);
        assert_eq!(engine.state.run.charge_ticks, 0);
        assert_eq!(
            engine.apply(Action::Pulse),
            Err(ActionError::PulseNotCharged)
        );
        assert!(
            engine
                .state
                .meta
                .achievements
                .contains(&AchievementId::FirstContact)
        );
        assert!(engine.state.meta.logs.contains(&LogId::FirstPulse));
        let events: Vec<Event> = engine.drain_events().collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::AchievementUnlocked(AchievementId::FirstContact)))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::LogRecorded(LogId::FirstPulse)))
        );
    }

    #[test]
    fn test_pulse_yield_formula() {
        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 20;
        // Force a carrier mismatch so the global multiplier is exactly 1.
        engine.state.run.tuned_band = 1;
        engine.state.run.carrier_band = 0;
        // Before any achievements unlock, the rate is 0.1 + 20*0.5 = 10.1.
        approx(engine.view().rate(), 10.1);
        // Echo starts zero, so the pulse grants max(10, 10.1*10) = 101.
        assert_eq!(engine.apply(Action::Pulse), Ok(()));
        approx(engine.view().echo(), 101.0);
    }

    #[test]
    fn test_pulse_coiled_hull_doubles() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::CoiledHull);
        engine.state.run.tuned_band = 1;
        engine.state.run.carrier_band = 0;
        assert_eq!(engine.apply(Action::Pulse), Ok(()));
        approx(engine.view().echo(), 20.0); // floor 10 × 2
    }

    #[test]
    fn test_generator_unit_cost_values() {
        let hydro = GeneratorId::Hydrophone;
        approx(generator_unit_cost(hydro, 0), 10.0);
        approx(generator_unit_cost(hydro, 1), 11.5);
        approx(generator_unit_cost(hydro, 2), 13.225);
        let core = GeneratorId::TidalCore;
        approx(generator_unit_cost(core, 0), 2_500_000.0);
        approx(generator_unit_cost(core, 1), 3_050_000.0);
    }

    #[test]
    fn test_buy_ten_equals_sum_of_singles() {
        for id in GeneratorId::ALL {
            let mut manual = Decimal::ZERO;
            for k in 0..10u32 {
                manual = manual.add(generator_unit_cost(*id, k));
            }
            let cumulative = generator_cumulative_cost(*id, 0, 10);
            assert_eq!(manual, cumulative, "ten-buy mismatch for {id}");
        }
    }

    #[test]
    fn test_buy_generator_basic_and_funds() {
        let mut engine = Engine::new();
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            }),
            Err(ActionError::InsufficientFunds)
        );
        give_echo(&mut engine, 1000);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            }),
            Ok(())
        );
        assert_eq!(engine.view().generator_owned(GeneratorId::Hydrophone), 1);
        approx(engine.view().echo(), 990.0);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            }),
            Ok(())
        );
        approx(engine.view().echo(), 978.5);
        assert_eq!(engine.state.meta.stats.purchases, 2);
        assert!(
            engine
                .state
                .meta
                .achievements
                .contains(&AchievementId::Hydrophile)
        );
        assert!(engine.state.meta.logs.contains(&LogId::FirstHydro));
    }

    #[test]
    fn test_locked_generators() {
        let mut engine = Engine::new();
        for id in [
            GeneratorId::Drifter,
            GeneratorId::SonarMast,
            GeneratorId::TidalCore,
        ] {
            assert_eq!(
                engine.apply(Action::BuyGenerator {
                    id,
                    quantity: BuyQuantity::One,
                }),
                Err(ActionError::Locked),
                "{id} should be locked"
            );
        }
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 10;
        give_echo(&mut engine, 1000);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Drifter,
                quantity: BuyQuantity::One,
            }),
            Ok(())
        );
    }

    #[test]
    fn test_max_buy_invariant_and_apply() {
        let id = GeneratorId::Hydrophone;
        for magnitude in [1000u64, 1_000_000, 1_000_000_000] {
            let echo = Decimal::from_u64(magnitude);
            let n = max_affordable(id, 0, echo);
            assert!(n > 0);
            let cost_n = generator_cumulative_cost(id, 0, n);
            let cost_next = generator_cumulative_cost(id, 0, n + 1);
            assert!(cost_n <= echo, "cost(n) must be affordable at {magnitude}");
            assert!(
                cost_next > echo,
                "cost(n+1) must not be affordable at {magnitude}"
            );
        }

        let mut engine = Engine::new();
        give_echo(&mut engine, 1000);
        let before = engine.state.run.echo;
        let n = max_affordable(id, 0, before);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id,
                quantity: BuyQuantity::Max,
            }),
            Ok(())
        );
        assert_eq!(engine.view().generator_owned(id), n);
        let spent = generator_cumulative_cost(id, 0, n);
        assert_eq!(engine.state.run.echo, before.sub(spent));
        let events: Vec<Event> = engine.drain_events().collect();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PurchasedGenerator { count, .. } if *count == n
        )));
    }

    #[test]
    fn test_max_buy_zero_is_error() {
        let mut engine = Engine::new();
        give_echo(&mut engine, 0);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::Max,
            }),
            Err(ActionError::InsufficientFunds)
        );
    }

    #[test]
    fn test_buy_upgrade_rules() {
        let mut engine = Engine::new();
        // Impedance Match needs a Drifter.
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch)),
            Err(ActionError::Locked)
        );
        // Full Spectrum needs 25 Masts.
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::FullSpectrum)),
            Err(ActionError::Locked)
        );

        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 10;
        engine.state.run.generators[GeneratorId::Drifter.index()] = 1;
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch)),
            Err(ActionError::InsufficientFunds)
        );
        give_echo(&mut engine, 1000);
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch)),
            Ok(())
        );
        assert!(engine.view().upgrade_owned(UpgradeId::ImpedanceMatch));
        approx(engine.view().echo(), 900.0);
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch)),
            Err(ActionError::AlreadyOwned)
        );
        // Sweep Profile is unlocked (10 hydro) and costs 750.
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::SweepProfile)),
            Ok(())
        );
        assert!(engine.view().upgrade_owned(UpgradeId::SweepProfile));
    }

    #[test]
    fn test_buy_hull_rules() {
        let mut engine = Engine::new();
        assert_eq!(
            engine.apply(Action::BuyHull(HullId::AutoPulse)),
            Err(ActionError::InsufficientFunds)
        );
        give_fathoms(&mut engine, 1);
        assert_eq!(engine.apply(Action::BuyHull(HullId::AutoPulse)), Ok(()));
        assert!(engine.view().hull_owned(HullId::AutoPulse));
        assert_eq!(engine.view().fathoms(), 0);
        assert_eq!(
            engine.apply(Action::BuyHull(HullId::AutoPulse)),
            Err(ActionError::AlreadyOwned)
        );
        // Coiled Hull requires Deep Memory.
        assert_eq!(
            engine.apply(Action::BuyHull(HullId::CoiledHull)),
            Err(ActionError::PrereqMissing)
        );
        give_fathoms(&mut engine, 1000);
        assert_eq!(engine.apply(Action::BuyHull(HullId::DeepMemory)), Ok(()));
        assert_eq!(engine.apply(Action::BuyHull(HullId::CoiledHull)), Ok(()));
        assert_eq!(engine.view().fathoms(), 1000 - 8 - 12);
    }

    #[test]
    fn test_auto_toggles_and_governor_bounds() {
        let mut engine = Engine::new();
        assert_eq!(
            engine.apply(Action::SetAutoBuy {
                id: GeneratorId::Hydrophone,
                enabled: false,
            }),
            Ok(())
        );
        assert!(!engine.state.meta.auto_buy[GeneratorId::Hydrophone.index()]);

        assert_eq!(
            engine.apply(Action::SetAutoDescendTarget { fathoms: 0 }),
            Ok(())
        );
        assert_eq!(
            engine.apply(Action::SetAutoDescendTarget { fathoms: 3 }),
            Ok(())
        );
        assert_eq!(
            engine.apply(Action::SetAutoDescendTarget { fathoms: 1_000_000 }),
            Ok(())
        );
        for bad in [1u32, 2, 1_000_001] {
            assert_eq!(
                engine.apply(Action::SetAutoDescendTarget { fathoms: bad }),
                Err(ActionError::InvalidTarget),
                "target {bad} should be rejected"
            );
        }
        assert_eq!(engine.state.meta.auto_descend_at, 1_000_000);
    }

    #[test]
    fn test_tuning_match_accounting() {
        let mut engine = Engine::new();
        // Fresh: tuned == carrier == 0.
        assert_eq!(engine.apply(Action::SetTuning { band: 0 }), Ok(()));
        assert_eq!(engine.state.meta.stats.bands_matched, 1);
        assert!(engine.state.meta.bands_seen[0]);

        assert_eq!(engine.apply(Action::SetTuning { band: 1 }), Ok(()));
        assert_eq!(engine.state.meta.stats.bands_matched, 1);
        assert!(!engine.state.meta.bands_seen[1]);

        // A carrier change alone must not count a match.
        engine.state.run.carrier_band = 1;
        assert_eq!(engine.state.meta.stats.bands_matched, 1);
        assert!(!engine.state.meta.bands_seen[1]);

        // Tuning to the new carrier counts.
        assert_eq!(engine.apply(Action::SetTuning { band: 1 }), Ok(()));
        assert_eq!(engine.state.meta.stats.bands_matched, 2);
        assert!(engine.state.meta.bands_seen[1]);

        assert_eq!(
            engine.apply(Action::SetTuning { band: 6 }),
            Err(ActionError::InvalidBand)
        );
    }

    #[test]
    fn test_descent_unavailable() {
        let mut engine = Engine::new();
        assert_eq!(
            engine.apply(Action::Descend),
            Err(ActionError::DescentUnavailable)
        );
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 5).unwrap();
        assert_eq!(
            engine.apply(Action::Descend),
            Err(ActionError::DescentUnavailable)
        );
    }

    #[test]
    fn test_descent_gain_and_stats() {
        let mut engine = Engine::new();
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        assert_eq!(engine.apply(Action::Descend), Ok(()));
        assert_eq!(engine.state.meta.fathoms, 3);
        assert_eq!(engine.state.meta.era_fathoms, 3);
        assert_eq!(engine.state.meta.lifetime_fathoms, 3);
        assert_eq!(engine.state.meta.stats.descents, 1);
        assert_eq!(engine.state.meta.depth_m, 8412);
        let events: Vec<Event> = engine.drain_events().collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Descended { gain: 3 }))
        );
    }

    #[test]
    fn test_descent_gain_with_tidal_and_fractal() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::TidalLoop);
        engine.state.meta.hulls.insert(HullId::FractalHull);
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        assert_eq!(engine.apply(Action::Descend), Ok(()));
        assert_eq!(engine.state.meta.fathoms, 9); // 3 × 1.5 × 2
    }

    #[test]
    fn test_descent_reset_scope() {
        let mut engine = Engine::new();
        engine.state.meta.fathoms = 10;
        engine.state.meta.era_fathoms = 7;
        engine.state.meta.lifetime_fathoms = 50;
        engine.state.meta.depth_m = 9000;
        engine.state.meta.hulls.insert(HullId::BallastRecall);
        engine.state.meta.hulls.insert(HullId::DeepMemory);
        engine.state.meta.bands_seen = [true, false, false, false, false, false];
        engine.state.meta.stats.bands_matched = 4;

        engine.state.run.echo = Decimal::from_u64(5);
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.state.run.generators = [7, 2, 1, 0];
        engine.state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        engine.state.run.charge_ticks = 100;
        engine.state.run.carrier_band = 3;
        engine.state.run.tuned_band = 5;
        engine.state.run.carrier_ticks = 123;
        engine.state.run.run_ticks = 456;

        assert_eq!(engine.apply(Action::Descend), Ok(()));

        // Persisted.
        assert_eq!(engine.state.meta.fathoms, 13);
        assert_eq!(engine.state.meta.era_fathoms, 10);
        assert_eq!(engine.state.meta.lifetime_fathoms, 53);
        assert_eq!(engine.state.meta.depth_m, 9500);
        assert!(engine.state.meta.hulls.contains(&HullId::BallastRecall));
        assert!(engine.state.meta.hulls.contains(&HullId::DeepMemory));
        assert_eq!(
            engine.state.meta.bands_seen,
            [true, false, false, false, false, false]
        );
        assert_eq!(engine.state.meta.stats.bands_matched, 4);
        assert_eq!(engine.state.meta.stats.descents, 1);
        assert_eq!(engine.state.meta.auto_buy, [true; 4]);

        // Reset.
        assert!(engine.state.run.echo.is_zero());
        assert!(engine.state.run.peak_echo.is_zero());
        assert_eq!(engine.state.run.generators, [5, 0, 0, 0]);
        assert!(engine.state.run.upgrades.is_empty());
        assert_eq!(engine.state.run.charge_ticks, tables::PULSE_CAPACITOR_TICKS);
        assert_eq!(engine.state.run.carrier_band, 0);
        assert_eq!(engine.state.run.tuned_band, 0);
        assert_eq!(engine.state.run.carrier_ticks, 0);
        assert_eq!(engine.state.run.run_ticks, 0);
    }

    #[test]
    fn test_descent_without_ballast_recall_zeroes_generators() {
        let mut engine = Engine::new();
        engine.state.run.generators = [9, 3, 0, 0];
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        assert_eq!(engine.apply(Action::Descend), Ok(()));
        assert_eq!(engine.state.run.generators, [0; 4]);
    }

    #[test]
    fn test_purchases_increment_stat() {
        let mut engine = Engine::new();
        give_echo(&mut engine, 10_000);
        give_fathoms(&mut engine, 10);
        assert_eq!(
            engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            }),
            Ok(())
        );
        engine.state.run.generators[GeneratorId::Drifter.index()] = 1;
        assert_eq!(
            engine.apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch)),
            Ok(())
        );
        assert_eq!(engine.apply(Action::BuyHull(HullId::AutoPulse)), Ok(()));
        assert_eq!(engine.state.meta.stats.purchases, 3);
    }

    #[test]
    fn test_global_and_tier_multipliers() {
        let mut engine = Engine::new();
        engine.state.meta.era_fathoms = 4;
        engine
            .state
            .meta
            .achievements
            .insert(AchievementId::FirstContact);
        engine
            .state
            .meta
            .achievements
            .insert(AchievementId::Hydrophile);
        engine.state.meta.hulls.insert(HullId::DeepMemory);
        engine.state.meta.hulls.insert(HullId::NullArray);
        engine.state.run.tuned_band = 0;
        engine.state.run.carrier_band = 0;

        let expected = dec_f64(tables::CARRIER_MATCH_MULT)
            .mul(dec_f64(1.25).mul(dec_f64(2.0)))
            .mul(dec_f64(1.0 + tables::ERA_FATHOM_BONUS * 4.0))
            .mul(dec_f64(1.0 + tables::ACHIEVEMENT_BONUS * 2.0));
        assert_eq!(engine.view().global_multiplier(), expected);

        // Tier multipliers come from tier upgrades only.
        assert_eq!(
            generator_tier_mult(&engine.state, GeneratorId::Hydrophone),
            Decimal::ONE
        );
        engine.state.run.upgrades.insert(UpgradeId::SweepProfile);
        engine.state.run.upgrades.insert(UpgradeId::MastPhasing);
        assert_eq!(
            generator_tier_mult(&engine.state, GeneratorId::Hydrophone),
            dec_f64(3.0)
        );
        assert_eq!(
            generator_tier_mult(&engine.state, GeneratorId::SonarMast),
            dec_f64(4.0)
        );
        // All-production upgrades compose globally.
        assert_eq!(global_upgrade_mult(&engine.state), Decimal::ONE);
        engine.state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        engine.state.run.upgrades.insert(UpgradeId::FullSpectrum);
        assert_eq!(global_upgrade_mult(&engine.state), dec_f64(6.0));
    }

    #[test]
    fn test_event_buffer_bounded() {
        let mut engine = Engine::new();
        for _ in 0..1000 {
            assert_eq!(engine.apply(Action::SetTuning { band: 0 }), Ok(()));
        }
        let count = engine.drain_events().count();
        assert_eq!(count, EVENT_CAPACITY);
    }

    #[test]
    fn test_state_hash_deterministic_and_sensitive() {
        let mut a = Engine::new();
        let mut b = Engine::new();
        assert_eq!(a.state_hash(), b.state_hash());

        for engine in [&mut a, &mut b] {
            assert_eq!(engine.apply(Action::Pulse), Ok(()));
            assert_eq!(engine.apply(Action::SetTuning { band: 2 }), Ok(()));
        }
        assert_eq!(a.state_hash(), b.state_hash());

        // A divergent state must change the hash.
        assert_eq!(b.apply(Action::Pulse), Err(ActionError::PulseNotCharged));
        give_echo(&mut b, 1_000_000);
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn test_view_unlock_and_affordability() {
        let mut engine = Engine::new();
        assert!(engine.view().generator_unlocked(GeneratorId::Hydrophone));
        assert!(!engine.view().generator_unlocked(GeneratorId::Drifter));
        assert!(
            !engine
                .view()
                .generator_affordable(GeneratorId::Hydrophone, BuyQuantity::One)
        );
        give_echo(&mut engine, 100);
        assert!(
            engine
                .view()
                .generator_affordable(GeneratorId::Hydrophone, BuyQuantity::One)
        );
        assert!(!engine.view().upgrade_affordable(UpgradeId::ImpedanceMatch));
        assert!(!engine.view().hull_affordable(HullId::AutoPulse));
        give_fathoms(&mut engine, 1);
        assert!(engine.view().hull_affordable(HullId::AutoPulse));
    }

    #[test]
    fn test_view_descent_thresholds() {
        let mut engine = Engine::new();
        assert!(!engine.view().can_descend());
        assert_eq!(engine.view().descent_gain(), 0);
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        assert!(engine.view().can_descend());
        assert_eq!(engine.view().descent_gain(), 3);
        assert_eq!(
            engine.view().next_bucket_threshold(),
            Decimal::from_parts(tables::DESCENT_BUCKET0, 6).unwrap()
        );
        engine.state.run.peak_echo = Decimal::from_parts(3.0, 6).unwrap();
        assert_eq!(
            engine.view().next_bucket_threshold(),
            Decimal::from_parts(tables::DESCENT_BUCKET1, 6).unwrap()
        );
        engine.state.run.peak_echo = Decimal::from_parts(5.0, 6).unwrap();
        assert_eq!(
            engine.view().next_bucket_threshold(),
            Decimal::from_parts(1.0, 7).unwrap()
        );
    }

    #[test]
    fn test_peak_tracking_on_pulse() {
        let mut engine = Engine::new();
        assert_eq!(engine.apply(Action::Pulse), Ok(()));
        let peak = engine.view().peak();
        assert_eq!(peak, engine.view().echo());
        assert_eq!(engine.state.meta.stats.best_peak, peak);
    }

    // -----------------------------------------------------------------
    // Phase 3: tick law, automation, determinism
    // -----------------------------------------------------------------

    fn insert_all_hulls(engine: &mut Engine) {
        for id in HullId::ALL {
            engine.state.meta.hulls.insert(*id);
        }
    }

    fn automated_engine() -> Engine {
        let mut engine = Engine::new();
        insert_all_hulls(&mut engine);
        engine.state.meta.auto_descend_at = 10;
        engine.state.meta.auto_buy = [true; 4];
        engine
    }

    fn scripted_engine() -> Engine {
        let mut engine = Engine::new();
        for t in 0..36_000u64 {
            if t % 250 == 0 {
                let _ = engine.apply(Action::Pulse);
            }
            if t % 400 == 0 {
                let _ = engine.apply(Action::SetTuning {
                    band: ((t / 400) % 6) as u8,
                });
            }
            if t % 10 == 0 {
                let _ = engine.apply(Action::BuyGenerator {
                    id: GeneratorId::Hydrophone,
                    quantity: BuyQuantity::Max,
                });
            }
            if t % 20 == 5 {
                let _ = engine.apply(Action::BuyGenerator {
                    id: GeneratorId::Drifter,
                    quantity: BuyQuantity::One,
                });
            }
            if t % 50 == 0 {
                let _ = engine.apply(Action::BuyGenerator {
                    id: GeneratorId::SonarMast,
                    quantity: BuyQuantity::One,
                });
            }
            if t % 100 == 0 {
                let _ = engine.apply(Action::BuyUpgrade(UpgradeId::ALL[((t / 100) % 8) as usize]));
            }
            if engine.view().can_descend() && engine.state.meta.stats.descents < 2 {
                let _ = engine.apply(Action::Descend);
            }
            engine.tick();
        }
        engine
    }

    fn count_achievement_events(engine: &mut Engine, id: AchievementId) -> usize {
        let events: Vec<Event> = engine.drain_events().collect();
        events
            .iter()
            .filter(|e| matches!(e, Event::AchievementUnlocked(a) if *a == id))
            .count()
    }

    fn count_log_events(engine: &mut Engine, id: LogId) -> usize {
        let events: Vec<Event> = engine.drain_events().collect();
        events
            .iter()
            .filter(|e| matches!(e, Event::LogRecorded(l) if *l == id))
            .count()
    }

    fn assert_achievement_once(setup: impl Fn(&mut Engine), id: AchievementId) {
        let mut engine = Engine::new();
        setup(&mut engine);
        engine.tick();
        assert!(
            engine.state.meta.achievements.contains(&id),
            "achievement not unlocked: {id}"
        );
        assert_eq!(
            count_achievement_events(&mut engine, id),
            1,
            "unlock event count for {id}"
        );
        engine.tick();
        assert_eq!(
            count_achievement_events(&mut engine, id),
            0,
            "re-fired {id}"
        );
    }

    fn assert_log_once(setup: impl Fn(&mut Engine), id: LogId) {
        let mut engine = Engine::new();
        setup(&mut engine);
        engine.tick();
        assert!(engine.state.meta.logs.contains(&id), "log missing: {id}");
        assert_eq!(count_log_events(&mut engine, id), 1, "log event for {id}");
        engine.tick();
        assert_eq!(count_log_events(&mut engine, id), 0, "re-fired log {id}");
    }

    #[test]
    fn test_carrier_cycle_timing() {
        for (ticks, band) in [(899u64, 0u8), (900, 1), (901, 1), (5400, 0)] {
            let mut engine = Engine::new();
            engine.advance(ticks);
            assert_eq!(engine.state.run.carrier_band, band, "after {ticks} ticks");
        }

        let mut engine = Engine::new();
        engine.advance(5400);
        let events: Vec<Event> = engine.drain_events().collect();
        let changes = events
            .iter()
            .filter(|e| matches!(e, Event::CarrierChanged { .. }))
            .count();
        assert_eq!(changes, 6);
    }

    #[test]
    fn test_tuned_ballast_auto_tunes_and_counts() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::TunedBallast);
        engine.advance(900);
        assert_eq!(engine.state.run.carrier_band, 1);
        assert_eq!(engine.state.run.tuned_band, 1);
        assert!(engine.state.meta.bands_seen[1]);
        assert_eq!(engine.state.meta.stats.bands_matched, 1);

        engine.advance(900);
        assert_eq!(engine.state.run.carrier_band, 2);
        assert_eq!(engine.state.run.tuned_band, 2);
        assert_eq!(engine.state.meta.stats.bands_matched, 2);
    }

    #[test]
    fn test_capacitor_refill_and_manual_threshold() {
        let mut engine = Engine::new();
        engine.state.run.charge_ticks = 0;
        engine.advance(100);
        assert_eq!(engine.state.run.charge_ticks, 100);
        engine.advance(150);
        assert_eq!(engine.state.run.charge_ticks, 250);

        engine.state.run.charge_ticks = 249;
        assert_eq!(
            engine.apply(Action::Pulse),
            Err(ActionError::PulseNotCharged)
        );
        engine.state.run.charge_ticks = 250;
        assert_eq!(engine.apply(Action::Pulse), Ok(()));
    }

    #[test]
    fn test_auto_pulse_fires_once_per_charge() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoPulse);
        let rate = production_rate(&engine.state);
        let per_tick = rate.div(dec_u32(crate::TICKS_PER_SECOND as u32)).unwrap();
        let expected = pulse_yield_from_rate(rate, &engine.state);

        engine.tick();
        assert_eq!(engine.state.meta.stats.pulses_auto, 1);
        assert_eq!(engine.state.run.charge_ticks, 0);
        assert_eq!(engine.state.run.echo, per_tick.add(expected));
        // The auto-pulse landed before the peak check in step 7.
        assert_eq!(engine.state.run.peak_echo, engine.state.run.echo);

        engine.advance(249);
        assert_eq!(engine.state.meta.stats.pulses_auto, 1);
        engine.advance(1);
        assert_eq!(engine.state.meta.stats.pulses_auto, 2);

        // No per-pulse events.
        let events: Vec<Event> = engine.drain_events().collect();
        assert!(!events.iter().any(|e| matches!(e, Event::Pulse { .. })));
    }

    #[test]
    fn test_auto_pulse_coiled_hull() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoPulse);
        engine.state.meta.hulls.insert(HullId::CoiledHull);
        let rate = production_rate(&engine.state);
        approx(pulse_yield_from_rate(rate, &engine.state), 20.0);
    }

    #[test]
    fn test_tick_purchase_before_production() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyI);
        engine.state.meta.auto_buy = [true; 4];
        engine.state.run.echo = Decimal::from_u64(10);
        engine.tick();
        // The hydrophone was bought in step 3; rate in step 4 sees it:
        // (0.1 + 0.5) * 1.5 = 0.9/s -> 0.09 this tick.
        assert_eq!(engine.view().generator_owned(GeneratorId::Hydrophone), 1);
        approx(engine.view().echo(), 0.09);
    }

    #[test]
    fn test_auto_buy_value_order_lower_rate_wins() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyII);
        engine.state.meta.auto_buy = [true; 4];
        engine.state.run.generators = [10, 6, 0, 0];
        engine.state.run.echo = Decimal::from_u64(1_000_000);
        engine.tick();
        // Hydrophone has the lowest base rate but the best value.
        assert_eq!(engine.view().generator_owned(GeneratorId::Hydrophone), 11);
        assert_eq!(engine.view().generator_owned(GeneratorId::Drifter), 6);
        assert_eq!(engine.view().generator_owned(GeneratorId::SonarMast), 0);
    }

    #[test]
    fn test_auto_buy_gating_by_hull() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyI); // only tiers 0..2
        engine.state.meta.auto_buy = [true; 4];
        engine.state.run.generators = [10, 6, 0, 0];
        engine.state.run.echo = Decimal::from_u64(1_000_000);
        engine.tick();
        assert_eq!(
            engine.view().generator_owned(GeneratorId::SonarMast),
            0,
            "Auto-Buy I must not buy masts"
        );
    }

    #[test]
    fn test_auto_buy_respects_toggle() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyI);
        engine.state.meta.auto_buy = [false, true, true, true];
        engine.state.run.generators = [10, 6, 0, 0];
        engine.state.run.echo = Decimal::from_u64(1_000_000);
        engine.tick();
        assert_eq!(engine.view().generator_owned(GeneratorId::Hydrophone), 10);
        assert_eq!(engine.view().generator_owned(GeneratorId::Drifter), 7);
    }

    #[test]
    fn test_auto_buy_no_funds_and_cadence() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyI);
        engine.state.meta.auto_buy = [true; 4];
        engine.state.run.echo = Decimal::ZERO;
        engine.tick();
        assert_eq!(engine.state.meta.stats.purchases, 0);

        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoBuyI);
        engine.state.meta.auto_buy = [true; 4];
        engine.state.run.generators = [10, 6, 0, 0];
        engine.state.run.echo = Decimal::from_u64(1_000_000_000);
        engine.advance(10);
        assert_eq!(engine.state.meta.stats.purchases, 1);
        engine.advance(1);
        assert_eq!(engine.state.meta.stats.purchases, 2);
    }

    #[test]
    fn test_chain_automator() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::ChainAutomator);
        engine.state.run.generators = [25, 6, 25, 1];
        engine.state.run.echo = Decimal::from_u64(1_000_000_000);
        engine.tick();
        assert_eq!(engine.state.run.upgrades.len(), 1);
        assert!(
            engine
                .state
                .run
                .upgrades
                .contains(&UpgradeId::ImpedanceMatch)
        );
        engine.advance(9);
        assert_eq!(engine.state.run.upgrades.len(), 1);
        engine.advance(1);
        assert_eq!(engine.state.run.upgrades.len(), 2);
        assert!(engine.state.run.upgrades.contains(&UpgradeId::SweepProfile));
    }

    #[test]
    fn test_chain_automator_none_affordable_or_locked() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::ChainAutomator);
        engine.state.run.echo = Decimal::ZERO;
        engine.tick();
        assert!(engine.state.run.upgrades.is_empty());

        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::ChainAutomator);
        engine.state.run.generators = [1, 0, 0, 0];
        engine.state.run.echo = Decimal::from_u64(1_000_000_000);
        engine.tick();
        // Impedance needs a Drifter; Sweep needs 10 Hydrophones.
        assert!(engine.state.run.upgrades.is_empty());
    }

    #[test]
    fn test_every_achievement_fires_exactly_once() {
        assert_achievement_once(
            |e| e.state.meta.stats.pulses_manual = 1,
            AchievementId::FirstContact,
        );
        assert_achievement_once(
            |e| e.state.run.generators[GeneratorId::Hydrophone.index()] = 1,
            AchievementId::Hydrophile,
        );
        assert_achievement_once(
            |e| e.state.run.generators[GeneratorId::Hydrophone.index()] = 50,
            AchievementId::Chorus,
        );
        assert_achievement_once(
            |e| e.state.run.generators[GeneratorId::Drifter.index()] = 1,
            AchievementId::Drifter,
        );
        assert_achievement_once(
            |e| e.state.run.generators[GeneratorId::SonarMast.index()] = 1,
            AchievementId::Mast,
        );
        assert_achievement_once(
            |e| e.state.run.generators[GeneratorId::TidalCore.index()] = 1,
            AchievementId::Core,
        );
        assert_achievement_once(
            |e| e.state.meta.stats.descents = 1,
            AchievementId::FirstDescent,
        );
        assert_achievement_once(|e| e.state.meta.stats.descents = 6, AchievementId::SixDeep);
        assert_achievement_once(
            |e| e.state.meta.lifetime_fathoms = 100,
            AchievementId::Century,
        );
        assert_achievement_once(
            |e| e.state.meta.bands_seen = [true; 6],
            AchievementId::AllBands,
        );
        assert_achievement_once(
            |e| e.state.meta.stats.best_peak = Decimal::from_parts(1.0, 12).unwrap(),
            AchievementId::Signal,
        );
    }

    #[test]
    fn test_achievement_bonus_applies_next_tick() {
        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 1;
        let rate_before = production_rate(&engine.state);
        let per_tick = rate_before
            .div(dec_u32(crate::TICKS_PER_SECOND as u32))
            .unwrap();
        engine.tick();
        // Production this tick used the pre-achievement rate.
        approx(engine.view().echo(), f(per_tick));
        // Hydrophile unlocked at the end of the tick; rate now includes +1%.
        approx(engine.view().rate(), f(rate_before) * 1.01);
    }

    #[test]
    fn test_achievements_no_refire_after_descent() {
        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 1;
        engine.tick();
        assert_eq!(
            count_achievement_events(&mut engine, AchievementId::Hydrophile),
            1
        );
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.apply(Action::Descend).unwrap();
        engine.tick();
        engine.tick();
        assert_eq!(
            count_achievement_events(&mut engine, AchievementId::Hydrophile),
            0
        );
        assert!(
            engine
                .state
                .meta
                .achievements
                .contains(&AchievementId::Hydrophile)
        );
    }

    #[test]
    fn test_every_log_triggers_exactly_once() {
        assert_log_once(|_| {}, LogId::Boot);
        assert_log_once(|e| e.state.meta.stats.pulses_manual = 1, LogId::FirstPulse);
        assert_log_once(
            |e| e.state.run.generators[GeneratorId::Hydrophone.index()] = 1,
            LogId::FirstHydro,
        );
        assert_log_once(|e| e.state.meta.stats.bands_matched = 1, LogId::CarrierLock);
        assert_log_once(|e| e.state.meta.stats.descents = 1, LogId::FirstDescent);
        assert_log_once(|e| e.state.meta.depth_m = 9000, LogId::Depth9000);
        assert_log_once(
            |e| e.state.meta.stats.best_peak = Decimal::from_parts(1.0, 9).unwrap(),
            LogId::Peak1e9,
        );
        assert_log_once(|e| e.state.meta.lifetime_fathoms = 100, LogId::Fathoms100);
        assert_log_once(|e| e.state.meta.bands_seen = [true; 6], LogId::AllBands);
        assert_log_once(|e| e.state.meta.stats.pulses_auto = 1, LogId::AutoPulse);
    }

    #[test]
    fn test_descent_available_edge_trigger() {
        let mut engine = Engine::new();
        engine.state.run.echo = Decimal::from_u64(1_000_000);
        engine.tick();
        let events: Vec<Event> = engine.drain_events().collect();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::DescentAvailable))
                .count(),
            1
        );
        assert!(engine.descent_available);

        engine.tick();
        let events: Vec<Event> = engine.drain_events().collect();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::DescentAvailable))
                .count(),
            0
        );

        // Latch resets on descent and can fire again.
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.apply(Action::Descend).unwrap();
        assert!(!engine.descent_available);
        engine.state.run.echo = Decimal::from_u64(1_000_000);
        engine.tick();
        let events: Vec<Event> = engine.drain_events().collect();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::DescentAvailable))
                .count(),
            1
        );
    }

    fn governor_engine(target: u32, peak: Option<Decimal>) -> Engine {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::BallastGovernor);
        engine.state.meta.auto_descend_at = target;
        if let Some(p) = peak {
            engine.state.run.peak_echo = p;
            engine.state.run.echo = p;
        }
        engine
    }

    #[test]
    fn test_governor_rules() {
        // Target 0 disables it.
        let mut engine = governor_engine(0, Some(Decimal::from_parts(1.0, 6).unwrap()));
        engine.tick();
        assert_eq!(engine.state.meta.stats.descents, 0);

        // Target met: descends.
        let mut engine = governor_engine(3, Some(Decimal::from_parts(1.0, 6).unwrap()));
        engine.tick();
        assert_eq!(engine.state.meta.stats.descents, 1);
        assert_eq!(engine.state.meta.auto_descend_at, 3);
        assert!(engine.state.meta.hulls.contains(&HullId::BallastGovernor));

        // Target not met: gain 3 < 4.
        let mut engine = governor_engine(4, Some(Decimal::from_parts(1.0, 6).unwrap()));
        engine.tick();
        assert_eq!(engine.state.meta.stats.descents, 0);

        // Below the Descent threshold: no gain.
        let mut engine = governor_engine(3, Some(Decimal::from_parts(9.9, 5).unwrap()));
        engine.tick();
        assert_eq!(engine.state.meta.stats.descents, 0);
    }

    #[test]
    fn test_descent_by_automation_reset_scope() {
        let mut engine = Engine::new();
        insert_all_hulls(&mut engine);
        engine.state.meta.auto_buy = [true; 4];
        engine.state.meta.auto_descend_at = 3;
        engine.state.run.generators = [7, 2, 0, 0];
        engine.state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.state.run.echo = Decimal::from_parts(1.0, 6).unwrap();
        let ticks_before = engine.state.meta.stats.total_ticks;

        engine.tick();

        assert_eq!(engine.state.meta.stats.descents, 1);
        assert_eq!(engine.state.meta.stats.total_ticks, ticks_before + 1);
        assert!(engine.state.meta.stats.purchases >= 1);
        assert!(engine.state.run.echo.is_zero());
        assert!(engine.state.run.peak_echo.is_zero());
        assert_eq!(engine.state.run.generators, [5, 0, 0, 0]);
        assert!(engine.state.run.upgrades.is_empty());
        assert_eq!(engine.state.run.charge_ticks, tables::PULSE_CAPACITOR_TICKS);
        assert_eq!(engine.state.run.carrier_band, 0);
        assert_eq!(engine.state.run.tuned_band, 0);
        assert_eq!(engine.state.run.run_ticks, 1);
        assert_eq!(engine.state.meta.auto_descend_at, 3);
    }

    #[test]
    fn test_lifetime_echo_includes_production_and_pulse() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoPulse);
        let rate = production_rate(&engine.state);
        let per_tick = rate.div(dec_u32(crate::TICKS_PER_SECOND as u32)).unwrap();
        let pulse = pulse_yield_from_rate(rate, &engine.state);
        engine.tick();
        assert_eq!(engine.state.meta.stats.lifetime_echo, per_tick.add(pulse));

        engine.state.run.charge_ticks = 250;
        let before = engine.state.meta.stats.lifetime_echo;
        let amount = pulse_yield(&engine.state);
        engine.apply(Action::Pulse).unwrap();
        assert_eq!(engine.state.meta.stats.lifetime_echo, before.add(amount));
    }

    #[test]
    fn test_advance_equals_ticks() {
        let mut a = automated_engine();
        a.advance(5000);
        let mut b = automated_engine();
        for _ in 0..5000 {
            b.tick();
        }
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn test_scripted_determinism_and_hash() {
        let a = scripted_engine();
        let b = scripted_engine();
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(
            a.state.meta.stats.descents, 2,
            "scripted run must descend twice"
        );
        assert_eq!(
            a.state_hash(),
            "1bec052fe29b7f6085a17a2f59b47679fb775c5141169b804329eb476ac84dee"
        );
    }

    #[test]
    fn test_full_automation_hash() {
        let mut a = automated_engine();
        a.advance(36_000);
        let mut b = automated_engine();
        b.advance(36_000);
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(
            a.state_hash(),
            "89bca914158ef0cf8e547cf2ea1fae01f3ac278ea19a909d84f0a6d174a4f049"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_long_run_finite() {
        let mut engine = automated_engine();
        engine.advance(200_000);
        assert!(engine.view().echo().mantissa().is_finite());
        assert!(engine.view().peak().mantissa().is_finite());
        assert!(engine.state.run.echo.exponent10() <= i64::MAX / 2);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_release_168h_under_two_seconds() {
        use std::time::{Duration, Instant};
        let mut engine = automated_engine();
        let start = Instant::now();
        engine.advance(6_048_000);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "168h simulation took {elapsed:?}"
        );
        assert!(engine.view().echo().mantissa().is_finite());
        assert!(engine.view().peak().mantissa().is_finite());
        println!("RELEASE_168H={elapsed:?}");
    }

    #[test]
    fn test_governor_sees_same_tick_production() {
        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::BallastGovernor);
        engine.state.meta.auto_descend_at = 3;
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 100;
        // Just below the threshold; production in step 5 must push it over so
        // the governor in step 9 sees the tick's own output.
        engine.state.run.echo = Decimal::from_u64(999_999);
        engine.state.run.peak_echo = Decimal::from_u64(999_999);
        engine.tick();
        assert_eq!(engine.state.meta.stats.descents, 1);
    }

    #[test]
    fn test_auto_buy_deterministic_repeat() {
        fn setup() -> Engine {
            let mut engine = Engine::new();
            engine.state.meta.hulls.insert(HullId::AutoBuyII);
            engine.state.meta.auto_buy = [true; 4];
            engine.state.run.generators = [10, 6, 0, 0];
            engine.state.run.echo = Decimal::from_u64(1_000_000);
            engine
        }
        let mut a = setup();
        let mut b = setup();
        a.tick();
        b.tick();
        assert_eq!(a.state.run.generators, b.state.run.generators);
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn test_view_auto_buy_default_and_toggle() {
        let mut engine = Engine::new();
        for id in GeneratorId::ALL {
            assert!(engine.view().auto_buy(*id), "default on for {id}");
        }
        assert_eq!(
            engine.apply(Action::SetAutoBuy {
                id: GeneratorId::Hydrophone,
                enabled: false,
            }),
            Ok(())
        );
        assert!(!engine.view().auto_buy(GeneratorId::Hydrophone));
        assert!(engine.view().auto_buy(GeneratorId::Drifter));
    }

    #[test]
    fn demo_is_deterministic_and_consistent() {
        let mut a = Engine::demo();
        let b = Engine::demo();
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.state_hash(), a.state_hash(), "hash stable across calls");

        let view = a.view();
        assert!(view.can_descend());
        assert!(view.rate() > Decimal::ZERO);
        assert_eq!(view.depth_m(), 9_412);
        assert!(view.hull_owned(HullId::BallastGovernor));
        assert_eq!(view.hull_count(), 15);
        for id in GeneratorId::ALL {
            assert!(view.generator_owned(*id) > 0, "demo must own {id}");
        }
        // No non-finite values can exist in Decimal; assert bounds explicitly.
        assert!(a.state.run.echo.mantissa().is_finite());
        assert!(a.state.meta.stats.best_peak.mantissa().is_finite());
        assert!(a.state.run.echo.exponent10() <= i64::MAX / 2);

        // The governor is owned, so the target action is accepted.
        assert_eq!(a.apply(Action::SetAutoDescendTarget { fathoms: 3 }), Ok(()));
    }

    // -----------------------------------------------------------------
    // Persistence boundary: state() / from_state()
    // -----------------------------------------------------------------

    #[test]
    fn state_roundtrip_through_save_load_preserves_hash() {
        let engine = scripted_engine();
        let original_hash = engine.state_hash();

        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = crate::save::SaveStore::new(dir.path());
        store.save(engine.state(), 1_000).expect("save");
        let loaded = store.load(1_000).expect("load");

        let rebuilt = Engine::from_state(loaded.state);
        assert_eq!(rebuilt.state_hash(), original_hash);
    }

    #[test]
    fn from_state_seeds_descent_latch_from_peak() {
        let mut source = Engine::new();
        source.state.run.peak_echo = Decimal::from_u64(1_000_000);
        let mut rebuilt = Engine::from_state(source.state().clone());
        rebuilt.tick();
        let events: Vec<Event> = rebuilt.drain_events().collect();
        assert!(
            !events.iter().any(|e| matches!(e, Event::DescentAvailable)),
            "from_state must not emit DescentAvailable on the first tick"
        );
    }

    #[test]
    fn from_state_first_tick_matches_continuation() {
        let mut source = scripted_engine();
        let rebuilt_state = source.state().clone();
        let mut rebuilt = Engine::from_state(rebuilt_state);

        source.tick();
        rebuilt.tick();

        assert_eq!(source.state_hash(), rebuilt.state_hash());
    }

    // -----------------------------------------------------------------
    // Tutorial
    // -----------------------------------------------------------------

    fn step_satisfied(engine: &Engine, step: u8) -> bool {
        tutorial_step_satisfied(&engine.state, step)
    }

    #[test]
    fn test_tutorial_predicates() {
        // Fresh state satisfies nothing.
        let fresh = Engine::new();
        for step in 0..13u8 {
            assert!(
                !step_satisfied(&fresh, step),
                "step {step} unexpectedly true"
            );
        }

        let mut engine = Engine::new();
        engine.state.meta.stats.pulses_manual = 1;
        assert!(step_satisfied(&engine, 0));

        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 1;
        assert!(step_satisfied(&engine, 1));

        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 2;
        assert!(step_satisfied(&engine, 2));

        let mut engine = Engine::new();
        engine.state.run.generators[GeneratorId::Drifter.index()] = 1;
        assert!(step_satisfied(&engine, 3));

        let mut engine = Engine::new();
        engine.state.run.upgrades.insert(UpgradeId::SweepProfile);
        assert!(
            !step_satisfied(&engine, 4),
            "upgrade step is specific to Impedance Match"
        );
        engine.state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        assert!(step_satisfied(&engine, 4));

        let mut engine = Engine::new();
        engine.state.meta.tutorial.seen_carrier_change = true;
        engine.state.meta.tutorial.tuned_since_change = true;
        engine.state.run.carrier_band = 2;
        engine.state.run.tuned_band = 2;
        assert!(step_satisfied(&engine, 5));

        let mut engine = Engine::new();
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        assert!(step_satisfied(&engine, 6));

        let mut engine = Engine::new();
        engine.state.meta.stats.descents = 1;
        assert!(step_satisfied(&engine, 7));

        let mut engine = Engine::new();
        engine.state.meta.hulls.insert(HullId::AutoPulse);
        assert!(step_satisfied(&engine, 8));

        let mut engine = Engine::new();
        engine.state.meta.stats.descents = 2;
        assert!(step_satisfied(&engine, 9));

        let mut engine = Engine::new();
        engine.state.meta.tutorial.system_opened = true;
        assert!(step_satisfied(&engine, 10));

        let mut engine = Engine::new();
        engine.state.meta.tutorial.manual_opened = true;
        engine.state.meta.tutorial.manual_closed = true;
        assert!(step_satisfied(&engine, 11));

        let mut engine = Engine::new();
        engine.state.meta.tutorial.acknowledged = true;
        assert!(step_satisfied(&engine, 12));
    }

    #[test]
    fn test_tutorial_progress_values() {
        let mut engine = Engine::new();
        let progress = |engine: &mut Engine, step: u8| {
            engine.state.meta.tutorial.step = step;
            engine.tutorial().progress
        };

        let p = progress(&mut engine, 1).unwrap();
        assert_eq!(p.label, "HYDROPHONES");
        assert_eq!(p.current, Decimal::ZERO);
        assert_eq!(p.target, Decimal::from_u64(1));

        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 1;
        let p = progress(&mut engine, 2).unwrap();
        assert_eq!(p.target, Decimal::from_u64(2));

        engine.state.run.generators[GeneratorId::Hydrophone.index()] = 3;
        let p = progress(&mut engine, 3).unwrap();
        assert_eq!(p.label, "HYDROPHONES");
        assert_eq!(p.current, Decimal::from_u64(3));
        assert_eq!(p.target, Decimal::from_u64(10));

        engine.state.run.echo = Decimal::from_u64(50);
        let p = progress(&mut engine, 4).unwrap();
        assert_eq!(p.label, "ECHO");
        assert_eq!(p.current, Decimal::from_u64(50));
        assert_eq!(p.target, Decimal::from_u64(100));

        engine.state.run.peak_echo = Decimal::from_parts(2.0, 5).unwrap();
        let p = progress(&mut engine, 6).unwrap();
        assert_eq!(p.label, "PEAK ECHO");
        assert_eq!(p.target, Decimal::from_parts(1.0, 6).unwrap());

        engine.state.meta.stats.descents = 1;
        let p = progress(&mut engine, 7).unwrap();
        assert_eq!(p.label, "DESCENTS");
        assert_eq!(p.target, Decimal::from_u64(1));
        let p = progress(&mut engine, 9).unwrap();
        assert_eq!(p.target, Decimal::from_u64(2));

        for step in [0u8, 5, 8, 10, 11, 12] {
            assert!(progress(&mut engine, step).is_none(), "step {step}");
        }
    }

    #[test]
    fn test_tune_requires_post_drift_keypress() {
        let mut engine = Engine::new();
        engine.state.meta.tutorial.step = 5; // Tune
        engine.state.run.carrier_band = 0;
        engine.state.run.tuned_band = 0;

        // Already matched before any drift: a tuning keypress alone is not enough.
        engine.apply(Action::SetTuning { band: 0 }).unwrap();
        assert!(engine.state.meta.tutorial.tuned_since_change);
        assert!(!engine.state.meta.tutorial.seen_carrier_change);
        assert_eq!(engine.tutorial().index, 5);

        // Carrier drift clears the keypress requirement.
        engine.advance(900);
        assert!(engine.state.meta.tutorial.seen_carrier_change);
        assert!(!engine.state.meta.tutorial.tuned_since_change);
        assert_eq!(engine.tutorial().index, 5);

        // Tuning to the wrong band is still a keypress but not a match.
        engine.apply(Action::SetTuning { band: 0 }).unwrap();
        assert!(engine.state.meta.tutorial.tuned_since_change);
        assert_eq!(engine.tutorial().index, 5);

        // Post-drift tuning to the carrier satisfies the step.
        engine.apply(Action::SetTuning { band: 1 }).unwrap();
        assert_eq!(engine.tutorial().index, 6);
    }

    fn buy_everything(engine: &mut Engine) {
        for id in [
            GeneratorId::Hydrophone,
            GeneratorId::Drifter,
            GeneratorId::SonarMast,
            GeneratorId::TidalCore,
        ] {
            let _ = engine.apply(Action::BuyGenerator {
                id,
                quantity: BuyQuantity::Max,
            });
        }
        for id in UpgradeId::ALL {
            let _ = engine.apply(Action::BuyUpgrade(*id));
        }
        let _ = engine.apply(Action::BuyHull(HullId::AutoPulse));
    }

    fn advance_to_peak(engine: &mut Engine) {
        for _ in 0..5000 {
            engine.advance(10);
            if !engine.view().peak().is_zero() && engine.view().peak().exponent10() >= 6 {
                return;
            }
            if engine.view().can_pulse() {
                let _ = engine.apply(Action::Pulse);
            }
            buy_everything(engine);
        }
        panic!(
            "peak not reached; tutorial step {}",
            engine.tutorial().index
        );
    }

    #[test]
    fn test_tutorial_full_progression() {
        let mut engine = Engine::new();
        assert_eq!(engine.tutorial().index, 0);

        // Step 0: pulse.
        engine.apply(Action::Pulse).unwrap();
        assert_eq!(engine.tutorial().index, 1);

        // Steps 1-2: two Hydrophones.
        engine
            .apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            })
            .unwrap();
        for _ in 0..100 {
            engine.advance(10);
            if engine.view().echo() >= Decimal::from_u64(12) {
                break;
            }
        }
        engine
            .apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            })
            .unwrap();
        assert!(engine.view().generator_owned(GeneratorId::Hydrophone) >= 2);
        assert_eq!(engine.tutorial().index, 3);

        // Step 3: reach 10 Hydrophones and buy a Drifter.
        for _ in 0..3000 {
            engine.advance(10);
            let _ = engine.apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::Max,
            });
            let _ = engine.apply(Action::BuyGenerator {
                id: GeneratorId::Drifter,
                quantity: BuyQuantity::Max,
            });
            if engine.view().generator_owned(GeneratorId::Drifter) >= 1
                && engine.view().echo() >= Decimal::from_u64(150)
            {
                break;
            }
        }
        assert!(engine.view().generator_owned(GeneratorId::Drifter) >= 1);
        assert_eq!(engine.tutorial().index, 4);

        // Step 4: Impedance Match is unlocked by the Drifter.
        engine
            .apply(Action::BuyUpgrade(UpgradeId::ImpedanceMatch))
            .unwrap();
        assert!(engine.view().upgrade_owned(UpgradeId::ImpedanceMatch));
        assert_eq!(engine.tutorial().index, 5);

        // Step 5: wait for the carrier to drift, then tune to it.
        engine.advance(900);
        assert!(engine.state.meta.tutorial.seen_carrier_change);
        assert!(!engine.state.meta.tutorial.tuned_since_change);
        let band = engine.view().carrier_band();
        engine.apply(Action::SetTuning { band }).unwrap();
        assert!(engine.tutorial().index >= 6, "tune chain");

        // Step 6: peak 1e6.
        if engine.tutorial().index == 6 {
            advance_to_peak(&mut engine);
        }
        assert_eq!(engine.tutorial().index, 7);

        // Step 7-8: descend, then buy Auto-Pulse.
        engine.apply(Action::Descend).unwrap();
        assert_eq!(engine.tutorial().index, 8);
        engine.apply(Action::BuyHull(HullId::AutoPulse)).unwrap();
        assert_eq!(engine.tutorial().index, 9);

        // Step 9: second descent.
        advance_to_peak(&mut engine);
        engine.apply(Action::Descend).unwrap();
        assert_eq!(engine.tutorial().index, 10);

        // Steps 10-11: UI surfaces.
        engine.tutorial_ui(TutorialUiEvent::SystemOpened);
        assert_eq!(engine.tutorial().index, 11);
        engine.tutorial_ui(TutorialUiEvent::ManualOpened);
        engine.tutorial_ui(TutorialUiEvent::ManualClosed);
        assert_eq!(engine.tutorial().index, 12);

        // Step 12: acknowledge.
        engine.tutorial_acknowledge();
        assert!(engine.tutorial().completed);
        assert!(engine.tutorial().progress.is_none());
    }

    #[test]
    fn test_tutorial_out_of_order_flags() {
        let mut engine = Engine::new();
        // Flags set long before their step is current.
        engine.tutorial_ui(TutorialUiEvent::SystemOpened);
        engine.tutorial_ui(TutorialUiEvent::ManualOpened);
        engine.tutorial_ui(TutorialUiEvent::ManualClosed);
        assert_eq!(engine.tutorial().index, 0, "still waiting on pulse");

        engine.state.meta.tutorial.step = 10;
        engine.observe_tutorial();
        assert_eq!(engine.tutorial().index, 12);
    }

    #[test]
    fn test_tutorial_auto_advance_stops_at_acknowledge() {
        let mut engine = Engine::new();
        engine.state.meta.stats.pulses_manual = 1;
        engine.state.run.generators = [10, 1, 0, 0];
        engine.state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        engine.state.meta.tutorial.seen_carrier_change = true;
        engine.state.meta.tutorial.tuned_since_change = true;
        engine.state.run.carrier_band = 0;
        engine.state.run.tuned_band = 0;
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.state.meta.stats.descents = 2;
        engine.state.meta.hulls.insert(HullId::AutoPulse);
        engine.state.meta.tutorial.system_opened = true;
        engine.state.meta.tutorial.manual_opened = true;
        engine.state.meta.tutorial.manual_closed = true;

        engine.observe_tutorial();
        assert_eq!(engine.tutorial().index, 12);
        assert!(!engine.tutorial().completed);
        engine.observe_tutorial();
        assert_eq!(engine.tutorial().index, 12);
    }

    #[test]
    fn test_tutorial_skip_and_replay_idempotent() {
        let mut engine = Engine::new();
        engine.tutorial_skip();
        assert!(engine.tutorial().skipped);
        assert!(!engine.tutorial().completed);
        assert_eq!(engine.tutorial().index, 0);
        assert!(engine.tutorial().progress.is_none());
        engine.tutorial_skip();
        assert!(engine.tutorial().skipped);

        engine.tutorial_ui(TutorialUiEvent::SystemOpened);
        engine.tutorial_replay();
        assert_eq!(engine.tutorial().index, 0);
        assert!(!engine.tutorial().skipped);
        assert!(!engine.tutorial().completed);
        assert!(!engine.state.meta.tutorial.system_opened);
        engine.tutorial_replay();
        assert_eq!(engine.state.meta.tutorial, TutorialState::new());
    }

    #[test]
    fn test_tutorial_survives_descent() {
        let mut engine = Engine::new();
        engine.state.meta.tutorial.step = 9;
        engine.state.run.peak_echo = Decimal::from_parts(1.0, 6).unwrap();
        engine.apply(Action::Descend).unwrap();
        assert_eq!(engine.state.meta.tutorial.step, 9);
        assert!(!engine.tutorial().completed);
        assert_eq!(engine.tutorial().chapter, 2);
    }

    #[test]
    fn test_demo_tutorial_completed() {
        let engine = Engine::demo();
        assert!(engine.tutorial().completed);
        assert!(!engine.tutorial().skipped);
        assert!(engine.tutorial().progress.is_none());
        assert_eq!(engine.tutorial().chapter, 2);
    }
}
