//! Serializable game state.
//!
//! Every field is `pub(crate)`; the UI mutates state only through `Engine`
//! actions and reads through `StateView`. There are **no `f64` fields** here;
//! all economy values are [`Decimal`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::content::ids::{AchievementId, HullId, LogId, UpgradeId};
use crate::content::tables::{BASE_DEPTH_M, PULSE_CAPACITOR_TICKS};
use crate::decimal::Decimal;

/// Lifetime and cross-run statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stats {
    /// Total Echo ever produced: passive production plus manual and auto
    /// pulse yields. Purchases do not reduce it.
    pub(crate) lifetime_echo: Decimal,
    pub(crate) best_peak: Decimal,
    pub(crate) pulses_manual: u32,
    pub(crate) pulses_auto: u32,
    /// Number of purchase *actions* performed. A Ten or Max buy counts once.
    pub(crate) purchases: u32,
    pub(crate) descents: u32,
    pub(crate) total_ticks: u64,
    pub(crate) bands_matched: u32,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            lifetime_echo: Decimal::ZERO,
            best_peak: Decimal::ZERO,
            pulses_manual: 0,
            pulses_auto: 0,
            purchases: 0,
            descents: 0,
            total_ticks: 0,
            bands_matched: 0,
        }
    }
}

/// Persistent tutorial progress.
///
/// `Default` is the legacy-safe value: an existing save with no tutorial data
/// is treated as completed. New games use [`TutorialState::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TutorialState {
    pub(crate) step: u8,
    pub(crate) completed: bool,
    pub(crate) skipped: bool,
    pub(crate) seen_carrier_change: bool,
    pub(crate) tuned_since_change: bool,
    pub(crate) manual_opened: bool,
    pub(crate) manual_closed: bool,
    pub(crate) system_opened: bool,
    pub(crate) acknowledged: bool,
}

impl Default for TutorialState {
    fn default() -> Self {
        Self {
            step: 0,
            completed: true,
            skipped: false,
            seen_carrier_change: false,
            tuned_since_change: false,
            manual_opened: false,
            manual_closed: false,
            system_opened: false,
            acknowledged: false,
        }
    }
}

impl TutorialState {
    /// A fresh, active tutorial at the first step.
    pub fn new() -> Self {
        Self {
            completed: false,
            ..Self::default()
        }
    }
}

/// Persistent meta state that survives Descent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaState {
    pub(crate) fathoms: u32,
    pub(crate) era_fathoms: u32,
    pub(crate) lifetime_fathoms: u32,
    pub(crate) depth_m: u32,
    pub(crate) auto_descend_at: u32,
    pub(crate) hulls: BTreeSet<HullId>,
    pub(crate) auto_buy: [bool; 4],
    pub(crate) achievements: BTreeSet<AchievementId>,
    pub(crate) logs: BTreeSet<LogId>,
    /// Which carrier bands have ever been matched by a tuning action.
    pub(crate) bands_seen: [bool; 6],
    #[serde(default)]
    pub(crate) tutorial: TutorialState,
    pub(crate) stats: Stats,
}

/// Per-run state that resets on Descent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunState {
    pub(crate) echo: Decimal,
    pub(crate) peak_echo: Decimal,
    pub(crate) generators: [u32; 4],
    pub(crate) upgrades: BTreeSet<UpgradeId>,
    pub(crate) charge_ticks: u16,
    pub(crate) carrier_band: u8,
    pub(crate) tuned_band: u8,
    pub(crate) carrier_ticks: u16,
    pub(crate) run_ticks: u64,
}

impl RunState {
    /// Fresh run state: full capacitor, tuned/carrier at band 0, no generators.
    pub(crate) fn fresh() -> Self {
        Self {
            echo: Decimal::ZERO,
            peak_echo: Decimal::ZERO,
            generators: [0; 4],
            upgrades: BTreeSet::new(),
            charge_ticks: PULSE_CAPACITOR_TICKS,
            carrier_band: 0,
            tuned_band: 0,
            carrier_ticks: 0,
            run_ticks: 0,
        }
    }
}

/// The complete game state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameState {
    pub(crate) meta: MetaState,
    pub(crate) run: RunState,
}

impl GameState {
    /// Creates a fresh game state at the start of a new save.
    pub fn new() -> Self {
        let mut logs = BTreeSet::new();
        logs.insert(LogId::Boot);
        Self {
            meta: MetaState {
                fathoms: 0,
                era_fathoms: 0,
                lifetime_fathoms: 0,
                depth_m: BASE_DEPTH_M,
                auto_descend_at: 0,
                hulls: BTreeSet::new(),
                auto_buy: [true; 4],
                achievements: BTreeSet::new(),
                logs,
                bands_seen: [false; 6],
                tutorial: TutorialState::new(),
                stats: Stats::default(),
            },
            run: RunState::fresh(),
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ids::LogId;

    #[test]
    fn test_fresh_state_defaults() {
        let state = GameState::new();
        assert_eq!(state.meta.fathoms, 0);
        assert_eq!(state.meta.era_fathoms, 0);
        assert_eq!(state.meta.lifetime_fathoms, 0);
        assert_eq!(state.meta.depth_m, BASE_DEPTH_M);
        assert_eq!(state.meta.auto_descend_at, 0);
        assert!(state.meta.hulls.is_empty());
        assert_eq!(state.meta.auto_buy, [true; 4]);
        assert!(state.meta.achievements.is_empty());
        assert!(state.meta.logs.contains(&LogId::Boot));
        assert_eq!(state.meta.bands_seen, [false; 6]);
        assert_eq!(state.meta.stats, Stats::default());

        assert!(state.run.echo.is_zero());
        assert!(state.run.peak_echo.is_zero());
        assert_eq!(state.run.generators, [0; 4]);
        assert!(state.run.upgrades.is_empty());
        assert_eq!(state.run.charge_ticks, PULSE_CAPACITOR_TICKS);
        assert_eq!(state.run.carrier_band, 0);
        assert_eq!(state.run.tuned_band, 0);
        assert_eq!(state.run.carrier_ticks, 0);
        assert_eq!(state.run.run_ticks, 0);
    }

    #[test]
    fn test_state_serde_roundtrip() {
        let mut state = GameState::new();
        state.meta.fathoms = 42;
        state.meta.hulls.insert(HullId::DeepMemory);
        state.meta.achievements.insert(AchievementId::FirstContact);
        state.run.echo = Decimal::from_u64(1_234_567);
        state.run.upgrades.insert(UpgradeId::ImpedanceMatch);
        state.run.generators = [12, 4, 1, 0];

        let json = serde_json::to_string(&state).unwrap();
        let back: GameState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }

    #[test]
    fn test_state_rejects_unknown_fields() {
        let state = GameState::new();
        let mut value = serde_json::to_value(&state).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), serde_json::json!(1));
        assert!(serde_json::from_value::<GameState>(value).is_err());

        // Nested unknown field in meta must also be rejected.
        let state = GameState::new();
        let mut value = serde_json::to_value(&state).unwrap();
        value["meta"]
            .as_object_mut()
            .unwrap()
            .insert("bogus".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<GameState>(value).is_err());
    }

    #[test]
    fn test_tutorial_legacy_and_active_defaults() {
        let legacy = TutorialState::default();
        assert!(legacy.completed, "default must be legacy-safe/completed");
        assert_eq!(legacy.step, 0);
        assert!(!legacy.skipped);
        assert!(!legacy.acknowledged);
        assert!(!legacy.tuned_since_change);

        let active = TutorialState::new();
        assert!(!active.completed);
        assert!(!active.skipped);
        assert_eq!(active.step, 0);
        assert!(!active.seen_carrier_change);
        assert!(!active.tuned_since_change);

        assert_eq!(GameState::new().meta.tutorial, active);
    }
}
