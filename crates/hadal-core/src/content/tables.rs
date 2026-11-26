//! The single source of truth for all balance numbers.
//!
//! `f64` appears only in these constant tables (content), never in state.
//! Values are converted to [`Decimal`] at the point of use in the engine.

use std::collections::BTreeSet;

use crate::decimal::Decimal;
use crate::state::GameState;

use super::ids::{AchievementId, GeneratorId, HullId, LogId, UpgradeId};

// ---------------------------------------------------------------------
// Global constants
// ---------------------------------------------------------------------

/// Passive station draw in Echo/second before multipliers.
pub const STATION_DRAW_PER_SEC: f64 = 0.1;
/// Pulse capacitor capacity in ticks (10 Hz → 25 s).
pub const PULSE_CAPACITOR_TICKS: u16 = 250;
/// Pulse yield counts this many seconds of production.
pub const PULSE_SECONDS: f64 = 10.0;
/// Minimum pulse yield.
pub const PULSE_FLOOR: f64 = 10.0;
/// Carrier band cycle length in ticks.
pub const CARRIER_CYCLE_TICKS: u16 = 900;
/// Carrier match global multiplier.
pub const CARRIER_MATCH_MULT: f64 = 1.5;
/// Descent threshold as a base-10 exponent (1e6).
pub const DESCENT_THRESHOLD_EXP: i64 = 6;
/// Starting depth in metres.
pub const BASE_DEPTH_M: u32 = 7912;
/// Metres added per Descent.
pub const DEPTH_PER_DESCENT_M: u32 = 500;
/// Era Fathom passive: +5% per Fathom.
pub const ERA_FATHOM_BONUS: f64 = 0.05;
/// Achievement passive: +1% per achievement.
pub const ACHIEVEMENT_BONUS: f64 = 0.01;
/// Base offline cap in hours.
pub const OFFLINE_CAP_BASE_HOURS: u32 = 8;
/// Maximum offline cap in hours.
pub const OFFLINE_CAP_MAX_HOURS: u32 = 168;
/// Minimum non-zero Ballast Governor target.
pub const GOVERNOR_TARGET_MIN: u32 = 3;
/// Maximum Ballast Governor target.
pub const GOVERNOR_TARGET_MAX: u32 = 1_000_000;
/// Hydrophones granted at run start by Ballast Recall.
pub const BALLAST_RECALL_HYDROPHONES: u32 = 5;

/// First Descent bucket mantissa boundary (s = 0 below, 1 at/above).
pub const DESCENT_BUCKET0: f64 = 2.154_434_690_031_884;
/// Second Descent bucket mantissa boundary (s = 1 below, 2 at/above).
pub const DESCENT_BUCKET1: f64 = 4.641_588_833_612_779;

// ---------------------------------------------------------------------
// Unlock conditions
// ---------------------------------------------------------------------

/// A condition that must hold for an item to be purchasable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlock {
    /// Always available.
    Always,
    /// At least `count` of the given generator must be owned.
    GeneratorsAtLeast(GeneratorId, u32),
}

impl Unlock {
    /// Returns whether this unlock is satisfied by `generators`.
    pub fn met(self, generators: &[u32; 4]) -> bool {
        match self {
            Unlock::Always => true,
            Unlock::GeneratorsAtLeast(id, n) => generators[id.index()] >= n,
        }
    }
}

// ---------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------

/// Definition of an Echo generator.
#[derive(Clone, Copy, Debug)]
pub struct GeneratorDef {
    /// Stable id.
    pub id: GeneratorId,
    /// Base cost at zero owned.
    pub base_cost: f64,
    /// Cost ratio per owned unit.
    pub cost_ratio: f64,
    /// Base Echo/second per owned unit.
    pub base_rate: f64,
    /// Unlock condition.
    pub unlock: Unlock,
}

/// The four generators, in [`GeneratorId`] order.
pub const GENERATORS: [GeneratorDef; 4] = [
    GeneratorDef {
        id: GeneratorId::Hydrophone,
        base_cost: 10.0,
        cost_ratio: 1.15,
        base_rate: 0.5,
        unlock: Unlock::Always,
    },
    GeneratorDef {
        id: GeneratorId::Drifter,
        base_cost: 250.0,
        cost_ratio: 1.18,
        base_rate: 8.0,
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::Hydrophone, 10),
    },
    GeneratorDef {
        id: GeneratorId::SonarMast,
        base_cost: 20_000.0,
        cost_ratio: 1.20,
        base_rate: 120.0,
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::Drifter, 6),
    },
    GeneratorDef {
        id: GeneratorId::TidalCore,
        base_cost: 2_500_000.0,
        cost_ratio: 1.22,
        base_rate: 2500.0,
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 5),
    },
];

/// Returns the definition for a generator.
pub fn generator_def(id: GeneratorId) -> &'static GeneratorDef {
    &GENERATORS[id.index()]
}

// ---------------------------------------------------------------------
// Signal Chain upgrades
// ---------------------------------------------------------------------

/// The production effect of an upgrade.
#[derive(Clone, Copy, Debug)]
pub enum UpgradeEffect {
    /// Multiplies all production by the given factor.
    All(f64),
    /// Multiplies one generator tier by the given factor.
    Tier(GeneratorId, f64),
}

/// Definition of an in-run upgrade.
#[derive(Clone, Copy, Debug)]
pub struct UpgradeDef {
    /// Stable id.
    pub id: UpgradeId,
    /// Cost in Echo.
    pub cost: u32,
    /// Production effect.
    pub effect: UpgradeEffect,
    /// Unlock condition.
    pub unlock: Unlock,
}

/// The eight Signal Chain upgrades, in [`UpgradeId`] order.
pub const UPGRADES: [UpgradeDef; 8] = [
    UpgradeDef {
        id: UpgradeId::ImpedanceMatch,
        cost: 100,
        effect: UpgradeEffect::All(2.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::Drifter, 1),
    },
    UpgradeDef {
        id: UpgradeId::SweepProfile,
        cost: 750,
        effect: UpgradeEffect::Tier(GeneratorId::Hydrophone, 3.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::Hydrophone, 10),
    },
    UpgradeDef {
        id: UpgradeId::CascadedGain,
        cost: 8_000,
        effect: UpgradeEffect::All(2.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 1),
    },
    UpgradeDef {
        id: UpgradeId::StableDrift,
        cost: 40_000,
        effect: UpgradeEffect::Tier(GeneratorId::Drifter, 3.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::Drifter, 6),
    },
    UpgradeDef {
        id: UpgradeId::CoherentAperture,
        cost: 300_000,
        effect: UpgradeEffect::All(2.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 5),
    },
    UpgradeDef {
        id: UpgradeId::MastPhasing,
        cost: 2_000_000,
        effect: UpgradeEffect::Tier(GeneratorId::SonarMast, 4.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 5),
    },
    UpgradeDef {
        id: UpgradeId::CoreResonance,
        cost: 25_000_000,
        effect: UpgradeEffect::Tier(GeneratorId::TidalCore, 3.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::TidalCore, 1),
    },
    UpgradeDef {
        id: UpgradeId::FullSpectrum,
        cost: 150_000_000,
        effect: UpgradeEffect::All(3.0),
        unlock: Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 25),
    },
];

/// Returns the definition for an upgrade.
pub fn upgrade_def(id: UpgradeId) -> &'static UpgradeDef {
    &UPGRADES[id.index()]
}

// ---------------------------------------------------------------------
// Hull tree
// ---------------------------------------------------------------------

/// Definition of a permanent hull upgrade.
#[derive(Clone, Copy, Debug)]
pub struct HullDef {
    /// Stable id.
    pub id: HullId,
    /// Cost in spendable Fathoms.
    pub cost: u32,
    /// Required hull, if any.
    pub prereq: Option<HullId>,
    /// Global production multiplier, if any.
    pub global_mult: Option<f64>,
    /// Pulse yield multiplier, if any.
    pub pulse_mult: Option<f64>,
    /// Fathom gain multiplier, if any.
    pub fathom_mult: Option<f64>,
    /// Offline cap in hours, if any.
    pub offline_hours: Option<u32>,
}

const fn hull(id: HullId, cost: u32, prereq: Option<HullId>, global_mult: Option<f64>) -> HullDef {
    HullDef {
        id,
        cost,
        prereq,
        global_mult,
        pulse_mult: None,
        fathom_mult: None,
        offline_hours: None,
    }
}

/// The seventeen hulls, in [`HullId`] order.
pub const HULLS: [HullDef; 17] = [
    hull(HullId::AutoPulse, 1, None, None),
    hull(HullId::BallastRecall, 2, None, None),
    hull(HullId::TunedBallast, 3, Some(HullId::BallastRecall), None),
    hull(HullId::AutoBuyI, 5, Some(HullId::AutoPulse), None),
    hull(HullId::DeepMemory, 8, None, Some(1.25)),
    HullDef {
        id: HullId::CoiledHull,
        cost: 12,
        prereq: Some(HullId::DeepMemory),
        global_mult: None,
        pulse_mult: Some(2.0),
        fathom_mult: None,
        offline_hours: None,
    },
    hull(HullId::AutoBuyII, 20, Some(HullId::AutoBuyI), None),
    hull(
        HullId::BallastGovernor,
        25,
        Some(HullId::BallastRecall),
        None,
    ),
    HullDef {
        id: HullId::LongStorage,
        cost: 30,
        prereq: None,
        global_mult: None,
        pulse_mult: None,
        fathom_mult: None,
        offline_hours: Some(24),
    },
    hull(HullId::ChainAutomator, 40, Some(HullId::AutoBuyI), None),
    hull(HullId::NullArray, 50, Some(HullId::DeepMemory), Some(2.0)),
    hull(HullId::AutoBuyIII, 80, Some(HullId::AutoBuyII), None),
    HullDef {
        id: HullId::ExtendedBallast,
        cost: 120,
        prereq: Some(HullId::LongStorage),
        global_mult: None,
        pulse_mult: None,
        fathom_mult: None,
        offline_hours: Some(72),
    },
    HullDef {
        id: HullId::TidalLoop,
        cost: 200,
        prereq: Some(HullId::TunedBallast),
        global_mult: None,
        pulse_mult: None,
        fathom_mult: Some(1.5),
        offline_hours: None,
    },
    HullDef {
        id: HullId::AbyssalReservoir,
        cost: 350,
        prereq: Some(HullId::ExtendedBallast),
        global_mult: None,
        pulse_mult: None,
        fathom_mult: None,
        offline_hours: Some(168),
    },
    hull(HullId::DeepField, 600, Some(HullId::NullArray), Some(3.0)),
    HullDef {
        id: HullId::FractalHull,
        cost: 50000,
        prereq: Some(HullId::TidalLoop),
        global_mult: None,
        pulse_mult: None,
        fathom_mult: Some(2.0),
        offline_hours: None,
    },
];

/// Returns the definition for a hull.
pub fn hull_def(id: HullId) -> &'static HullDef {
    &HULLS[id.index()]
}

/// Short human-readable effect description for a hull.
pub fn hull_effect_text(id: HullId) -> &'static str {
    match id {
        HullId::AutoPulse => "Fires a pulse whenever the capacitor is full.",
        HullId::BallastRecall => "Start every run with 5 Hydrophones.",
        HullId::TunedBallast => "Auto-tune to the carrier on every change.",
        HullId::AutoBuyI => "Auto-buy Hydrophones and Drifters.",
        HullId::DeepMemory => "Global production ×1.25.",
        HullId::CoiledHull => "Pulse yield ×2.",
        HullId::AutoBuyII => "Auto-buy also covers Sonar Masts.",
        HullId::BallastGovernor => "Enables an automatic Descent target.",
        HullId::LongStorage => "Offline cap 24 h.",
        HullId::ChainAutomator => "Buys the cheapest affordable upgrade each second.",
        HullId::NullArray => "Global production ×2.",
        HullId::AutoBuyIII => "Auto-buy also covers Tidal Cores.",
        HullId::ExtendedBallast => "Offline cap 72 h.",
        HullId::TidalLoop => "Fathom gain ×1.5.",
        HullId::AbyssalReservoir => "Offline cap 168 h.",
        HullId::DeepField => "Global production ×3.",
        HullId::FractalHull => "Fathom gain ×2.",
    }
}

/// Offline cap in hours given the owned hulls.
pub fn offline_cap_hours(hulls: &BTreeSet<HullId>) -> u32 {
    let mut cap = OFFLINE_CAP_BASE_HOURS;
    for h in HullId::ALL {
        if let Some(hours) = hull_def(*h).offline_hours
            && hulls.contains(h)
        {
            cap = cap.max(hours);
        }
    }
    cap.min(OFFLINE_CAP_MAX_HOURS)
}

// ---------------------------------------------------------------------
// Achievements and logs
// ---------------------------------------------------------------------

/// Definition of an achievement.
#[derive(Clone, Copy, Debug)]
pub struct AchievementDef {
    /// Stable id.
    pub id: AchievementId,
    /// Short display name.
    pub name: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// The eleven achievements, in [`AchievementId`] order.
pub const ACHIEVEMENTS: [AchievementDef; 11] = [
    AchievementDef {
        id: AchievementId::FirstContact,
        name: "First Contact",
        description: "Fire a manual pulse.",
    },
    AchievementDef {
        id: AchievementId::Hydrophile,
        name: "Hydrophile",
        description: "Own a Hydrophone.",
    },
    AchievementDef {
        id: AchievementId::Chorus,
        name: "Chorus",
        description: "Own 50 Hydrophones.",
    },
    AchievementDef {
        id: AchievementId::Drifter,
        name: "Drifter",
        description: "Own a Drifter.",
    },
    AchievementDef {
        id: AchievementId::Mast,
        name: "Mast",
        description: "Own a Sonar Mast.",
    },
    AchievementDef {
        id: AchievementId::Core,
        name: "Core",
        description: "Own a Tidal Core.",
    },
    AchievementDef {
        id: AchievementId::FirstDescent,
        name: "First Descent",
        description: "Complete a Descent.",
    },
    AchievementDef {
        id: AchievementId::SixDeep,
        name: "Six Deep",
        description: "Complete six Descents.",
    },
    AchievementDef {
        id: AchievementId::Century,
        name: "Century",
        description: "Earn 100 lifetime Fathoms.",
    },
    AchievementDef {
        id: AchievementId::AllBands,
        name: "All Bands",
        description: "Tune every carrier band.",
    },
    AchievementDef {
        id: AchievementId::Signal,
        name: "Signal",
        description: "Reach a best peak of 1e12.",
    },
];

/// Returns the definition for an achievement.
pub fn achievement_def(id: AchievementId) -> &'static AchievementDef {
    &ACHIEVEMENTS[id.index()]
}

/// The ten log IDs, in [`LogId`] order.
pub const LOGS: [LogId; 10] = [
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
];

/// Whether an achievement's condition currently holds.
pub fn achievement_reached(id: AchievementId, state: &GameState) -> bool {
    let meta = &state.meta;
    let run = &state.run;
    let owned = |g: GeneratorId| run.generators[g.index()];
    match id {
        AchievementId::FirstContact => meta.stats.pulses_manual > 0,
        AchievementId::Hydrophile => owned(GeneratorId::Hydrophone) >= 1,
        AchievementId::Chorus => owned(GeneratorId::Hydrophone) >= 50,
        AchievementId::Drifter => owned(GeneratorId::Drifter) >= 1,
        AchievementId::Mast => owned(GeneratorId::SonarMast) >= 1,
        AchievementId::Core => owned(GeneratorId::TidalCore) >= 1,
        AchievementId::FirstDescent => meta.stats.descents >= 1,
        AchievementId::SixDeep => meta.stats.descents >= 6,
        AchievementId::Century => meta.lifetime_fathoms >= 100,
        AchievementId::AllBands => meta.bands_seen.iter().all(|seen| *seen),
        AchievementId::Signal => {
            !meta.stats.best_peak.is_zero() && meta.stats.best_peak.exponent10() >= 12
        }
    }
}

/// Whether a log's trigger condition currently holds.
pub fn log_triggered(id: LogId, state: &GameState) -> bool {
    let meta = &state.meta;
    let run = &state.run;
    let owned = |g: GeneratorId| run.generators[g.index()];
    match id {
        LogId::Boot => true,
        LogId::FirstPulse => meta.stats.pulses_manual >= 1,
        LogId::FirstHydro => owned(GeneratorId::Hydrophone) >= 1,
        LogId::CarrierLock => meta.stats.bands_matched >= 1,
        LogId::FirstDescent => meta.stats.descents >= 1,
        LogId::Depth9000 => meta.depth_m >= 9000,
        LogId::Peak1e9 => !meta.stats.best_peak.is_zero() && meta.stats.best_peak.exponent10() >= 9,
        LogId::Fathoms100 => meta.lifetime_fathoms >= 100,
        LogId::AllBands => meta.bands_seen.iter().all(|seen| *seen),
        LogId::AutoPulse => meta.stats.pulses_auto >= 1,
    }
}

// ---------------------------------------------------------------------
// Descent gain
// ---------------------------------------------------------------------

/// Computes the Fathom gain for a peak Echo.
///
/// Formula (plan §6.4): `d = exponent - 6`, `s` from the two mantissa
/// buckets, `base = 3 + 3d + s`, `gain = max(3, floor(base × tidal × fractal))`.
/// `tidal` is ×1.5 when Tidal Loop is owned, `fractal` is ×2 when Fractal Hull
/// is owned. This is the only formula allowed outside the tables.
pub fn descent_gain(peak: Decimal, tidal: bool, fractal: bool) -> u32 {
    if peak.is_zero() || peak.exponent10() < DESCENT_THRESHOLD_EXP {
        return 0;
    }
    let mantissa = peak.mantissa();
    let exponent = peak.exponent10();
    // Clamp d so a tampered, astronomically large peak cannot overflow.
    let d = (exponent - DESCENT_THRESHOLD_EXP).clamp(0, 1_000_000);
    let s: i64 = if mantissa < DESCENT_BUCKET0 {
        0
    } else if mantissa < DESCENT_BUCKET1 {
        1
    } else {
        2
    };
    let base = 3 + 3 * d + s; // integer base
    let tidal_mult = if tidal { 1.5 } else { 1.0 };
    let fractal_mult = if fractal { 2.0 } else { 1.0 };
    let scaled = base as f64 * tidal_mult * fractal_mult;
    let floored = scaled.floor();
    if !floored.is_finite() || floored <= 3.0 {
        return 3;
    }
    floored.min(u32::MAX as f64) as u32
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::content::ids::{AchievementId, GeneratorId, HullId, LogId};

    #[test]
    fn test_generator_table_exact() {
        let expected: [(&str, f64, f64, f64); 4] = [
            ("hydrophone", 10.0, 1.15, 0.5),
            ("drifter", 250.0, 1.18, 8.0),
            ("sonar_mast", 20_000.0, 1.20, 120.0),
            ("tidal_core", 2_500_000.0, 1.22, 2500.0),
        ];
        for (i, (name, base, ratio, rate)) in expected.iter().enumerate() {
            let def = &GENERATORS[i];
            assert_eq!(def.id.as_str(), *name);
            assert_eq!(def.base_cost, *base);
            assert_eq!(def.cost_ratio, *ratio);
            assert_eq!(def.base_rate, *rate);
        }
        assert_eq!(
            GENERATORS[1].unlock,
            Unlock::GeneratorsAtLeast(GeneratorId::Hydrophone, 10)
        );
        assert_eq!(
            GENERATORS[2].unlock,
            Unlock::GeneratorsAtLeast(GeneratorId::Drifter, 6)
        );
        assert_eq!(
            GENERATORS[3].unlock,
            Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 5)
        );
    }

    #[test]
    fn test_upgrade_table_exact() {
        let costs = [
            100u32,
            750,
            8_000,
            40_000,
            300_000,
            2_000_000,
            25_000_000,
            150_000_000,
        ];
        for (i, cost) in costs.iter().enumerate() {
            assert_eq!(UPGRADES[i].cost, *cost);
        }
        assert!(matches!(UPGRADES[0].effect, UpgradeEffect::All(f) if f == 2.0));
        assert!(matches!(
            UPGRADES[1].effect,
            UpgradeEffect::Tier(GeneratorId::Hydrophone, f) if f == 3.0
        ));
        assert!(matches!(
            UPGRADES[5].effect,
            UpgradeEffect::Tier(GeneratorId::SonarMast, f) if f == 4.0
        ));
        assert!(matches!(
            UPGRADES[6].effect,
            UpgradeEffect::Tier(GeneratorId::TidalCore, f) if f == 3.0
        ));
        assert!(matches!(UPGRADES[7].effect, UpgradeEffect::All(f) if f == 3.0));
        assert_eq!(
            UPGRADES[0].unlock,
            Unlock::GeneratorsAtLeast(GeneratorId::Drifter, 1)
        );
        assert_eq!(
            UPGRADES[7].unlock,
            Unlock::GeneratorsAtLeast(GeneratorId::SonarMast, 25)
        );
    }

    #[test]
    fn test_hull_table_exact() {
        let costs = [
            1u32, 2, 3, 5, 8, 12, 20, 25, 30, 40, 50, 80, 120, 200, 350, 600, 50000,
        ];
        let prereqs: [Option<HullId>; 17] = [
            None,
            None,
            Some(HullId::BallastRecall),
            Some(HullId::AutoPulse),
            None,
            Some(HullId::DeepMemory),
            Some(HullId::AutoBuyI),
            Some(HullId::BallastRecall),
            None,
            Some(HullId::AutoBuyI),
            Some(HullId::DeepMemory),
            Some(HullId::AutoBuyII),
            Some(HullId::LongStorage),
            Some(HullId::TunedBallast),
            Some(HullId::ExtendedBallast),
            Some(HullId::NullArray),
            Some(HullId::TidalLoop),
        ];
        for (i, def) in HULLS.iter().enumerate() {
            assert_eq!(def.id, HullId::ALL[i]);
            assert_eq!(def.cost, costs[i]);
            assert_eq!(def.prereq, prereqs[i]);
        }
        assert_eq!(hull_def(HullId::DeepMemory).global_mult, Some(1.25));
        assert_eq!(hull_def(HullId::NullArray).global_mult, Some(2.0));
        assert_eq!(hull_def(HullId::DeepField).global_mult, Some(3.0));
        assert_eq!(hull_def(HullId::CoiledHull).pulse_mult, Some(2.0));
        assert_eq!(hull_def(HullId::TidalLoop).fathom_mult, Some(1.5));
        assert_eq!(hull_def(HullId::FractalHull).fathom_mult, Some(2.0));
        for id in HullId::ALL {
            assert!(
                !hull_effect_text(*id).is_empty(),
                "missing effect text for {id}"
            );
        }
    }

    #[test]
    fn test_hull_prereq_acyclic() {
        for start in HullId::ALL {
            let mut current = *start;
            let mut steps = 0;
            while let Some(prereq) = hull_def(current).prereq {
                current = prereq;
                steps += 1;
                assert!(steps <= HULLS.len(), "hull prereq cycle from {start}");
            }
        }
    }

    #[test]
    fn test_unlocks_reference_valid_thresholds() {
        for def in &GENERATORS {
            if let Unlock::GeneratorsAtLeast(_, n) = def.unlock {
                assert!(n > 0);
            }
        }
        for def in &UPGRADES {
            if let Unlock::GeneratorsAtLeast(_, n) = def.unlock {
                assert!(n > 0);
            }
        }
    }

    #[test]
    fn test_achievement_and_log_tables_complete() {
        assert_eq!(ACHIEVEMENTS.len(), AchievementId::ALL.len());
        for (i, def) in ACHIEVEMENTS.iter().enumerate() {
            assert_eq!(def.id, AchievementId::ALL[i]);
            assert!(!def.name.is_empty());
            assert!(!def.description.is_empty());
        }
        assert_eq!(LOGS, LogId::ALL);
    }

    #[test]
    fn test_every_achievement_and_log_has_checker() {
        let state = GameState::new();
        for id in AchievementId::ALL {
            // Must not panic and must be deterministic.
            let first = achievement_reached(*id, &state);
            assert_eq!(first, achievement_reached(*id, &state));
        }
        for id in LogId::ALL {
            let first = log_triggered(*id, &state);
            assert_eq!(first, log_triggered(*id, &state));
        }
        // Boot is always recorded; fresh state satisfies boot.
        assert!(log_triggered(LogId::Boot, &state));
    }

    #[test]
    fn test_offline_caps() {
        let mut hulls = BTreeSet::new();
        assert_eq!(offline_cap_hours(&hulls), 8);
        hulls.insert(HullId::LongStorage);
        assert_eq!(offline_cap_hours(&hulls), 24);
        hulls.insert(HullId::ExtendedBallast);
        assert_eq!(offline_cap_hours(&hulls), 72);
        hulls.insert(HullId::AbyssalReservoir);
        assert_eq!(offline_cap_hours(&hulls), 168);
        // Highest owned endurance hull wins regardless of insertion order.
        let mut only_max = BTreeSet::new();
        only_max.insert(HullId::AbyssalReservoir);
        assert_eq!(offline_cap_hours(&only_max), 168);
    }

    #[test]
    fn test_descent_gain_pinned() {
        let peak = |m: f64, e: i64| Decimal::from_parts(m, e).unwrap();
        assert_eq!(descent_gain(peak(1.0, 6), false, false), 3);
        assert_eq!(
            descent_gain(peak(2.154_434_690_031_884, 6), false, false),
            4
        );
        assert_eq!(
            descent_gain(peak(4.641_588_833_612_779, 6), false, false),
            5
        );
        assert_eq!(descent_gain(peak(1.0, 7), false, false), 6);
        assert_eq!(descent_gain(peak(1.0, 8), false, false), 9);
        assert_eq!(descent_gain(peak(1.0, 9), false, false), 12);
    }

    #[test]
    fn test_descent_gain_with_hulls() {
        let peak = |m: f64, e: i64| Decimal::from_parts(m, e).unwrap();
        // Tidal Loop ×1.5, floored.
        assert_eq!(descent_gain(peak(1.0, 6), true, false), 4); // floor(4.5)
        assert_eq!(descent_gain(peak(1.0, 7), true, false), 9); // floor(9.0)
        // Tidal ×1.5 then Fractal ×2.
        assert_eq!(descent_gain(peak(1.0, 6), true, true), 9);
        assert_eq!(descent_gain(peak(1.0, 9), true, true), 36);
    }

    #[test]
    fn test_descent_gain_below_threshold() {
        let peak = |m: f64, e: i64| Decimal::from_parts(m, e).unwrap();
        assert_eq!(descent_gain(peak(9.9, 5), false, false), 0);
        assert_eq!(descent_gain(Decimal::ZERO, false, false), 0);
    }

    #[test]
    fn test_unlock_met() {
        let mut generators = [0u32; 4];
        assert!(Unlock::Always.met(&generators));
        let unlock = Unlock::GeneratorsAtLeast(GeneratorId::Hydrophone, 10);
        assert!(!unlock.met(&generators));
        generators[GeneratorId::Hydrophone.index()] = 10;
        assert!(unlock.met(&generators));
    }

    #[test]
    fn test_story_texts_present() {
        for id in LogId::ALL {
            assert!(
                !crate::content::story::log_text(*id).is_empty(),
                "empty log text for {id}"
            );
        }
        assert!(!crate::content::story::BOOT_LINES.is_empty());
    }
}
