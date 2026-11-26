//! Offline progress: the exact online tick, capped, with no manual actions.

use std::collections::BTreeSet;

use crate::TICKS_PER_SECOND;
use crate::content::ids::AchievementId;
use crate::content::tables;
use crate::decimal::Decimal;
use crate::engine::Engine;

/// Summary of an offline catch-up.
#[derive(Debug, Clone, PartialEq)]
pub struct OfflineReport {
    /// Seconds the caller asked to credit.
    pub requested_secs: u64,
    /// Seconds actually credited (min of requested and cap).
    pub credited_secs: u64,
    /// Seconds dropped by the cap.
    pub capped_secs: u64,
    /// Effective cap in seconds.
    pub cap_secs: u64,
    /// Whether the cap was hit.
    pub cap_hit: bool,
    /// Ticks simulated.
    pub ticks: u64,
    /// Change in Echo balance.
    pub echo_delta: Decimal,
    /// Purchase actions performed by automation.
    pub purchases: u32,
    /// Descents performed by the governor.
    pub descents: u32,
    /// Era Fathoms gained (Fathoms granted by descents).
    pub fathoms_gained: u32,
    /// Achievements unlocked during the catch-up (sorted).
    pub achievements: Vec<AchievementId>,
}

/// Simulates `elapsed_secs` of offline progress by running the exact online
/// tick with automation only.
///
/// `elapsed_secs == 0` credits nothing. The caller must clamp negative clock
/// jumps to zero and save immediately after this returns.
pub fn simulate_offline(engine: &mut Engine, elapsed_secs: u64) -> OfflineReport {
    let cap_secs = u64::from(tables::offline_cap_hours(&engine.state.meta.hulls)) * 3600;
    let credited_secs = elapsed_secs.min(cap_secs);
    let capped_secs = elapsed_secs - credited_secs;
    let ticks = credited_secs.saturating_mul(TICKS_PER_SECOND);

    let before_echo = engine.state.run.echo;
    let before_purchases = engine.state.meta.stats.purchases;
    let before_descents = engine.state.meta.stats.descents;
    let before_era = engine.state.meta.era_fathoms;
    let before_achievements: BTreeSet<AchievementId> =
        engine.state.meta.achievements.iter().copied().collect();

    if ticks > 0 {
        engine.advance(ticks);
    }

    let after_achievements: BTreeSet<AchievementId> =
        engine.state.meta.achievements.iter().copied().collect();
    let achievements = after_achievements
        .difference(&before_achievements)
        .copied()
        .collect();

    OfflineReport {
        requested_secs: elapsed_secs,
        credited_secs,
        capped_secs,
        cap_secs,
        cap_hit: capped_secs > 0,
        ticks,
        echo_delta: engine.state.run.echo.sub(before_echo),
        purchases: engine
            .state
            .meta
            .stats
            .purchases
            .saturating_sub(before_purchases),
        descents: engine
            .state
            .meta
            .stats
            .descents
            .saturating_sub(before_descents),
        fathoms_gained: engine.state.meta.era_fathoms.saturating_sub(before_era),
        achievements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ids::HullId;
    use crate::engine::Engine;

    fn automated(hulls: &[HullId], governor: u32) -> Engine {
        let mut engine = Engine::new();
        for hull in hulls {
            engine.state.meta.hulls.insert(*hull);
        }
        engine.state.meta.auto_buy = [true; 4];
        engine.state.meta.auto_descend_at = governor;
        engine
    }

    fn core_automation() -> Vec<HullId> {
        vec![
            HullId::AutoPulse,
            HullId::TunedBallast,
            HullId::AutoBuyI,
            HullId::AutoBuyII,
            HullId::AutoBuyIII,
            HullId::ChainAutomator,
            HullId::BallastGovernor,
        ]
    }

    #[test]
    fn test_offline_caps_by_endurance_hull() {
        let cases: [(&[HullId], u64); 4] = [
            (&[], 8),
            (&[HullId::LongStorage], 24),
            (&[HullId::LongStorage, HullId::ExtendedBallast], 72),
            (&[HullId::AbyssalReservoir], 168),
        ];
        for (hulls, hours) in cases {
            let mut engine = automated(hulls, 0);
            let report = simulate_offline(&mut engine, 0);
            assert_eq!(report.cap_secs, hours * 3600, "{hulls:?}");
            assert_eq!(report.credited_secs, 0);
            assert!(!report.cap_hit);
        }
    }

    #[test]
    fn test_offline_zero_and_clock_skew() {
        let mut engine = automated(&core_automation(), 5);
        let before = engine.state_hash();
        let report = simulate_offline(&mut engine, 0);
        assert_eq!(engine.state_hash(), before);
        assert_eq!(report.requested_secs, 0);
        assert_eq!(report.credited_secs, 0);
        assert_eq!(report.ticks, 0);
        assert!(report.echo_delta.is_zero());
        assert!(report.achievements.is_empty());
        assert!(!report.cap_hit);
    }

    #[test]
    fn test_offline_30_day_capped_to_base() {
        let mut engine = automated(&core_automation(), 5);
        let thirty_days = 30 * 86_400;
        let report = simulate_offline(&mut engine, thirty_days);
        assert_eq!(report.cap_secs, 8 * 3600);
        assert_eq!(report.credited_secs, 8 * 3600);
        assert_eq!(report.capped_secs, thirty_days - 8 * 3600);
        assert!(report.cap_hit);
        assert_eq!(report.ticks, 8 * 3600 * TICKS_PER_SECOND);
    }

    #[test]
    fn test_offline_equals_online_8h() {
        let hulls = core_automation();
        let mut online = automated(&hulls, 5);
        online.advance(8 * 3600 * TICKS_PER_SECOND);
        let mut offline = automated(&hulls, 5);
        let report = simulate_offline(&mut offline, 8 * 3600);
        assert_eq!(report.credited_secs, 8 * 3600);
        assert!(!report.cap_hit, "exactly at cap is not capped");
        assert_eq!(online.state_hash(), offline.state_hash());
    }

    #[test]
    fn test_offline_168h_report_aggregates() {
        let mut hulls = core_automation();
        hulls.push(HullId::AbyssalReservoir);
        hulls.push(HullId::TidalLoop);
        hulls.push(HullId::FractalHull);
        let mut engine = automated(&hulls, 5);
        let report = simulate_offline(&mut engine, 168 * 3600);
        assert_eq!(report.cap_secs, 168 * 3600);
        assert_eq!(report.credited_secs, 168 * 3600);
        assert!(!report.cap_hit);
        assert_eq!(report.ticks, 6_048_000);
        assert!(report.purchases > 0, "automation purchases should occur");
        assert!(report.descents > 0, "governor should descend");
        // All Decimal values are finite by construction; assert bounds.
        assert!(report.echo_delta.mantissa().is_finite());
        assert!(report.echo_delta.exponent10() <= i64::MAX / 2);
        assert!(!report.achievements.is_empty());
    }
}
