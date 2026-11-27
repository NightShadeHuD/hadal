//! Read-only views for the presentation layer and simulator.

use crate::content::ids::{AchievementId, GeneratorId, HullId, LogId, UpgradeId};
use crate::content::tables;
use crate::decimal::Decimal;
use crate::engine::{
    BuyQuantity, dec_f64, generator_cumulative_cost, generator_tier_mult, global_multiplier,
    max_affordable, production_rate,
};
use crate::state::GameState;

/// What a Descent wipes (shown in the confirmation dialog).
pub const DESCENT_RESET_SCOPE: &[&str] = &[
    "Echo",
    "Peak Echo",
    "Generators",
    "Signal Chain upgrades",
    "Pulse capacitor",
    "Carrier tuning",
    "Run tick counters",
];

/// A borrowed, read-only view over the game state.
#[derive(Clone, Copy, Debug)]
pub struct StateView<'a> {
    pub(crate) state: &'a GameState,
}

impl StateView<'_> {
    // -------------------------------------------------------------
    // Resources
    // -------------------------------------------------------------

    /// Current Echo balance.
    pub fn echo(&self) -> Decimal {
        self.state.run.echo
    }

    /// Current Echo per second.
    pub fn rate(&self) -> Decimal {
        production_rate(self.state)
    }

    /// Peak Echo this run.
    pub fn peak(&self) -> Decimal {
        self.state.run.peak_echo
    }

    /// Spendable Fathoms.
    pub fn fathoms(&self) -> u32 {
        self.state.meta.fathoms
    }

    /// Cumulative era fathoms (granted by descents; never resets).
    pub fn era_fathoms(&self) -> u32 {
        self.state.meta.era_fathoms
    }

    /// Lifetime Fathoms.
    pub fn lifetime_fathoms(&self) -> u32 {
        self.state.meta.lifetime_fathoms
    }

    /// Current depth in metres.
    pub fn depth_m(&self) -> u32 {
        self.state.meta.depth_m
    }

    /// Offline cap in hours from the highest owned endurance hull.
    pub fn offline_cap_hours(&self) -> u32 {
        tables::offline_cap_hours(&self.state.meta.hulls)
    }

    /// Global multiplier (carrier × hulls × era × achievements).
    pub fn global_multiplier(&self) -> Decimal {
        global_multiplier(self.state)
    }

    // -------------------------------------------------------------
    // Generators
    // -------------------------------------------------------------

    /// Whether a generator is unlocked.
    pub fn generator_unlocked(&self, id: GeneratorId) -> bool {
        tables::generator_def(id)
            .unlock
            .met(&self.state.run.generators)
    }

    /// Owned count of a generator.
    pub fn generator_owned(&self, id: GeneratorId) -> u32 {
        self.state.run.generators[id.index()]
    }

    /// All four generator owned counts, in [`GeneratorId`] order.
    pub fn generator_counts(&self) -> [u32; 4] {
        self.state.run.generators
    }

    /// UI display of the automation toggle; mutation only through
    /// `Action::SetAutoBuy`.
    pub fn auto_buy(&self, id: GeneratorId) -> bool {
        self.state.meta.auto_buy[id.index()]
    }

    /// Cost of purchasing `quantity` of a generator (Max resolves to the
    /// affordable amount; zero if none is affordable).
    pub fn generator_cost(&self, id: GeneratorId, quantity: BuyQuantity) -> Decimal {
        let owned = self.generator_owned(id);
        let n = match quantity {
            BuyQuantity::One => 1,
            BuyQuantity::Ten => 10,
            BuyQuantity::Max => max_affordable(id, owned, self.state.run.echo),
        };
        generator_cumulative_cost(id, owned, n)
    }

    /// Whether `quantity` of a generator is affordable.
    pub fn generator_affordable(&self, id: GeneratorId, quantity: BuyQuantity) -> bool {
        if !self.generator_unlocked(id) {
            return false;
        }
        let owned = self.generator_owned(id);
        let n = match quantity {
            BuyQuantity::One => 1,
            BuyQuantity::Ten => 10,
            BuyQuantity::Max => max_affordable(id, owned, self.state.run.echo),
        };
        n > 0 && generator_cumulative_cost(id, owned, n) <= self.state.run.echo
    }

    /// Return-on-investment for one unit: `base_rate × tier_mult / cost`.
    pub fn generator_roi(&self, id: GeneratorId) -> Decimal {
        let def = tables::generator_def(id);
        let gain = dec_f64(def.base_rate).mul(generator_tier_mult(self.state, id));
        let cost = generator_cumulative_cost(id, self.generator_owned(id), 1);
        if cost.is_zero() {
            return Decimal::ZERO;
        }
        gain.div(cost).unwrap_or(Decimal::ZERO)
    }

    // -------------------------------------------------------------
    // Upgrades
    // -------------------------------------------------------------

    /// Whether an upgrade is owned.
    pub fn upgrade_owned(&self, id: UpgradeId) -> bool {
        self.state.run.upgrades.contains(&id)
    }

    /// Whether an upgrade is unlocked (ignoring ownership).
    pub fn upgrade_unlocked(&self, id: UpgradeId) -> bool {
        tables::upgrade_def(id)
            .unlock
            .met(&self.state.run.generators)
    }

    /// Echo cost of an upgrade.
    pub fn upgrade_cost(&self, id: UpgradeId) -> Decimal {
        Decimal::from_u64(tables::upgrade_def(id).cost as u64)
    }

    /// Whether an upgrade can be bought now.
    pub fn upgrade_affordable(&self, id: UpgradeId) -> bool {
        !self.upgrade_owned(id)
            && self.upgrade_unlocked(id)
            && self.state.run.echo >= self.upgrade_cost(id)
    }

    /// Number of owned in-run upgrades.
    pub fn upgrade_count(&self) -> usize {
        self.state.run.upgrades.len()
    }

    // -------------------------------------------------------------
    // Hulls
    // -------------------------------------------------------------

    /// Whether a hull is owned.
    pub fn hull_owned(&self, id: HullId) -> bool {
        self.state.meta.hulls.contains(&id)
    }

    /// Number of owned hulls.
    pub fn hull_count(&self) -> usize {
        self.state.meta.hulls.len()
    }

    /// Whether a hull's prerequisite is satisfied.
    pub fn hull_prereq_met(&self, id: HullId) -> bool {
        match tables::hull_def(id).prereq {
            None => true,
            Some(prereq) => self.state.meta.hulls.contains(&prereq),
        }
    }

    /// Fathom cost of a hull.
    pub fn hull_cost(&self, id: HullId) -> u32 {
        tables::hull_def(id).cost
    }

    /// Short effect description for a hull.
    pub fn hull_effect_text(&self, id: HullId) -> &'static str {
        tables::hull_effect_text(id)
    }

    /// Whether a hull can be bought now.
    pub fn hull_affordable(&self, id: HullId) -> bool {
        !self.hull_owned(id)
            && self.hull_prereq_met(id)
            && self.state.meta.fathoms >= self.hull_cost(id)
    }

    // -------------------------------------------------------------
    // Descent
    // -------------------------------------------------------------

    /// Whether a Descent is currently available.
    pub fn can_descend(&self) -> bool {
        let peak = self.state.run.peak_echo;
        !peak.is_zero() && peak.exponent10() >= tables::DESCENT_THRESHOLD_EXP
    }

    /// Fathom gain if a Descent were performed now (0 if unavailable).
    pub fn descent_gain(&self) -> u32 {
        let tidal = self.state.meta.hulls.contains(&HullId::TidalLoop);
        let fractal = self.state.meta.hulls.contains(&HullId::FractalHull);
        tables::descent_gain(self.state.run.peak_echo, tidal, fractal)
    }

    /// The next peak-Echo value that increases the Descent gain.
    pub fn next_bucket_threshold(&self) -> Decimal {
        let threshold = Decimal::from_u64(1_000_000);
        let peak = self.state.run.peak_echo;
        if peak.is_zero() || peak < threshold {
            return threshold;
        }
        let exponent = peak.exponent10();
        let mantissa = peak.mantissa();
        if mantissa < tables::DESCENT_BUCKET0 {
            return Decimal::from_parts(tables::DESCENT_BUCKET0, exponent).unwrap_or(Decimal::ZERO);
        }
        if mantissa < tables::DESCENT_BUCKET1 {
            return Decimal::from_parts(tables::DESCENT_BUCKET1, exponent).unwrap_or(Decimal::ZERO);
        }
        Decimal::from_parts(1.0, exponent + 1).unwrap_or(Decimal::ZERO)
    }

    /// The fields a Descent resets.
    pub fn reset_scope(&self) -> &'static [&'static str] {
        DESCENT_RESET_SCOPE
    }

    /// The Ballast Governor target (0 = off).
    pub fn auto_descend_at(&self) -> u32 {
        self.state.meta.auto_descend_at
    }

    // -------------------------------------------------------------
    // Carrier and pulse
    // -------------------------------------------------------------

    /// Current carrier band.
    pub fn carrier_band(&self) -> u8 {
        self.state.run.carrier_band
    }

    /// Current tuned band.
    pub fn tuned_band(&self) -> u8 {
        self.state.run.tuned_band
    }

    /// Whether the tuned band matches the carrier.
    pub fn carrier_matched(&self) -> bool {
        self.state.run.tuned_band == self.state.run.carrier_band
    }

    /// Current pulse capacitor charge in ticks.
    pub fn pulse_charge_ticks(&self) -> u16 {
        self.state.run.charge_ticks
    }

    /// Pulse capacitor capacity in ticks.
    pub fn pulse_capacity(&self) -> u16 {
        tables::PULSE_CAPACITOR_TICKS
    }

    /// Fractional charge in `0.0..=1.0` (presentation only).
    pub fn pulse_charge_fraction(&self) -> f64 {
        let cap = tables::PULSE_CAPACITOR_TICKS;
        if cap == 0 {
            return 0.0;
        }
        (self.state.run.charge_ticks as f64 / cap as f64).clamp(0.0, 1.0)
    }

    /// Whether a pulse may be fired now.
    pub fn can_pulse(&self) -> bool {
        self.state.run.charge_ticks >= tables::PULSE_CAPACITOR_TICKS
    }

    // -------------------------------------------------------------
    // Lifetime statistics (read-only metrics)
    // -------------------------------------------------------------

    /// Total Echo ever produced (production + pulse yields).
    pub fn lifetime_echo(&self) -> Decimal {
        self.state.meta.stats.lifetime_echo
    }

    /// Best peak Echo ever reached.
    pub fn best_peak(&self) -> Decimal {
        self.state.meta.stats.best_peak
    }

    /// Manual pulses fired.
    pub fn pulses_manual(&self) -> u32 {
        self.state.meta.stats.pulses_manual
    }

    /// Automatic pulses fired.
    pub fn pulses_auto(&self) -> u32 {
        self.state.meta.stats.pulses_auto
    }

    /// Purchase actions performed.
    pub fn purchases(&self) -> u32 {
        self.state.meta.stats.purchases
    }

    /// Descents completed.
    pub fn descents(&self) -> u32 {
        self.state.meta.stats.descents
    }

    /// Total ticks simulated.
    pub fn total_ticks(&self) -> u64 {
        self.state.meta.stats.total_ticks
    }

    /// Times a tuning action matched the carrier.
    pub fn bands_matched(&self) -> u32 {
        self.state.meta.stats.bands_matched
    }

    // -------------------------------------------------------------
    // Achievements and logs
    // -------------------------------------------------------------

    /// Whether an achievement is unlocked.
    pub fn achievement_unlocked(&self, id: AchievementId) -> bool {
        self.state.meta.achievements.contains(&id)
    }

    /// Whether a log entry is recorded.
    pub fn log_recorded(&self, id: LogId) -> bool {
        self.state.meta.logs.contains(&id)
    }

    /// Total unlocked achievements.
    pub fn achievement_count(&self) -> usize {
        self.state.meta.achievements.len()
    }
}
