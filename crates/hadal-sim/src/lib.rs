//! HADAL simulator — deterministic policies, fixtures, metrics, band assertions.
//!
//! # Design
//!
//! Three simulation profiles run the exact same 10 Hz tick loop with
//! deterministic, policy-driven actions applied *before* each tick:
//!
//! - **active** — pulse when full, tune on carrier change, buy all affordable
//!   upgrades and hulls (cheapest-first), descend immediately when available,
//!   buy highest-ROI generator each second.
//! - **idle** — same purchases/hulls/descents, but **never** pulse and **never**
//!   tune.  Pure passive production — the player is AFK.
//! - **offline** — builds a scripted midgame fixture (all automation hulls,
//!   key upgrades, ~50 era Fathoms), then runs exactly one
//!   `simulate_offline(engine, hours * 3600)` call and reports the
//!   [`OfflineReport`].
//!
//! # Tuning
//!
//! Only constants in `hadal-core::content::tables` may be adjusted to make the
//! bands in [`BANDS`] pass.  Formulas, reset scope, tick order, and layer
//! semantics are fixed.

#![forbid(unsafe_code)]

use hadal_core::content::ids::{GeneratorId, HullId, UpgradeId};
use hadal_core::content::tables;
use hadal_core::engine::{Action, BuyQuantity};
use hadal_core::offline::simulate_offline;
use hadal_core::{Decimal, Engine, OfflineReport, TICKS_PER_SECOND};

/// Descents happen only on check-in boundaries (seconds).  A human player
/// cannot descend every tick; the plan's active policy uses periodic check-ins
/// for longer runs.  Gating to a fixed cadence keeps the era feedback from
/// running away (which would exhaust all content), lets a run build past 1e6 so
/// the Tidal Core (2.5M) and higher descent buckets are reachable (the §9.2
/// "all 4 generator types used by 24 h" band), and puts the first descent at
/// the first check-in at or after descent availability.  This is a simulator
/// policy for long AFK-style runs, NOT a game rule: an online player may
/// descend the instant it is available.
const DESCENT_CHECKIN_SECS: u64 = 300;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which simulation profile to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimProfile {
    Active,
    Idle,
    Offline,
}

impl std::str::FromStr for SimProfile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(SimProfile::Active),
            "idle" => Ok(SimProfile::Idle),
            "offline" => Ok(SimProfile::Offline),
            _ => Err(format!("unknown profile: {s}")),
        }
    }
}

/// A single metric snapshot at a given elapsed-second mark.
#[derive(Debug, Clone)]
pub struct SimMetric {
    pub elapsed_secs: u64,
    pub echo: String,
    pub rate: String,
    pub peak: String,
    pub first_descent_seconds: Option<u64>,
    pub first_availability_seconds: Option<u64>,
    pub descents: u32,
    pub era_fathoms: u32,
    pub lifetime_fathoms: u32,
    pub hulls_owned: usize,
    pub depth_m: u32,
    pub distinct_generators_owned: usize,
    pub unowned_hulls: usize,
    pub unowned_upgrades: usize,
    /// Longest gap (seconds) between purchase/unlock/hull/descent events in
    /// the first 30 minutes.
    pub max_gap_secs: u64,
    /// Whether any anomaly was detected at this checkpoint.
    pub anomaly: Option<String>,
}

/// Result of running one profile for a set of hours.
#[derive(Debug, Clone)]
pub struct SimResult {
    pub profile: SimProfile,
    pub hours: u64,
    pub metrics: Vec<SimMetric>,
    /// For offline: the raw report from `simulate_offline`.
    pub offline_report: Option<OfflineReport>,
    /// For idle: time (seconds) from start to the moment a descent was performed,
    /// if one happened at all.
    pub idle_descent_performed_secs: Option<u64>,
}

/// A pacing band from the plan (§9.2).
#[derive(Debug, Clone)]
pub struct Band {
    pub name: &'static str,
    pub required: String,
    pub measured: String,
    pub pass: bool,
}

// ---------------------------------------------------------------------------
// Simulation runner
// ---------------------------------------------------------------------------

/// Total simulation time in seconds for the given hours.
fn total_secs(hours: u64) -> u64 {
    hours.saturating_mul(3600)
}

/// Checkpoints at which to record metrics: every requested hour plus the
/// first 30 minutes (1800 s).  Deduplicated and sorted.
fn make_checkpoints(hours: u64) -> Vec<u64> {
    let mut pts = Vec::new();
    if hours == 0 {
        return pts;
    }
    pts.push(1800); // 30 minutes
    pts.push(hours * 3600);
    pts.sort();
    pts.dedup();
    pts
}

/// Running tallies kept by the online policies for metrics that span the
/// whole run (not just the checkpoint instant).
#[derive(Default, Clone)]
struct Tracker {
    first_desc_secs: Option<u64>,
    first_avail_secs: Option<u64>,
    /// Generator types ever owned this run.
    ever_gens: [bool; 4],
    /// Largest gap (seconds) between any purchase/unlock/hull/descent event
    /// during the first 30 minutes.
    max_gap_secs: u64,
    /// Elapsed second of the last qualifying event.
    last_event_sec: u64,
    /// Signature of the last observed event-relevant counters.
    last_sig: (u32, u32, usize, usize, usize),
    /// Whether `last_sig` has been initialized.
    sig_init: bool,
}

impl Tracker {
    fn ever_gen_count(&self) -> usize {
        self.ever_gens.iter().filter(|&&b| b).count()
    }

    /// Observe state once per second; record events and gaps.
    fn observe(&mut self, engine: &Engine, elapsed_sec: u64) {
        let view = engine.view();
        for (i, &n) in view.generator_counts().iter().enumerate() {
            if n > 0 {
                self.ever_gens[i] = true;
            }
        }
        let sig = (
            view.purchases(),
            view.descents(),
            view.hull_count(),
            view.upgrade_count(),
            self.ever_gen_count(),
        );
        if self.sig_init && sig != self.last_sig && elapsed_sec <= 1800 {
            let gap = elapsed_sec.saturating_sub(self.last_event_sec);
            if gap > self.max_gap_secs {
                self.max_gap_secs = gap;
            }
            self.last_event_sec = elapsed_sec;
        }
        self.last_sig = sig;
        self.sig_init = true;
    }

    fn note_descent(&mut self, elapsed_sec: u64) {
        if self.first_desc_secs.is_none() {
            self.first_desc_secs = Some(elapsed_sec);
        }
    }

    fn note_availability(&mut self, elapsed_sec: u64) {
        if self.first_avail_secs.is_none() {
            self.first_avail_secs = Some(elapsed_sec);
        }
    }
}

/// Snapshot current engine state into a [`SimMetric`], filling in the
/// whole-run trackers.
fn snapshot(engine: &Engine, elapsed: u64, tracker: &Tracker) -> SimMetric {
    let view = engine.view();
    let hulls_owned = view.hull_count();
    let unowned_hulls = HullId::ALL.len() - hulls_owned;
    let unowned_upgrades = UpgradeId::ALL.len() - view.upgrade_count();

    let anomaly = check_anomaly(engine, elapsed, tracker.max_gap_secs);

    SimMetric {
        elapsed_secs: elapsed,
        echo: format!("{}", view.echo()),
        rate: format!("{}", view.rate()),
        peak: format!("{}", view.peak()),
        first_descent_seconds: tracker.first_desc_secs,
        first_availability_seconds: tracker.first_avail_secs,
        descents: view.descents(),
        era_fathoms: view.era_fathoms(),
        lifetime_fathoms: view.lifetime_fathoms(),
        hulls_owned,
        depth_m: view.depth_m(),
        distinct_generators_owned: tracker.ever_gen_count(),
        unowned_hulls,
        unowned_upgrades,
        max_gap_secs: tracker.max_gap_secs,
        anomaly,
    }
}

/// Run a simulation for the given profile and hour list.
///
/// Returns an empty vec when `hours` is empty (used by `--hours 0`).
pub fn run(profile: SimProfile, hours: &[u64]) -> Vec<SimResult> {
    if hours.is_empty() {
        return Vec::new();
    }

    hours.iter().map(|&h| run_for_hours(profile, h)).collect()
}

fn run_for_hours(profile: SimProfile, hours: u64) -> SimResult {
    match profile {
        SimProfile::Active => run_active(hours),
        SimProfile::Idle => run_idle(hours),
        SimProfile::Offline => run_offline(hours),
    }
}

// ---------------------------------------------------------------------------
// Active policy
// ---------------------------------------------------------------------------

fn run_active(hours: u64) -> SimResult {
    let mut engine = Engine::new();
    let total = total_secs(hours);
    let checkpoints = make_checkpoints(hours);
    let mut metrics = Vec::new();
    let mut cp_idx = 0;
    let mut tracker = Tracker::default();

    for tick in 0..total * TICKS_PER_SECOND {
        let elapsed = tick / TICKS_PER_SECOND;

        // --- Policy actions (before tick) ---
        let (can_pulse, carrier_band, can_descend) = {
            let view = engine.view();
            (view.can_pulse(), view.carrier_band(), view.can_descend())
        };

        // Track descent availability crossing.
        if can_descend {
            tracker.note_availability(elapsed);
        }

        if can_pulse {
            let _ = engine.apply(Action::Pulse);
        }
        buy_all_affordable_upgrades(&mut engine);
        buy_all_affordable_hulls(&mut engine);
        let _ = engine.apply(Action::SetTuning { band: carrier_band });
        let at_checkin = elapsed > 0 && elapsed.is_multiple_of(DESCENT_CHECKIN_SECS);
        if can_descend && at_checkin {
            let _ = engine.apply(Action::Descend);
            tracker.note_descent(elapsed);
        }
        buy_highest_roi_generator(&mut engine);

        // --- Advance one tick ---
        engine.tick();

        // --- Per-second bookkeeping ---
        let now_sec = (tick + 1) / TICKS_PER_SECOND;
        if (tick + 1) % TICKS_PER_SECOND == 0 {
            tracker.observe(&engine, now_sec);
        }
        while cp_idx < checkpoints.len() && checkpoints[cp_idx] <= now_sec {
            metrics.push(snapshot(&engine, checkpoints[cp_idx], &tracker));
            cp_idx += 1;
        }
    }

    SimResult {
        profile: SimProfile::Active,
        hours,
        metrics,
        offline_report: None,
        idle_descent_performed_secs: None,
    }
}

// ---------------------------------------------------------------------------
// Idle policy
// ---------------------------------------------------------------------------

/// Idle policy: same purchases/hulls/descents as active, but **never** pulse
/// and **never** tune.  Pure passive production — the player is AFK.
///
/// This records `first_availability_seconds` (when descent *becomes* available)
/// and `first_descent_performed_seconds` (when the policy actually descends).
fn run_idle(hours: u64) -> SimResult {
    let mut engine = Engine::new();
    let total = total_secs(hours);
    let checkpoints = make_checkpoints(hours);
    let mut metrics = Vec::new();
    let mut cp_idx = 0;
    let mut tracker = Tracker::default();

    for tick in 0..total * TICKS_PER_SECOND {
        let elapsed = tick / TICKS_PER_SECOND;
        let can_descend = engine.view().can_descend();
        if can_descend {
            tracker.note_availability(elapsed);
        }

        buy_all_affordable_upgrades(&mut engine);
        buy_all_affordable_hulls(&mut engine);
        let at_checkin = elapsed > 0 && elapsed.is_multiple_of(DESCENT_CHECKIN_SECS);
        if can_descend && at_checkin {
            let _ = engine.apply(Action::Descend);
            tracker.note_descent(elapsed);
        }
        buy_highest_roi_generator(&mut engine);

        // NOTE: NO pulse, NO tune in idle mode.

        engine.tick();

        let now_sec = (tick + 1) / TICKS_PER_SECOND;
        if (tick + 1) % TICKS_PER_SECOND == 0 {
            tracker.observe(&engine, now_sec);
        }
        while cp_idx < checkpoints.len() && checkpoints[cp_idx] <= now_sec {
            metrics.push(snapshot(&engine, checkpoints[cp_idx], &tracker));
            cp_idx += 1;
        }
    }

    SimResult {
        profile: SimProfile::Idle,
        hours,
        metrics,
        offline_report: None,
        idle_descent_performed_secs: tracker.first_desc_secs,
    }
}

// ---------------------------------------------------------------------------
// Offline policy
// ---------------------------------------------------------------------------

fn run_offline(hours: u64) -> SimResult {
    // Bands: "8 h offline from fresh" exercises the base 8 h cap and the
    // no-double-credit path; longer horizons use the midgame automation fixture.
    let report = if hours <= 8 {
        run_offline_fresh(hours)
    } else {
        build_offline_fixture_and_run(hours)
    };
    let elapsed = hours * 3600;
    let mut metrics = Vec::new();
    metrics.push(SimMetric {
        elapsed_secs: elapsed,
        echo: format!("{}", report.echo_delta),
        rate: "N/A (offline)".into(),
        peak: "N/A (offline)".into(),
        first_descent_seconds: None,
        first_availability_seconds: None,
        descents: report.descents,
        era_fathoms: report.fathoms_gained,
        lifetime_fathoms: report.fathoms_gained,
        hulls_owned: 0,
        depth_m: 0,
        distinct_generators_owned: 0,
        unowned_hulls: 0,
        unowned_upgrades: 0,
        max_gap_secs: 0,
        anomaly: if report.capped_secs > 0 {
            Some(format!(
                "capped at {} s (requested {} s)",
                report.capped_secs, report.requested_secs
            ))
        } else {
            None
        },
    });
    SimResult {
        profile: SimProfile::Offline,
        hours,
        metrics,
        offline_report: Some(report),
        idle_descent_performed_secs: None,
    }
}

/// Build the deterministic midgame fixture used by the offline profiles and
/// the online/offline equivalence check, starting from [`Engine::demo`] and
/// applying public actions only (no private state access).
fn demo_fixture() -> Engine {
    let mut engine = Engine::demo();
    // Ballast Governor target so an offline absence spans multiple descents.
    let _ = engine.apply(Action::SetAutoDescendTarget { fathoms: 10 });
    // Explicitly enable every per-generator auto-buy toggle.
    for id in GeneratorId::ALL {
        let _ = engine.apply(Action::SetAutoBuy {
            id: *id,
            enabled: true,
        });
    }
    engine
}

/// Run `simulate_offline` from a completely fresh engine (base 8 h cap).
fn run_offline_fresh(hours: u64) -> OfflineReport {
    let mut engine = Engine::new();
    simulate_offline(&mut engine, hours * 3600)
}

/// Build the midgame fixture, then run one `simulate_offline`.
fn build_offline_fixture_and_run(hours: u64) -> OfflineReport {
    let mut engine = demo_fixture();
    simulate_offline(&mut engine, hours * 3600)
}

// ---------------------------------------------------------------------------
// Policy helpers
// ---------------------------------------------------------------------------

/// Buy every affordable unowned upgrade, cheapest-first.
fn buy_all_affordable_upgrades(engine: &mut Engine) {
    let mut bought = true;
    while bought {
        bought = false;
        let mut best: Option<(UpgradeId, u32)> = None;
        for id in UpgradeId::ALL {
            if !engine.view().upgrade_affordable(*id) {
                continue;
            }
            let cost = tables::upgrade_def(*id).cost;
            match best {
                None => best = Some((*id, cost)),
                Some((_, bc)) if cost < bc => best = Some((*id, cost)),
                Some(_) => {}
            }
        }
        if let Some((id, _)) = best
            && engine.apply(Action::BuyUpgrade(id)).is_ok()
        {
            bought = true;
        }
    }
}

/// Buy every affordable hull with prereqs met, cheapest-first.
fn buy_all_affordable_hulls(engine: &mut Engine) {
    let mut bought = true;
    while bought {
        bought = false;
        let mut best: Option<(HullId, u32)> = None;
        for id in HullId::ALL {
            if !engine.view().hull_affordable(*id) {
                continue;
            }
            let cost = engine.view().hull_cost(*id);
            match best {
                None => best = Some((*id, cost)),
                Some((_, bc)) if cost < bc => best = Some((*id, cost)),
                Some(_) => {}
            }
        }
        if let Some((id, _)) = best
            && engine.apply(Action::BuyHull(id)).is_ok()
        {
            bought = true;
        }
    }
}

/// Buy one unit of the highest-ROI unlocked generator.
/// ROI = base_rate × tier_multiplier / cost; ties by enum order.
fn buy_highest_roi_generator(engine: &mut Engine) {
    let view = engine.view();
    let mut best: Option<(GeneratorId, Decimal)> = None;
    for id in GeneratorId::ALL {
        if !view.generator_unlocked(*id) {
            continue;
        }
        if !view.generator_affordable(*id, BuyQuantity::One) {
            continue;
        }
        let roi = view.generator_roi(*id);
        match &best {
            None => best = Some((*id, roi)),
            Some((_, br)) if roi > *br => best = Some((*id, roi)),
            Some(_) => {}
        }
    }
    if let Some((id, _)) = best {
        let _ = engine.apply(Action::BuyGenerator {
            id,
            quantity: BuyQuantity::One,
        });
    }
}

// ---------------------------------------------------------------------------
// Anomaly detection
// ---------------------------------------------------------------------------

fn check_anomaly(engine: &Engine, elapsed_secs: u64, max_gap_secs: u64) -> Option<String> {
    let view = engine.view();

    // No purchase/unlock for > 600 s while content remains.
    if elapsed_secs >= 600 && max_gap_secs > 600 {
        return Some(format!("no purchase/unlock for {max_gap_secs} s (> 600 s)"));
    }

    // Rate is zero with no forced state (no generators, no era bonus).
    if view.echo().is_zero() && elapsed_secs > 60 && view.generator_counts().iter().all(|&g| g == 0)
    {
        return Some("rate 0 with zero echo after 60 s".into());
    }

    // Detect overflow sentinels (Decimal saturates at large exponents).
    if view.echo().exponent10() > 300 {
        return Some("echo exponent > 300 (possible overflow)".into());
    }
    if view.peak().exponent10() > 300 {
        return Some("peak exponent > 300 (possible overflow)".into());
    }

    None
}

// ---------------------------------------------------------------------------
// Band assertions (§9.2)
// ---------------------------------------------------------------------------

/// Pacing bands from the plan §9.2.
pub const BANDS: &[BandDef] = &[
    BandDef {
        name: "active first descent (min)",
        profile: SimProfile::Active,
        hours: 1,
        check: BandCheck::FirstDescentSecs(900, 2100), // 15-35 min
    },
    BandDef {
        name: "idle first-descent availability (min)",
        profile: SimProfile::Idle,
        hours: 1,
        check: BandCheck::FirstAvailabilitySecs(1800, 5400), // 30-90 min
    },
    BandDef {
        name: "first 30 min gap ≤ 180 s",
        profile: SimProfile::Active,
        hours: 1,
        check: BandCheck::MaxGapSecs(180),
    },
    BandDef {
        name: "1 h active: ≥ 2 descents",
        profile: SimProfile::Active,
        hours: 1,
        check: BandCheck::MetricGE("descents", 2),
    },
    BandDef {
        name: "1 h active: ≥ 6 era Fathoms",
        profile: SimProfile::Active,
        hours: 1,
        check: BandCheck::MetricGE("era_fathoms", 6),
    },
    BandDef {
        name: "1 h active: ≥ 1 hull",
        profile: SimProfile::Active,
        hours: 1,
        check: BandCheck::MetricGE("hulls_owned", 1),
    },
    BandDef {
        name: "24 h active: ≥ 8 descents",
        profile: SimProfile::Active,
        hours: 24,
        check: BandCheck::MetricGE("descents", 8),
    },
    BandDef {
        name: "24 h active: ≥ 30 era Fathoms",
        profile: SimProfile::Active,
        hours: 24,
        check: BandCheck::MetricGE("era_fathoms", 30),
    },
    BandDef {
        name: "24 h active: all 4 generators used",
        profile: SimProfile::Active,
        hours: 24,
        check: BandCheck::MetricGE("distinct_generators_owned", 4),
    },
    BandDef {
        name: "24 h active: ≥ 6 hulls",
        profile: SimProfile::Active,
        hours: 24,
        check: BandCheck::MetricGE("hulls_owned", 6),
    },
    BandDef {
        name: "24 h active: depth ≥ 9,400 m",
        profile: SimProfile::Active,
        hours: 24,
        check: BandCheck::MetricGE("depth_m", 9400),
    },
    BandDef {
        name: "72 h active: ≥ 20 descents",
        profile: SimProfile::Active,
        hours: 72,
        check: BandCheck::MetricGE("descents", 20),
    },
    BandDef {
        name: "72 h active: ≥ 100 era Fathoms",
        profile: SimProfile::Active,
        hours: 72,
        check: BandCheck::MetricGE("era_fathoms", 100),
    },
    BandDef {
        name: "72 h active: depth ≥ 10,000 m",
        profile: SimProfile::Active,
        hours: 72,
        check: BandCheck::MetricGE("depth_m", 10000),
    },
    BandDef {
        name: "72 h active: ≥ 12 hulls",
        profile: SimProfile::Active,
        hours: 72,
        check: BandCheck::MetricGE("hulls_owned", 12),
    },
    BandDef {
        name: "72 h active: ≥ 1 unowned goal remains",
        profile: SimProfile::Active,
        hours: 72,
        check: BandCheck::MetricGE("unowned_hulls", 1),
    },
    BandDef {
        name: "8 h offline: credits exactly 8 h",
        profile: SimProfile::Offline,
        hours: 8,
        check: BandCheck::OfflineCreditsEqual(8 * 3600),
    },
    BandDef {
        name: "8 h offline: reports cap, no double credit",
        profile: SimProfile::Offline,
        hours: 8,
        check: BandCheck::OfflineFreshIntegrity,
    },
    BandDef {
        name: "72 h offline: credits 72 h",
        profile: SimProfile::Offline,
        hours: 72,
        check: BandCheck::OfflineCreditsEqual(72 * 3600),
    },
    BandDef {
        name: "72 h offline: ≥ 5 descents",
        profile: SimProfile::Offline,
        hours: 72,
        check: BandCheck::OfflineDescentsGE(5),
    },
];

#[derive(Debug, Clone)]
pub struct BandDef {
    pub name: &'static str,
    pub profile: SimProfile,
    pub hours: u64,
    pub check: BandCheck,
}

#[derive(Debug, Clone)]
pub enum BandCheck {
    FirstDescentSecs(u64, u64), // min, max
    FirstAvailabilitySecs(u64, u64),
    MaxGapSecs(u64),
    MetricGE(&'static str, i64),
    OfflineCreditsEqual(u64),
    OfflineFreshIntegrity,
    OfflineDescentsGE(u32),
}

/// Evaluate all bands that correspond to a profile/hour actually present in
/// `results`.  Bands for profiles that were not run are omitted so each
/// `--profile X --assert` invocation only enforces its own bands.
pub fn assert_bands(results: &[SimResult]) -> Vec<Band> {
    BANDS
        .iter()
        .filter(|bd| {
            results
                .iter()
                .any(|r| r.profile == bd.profile && r.hours == bd.hours)
        })
        .map(|bd| evaluate_band(bd, results))
        .collect()
}

fn evaluate_band(bd: &BandDef, results: &[SimResult]) -> Band {
    let result = results
        .iter()
        .find(|r| r.profile == bd.profile && r.hours == bd.hours);
    let measured = match &bd.check {
        BandCheck::FirstDescentSecs(lo, hi) => match result {
            Some(r) => {
                let desc = r.metrics.iter().find_map(|m| m.first_descent_seconds);
                match desc {
                    Some(val) => {
                        let pass = val >= *lo && val <= *hi;
                        (format!("{val} s (range {lo}-{hi} s)"), pass)
                    }
                    None => ("no descent".into(), false),
                }
            }
            None => ("no result".into(), false),
        },
        BandCheck::FirstAvailabilitySecs(lo, hi) => match result {
            Some(r) => {
                let avail = r.metrics.iter().find_map(|m| m.first_availability_seconds);
                match avail {
                    Some(val) => {
                        let pass = val >= *lo && val <= *hi;
                        (format!("{val} s (range {lo}-{hi} s)"), pass)
                    }
                    None => ("never reached 1e6".into(), false),
                }
            }
            None => ("no result".into(), false),
        },
        BandCheck::MaxGapSecs(max_gap) => match result {
            Some(r) => {
                let gap = r.metrics.iter().map(|m| m.max_gap_secs).max().unwrap_or(0);
                let pass = gap <= *max_gap;
                (format!("{gap} s (max {max_gap} s)"), pass)
            }
            None => ("no result".into(), false),
        },
        BandCheck::MetricGE(field, min) => match result {
            Some(r) => {
                let val = match *field {
                    "descents" => r.metrics.last().map(|m| m.descents as i64).unwrap_or(0),
                    "era_fathoms" => r.metrics.last().map(|m| m.era_fathoms as i64).unwrap_or(0),
                    "hulls_owned" => r.metrics.last().map(|m| m.hulls_owned as i64).unwrap_or(0),
                    "depth_m" => r.metrics.last().map(|m| m.depth_m as i64).unwrap_or(0),
                    "distinct_generators_owned" => r
                        .metrics
                        .last()
                        .map(|m| m.distinct_generators_owned as i64)
                        .unwrap_or(0),
                    "unowned_hulls" => r
                        .metrics
                        .last()
                        .map(|m| m.unowned_hulls as i64)
                        .unwrap_or(0),
                    _ => 0,
                };
                let pass = val >= *min;
                (format!("{val} (min {min})"), pass)
            }
            None => ("no result".into(), false),
        },
        BandCheck::OfflineCreditsEqual(expected) => match result {
            Some(r) => {
                let report = r.offline_report.as_ref().unwrap();
                let pass = report.credited_secs == *expected;
                (
                    format!(
                        "{} s credited (expected {} s)",
                        report.credited_secs, expected
                    ),
                    pass,
                )
            }
            None => ("no result".into(), false),
        },
        BandCheck::OfflineFreshIntegrity => match result {
            Some(r) => {
                let report = r.offline_report.as_ref().unwrap();
                let cap = 8 * 3600;
                let pass = report.cap_secs == cap
                    && report.credited_secs == cap
                    && report.capped_secs == 0
                    && report.ticks == cap * TICKS_PER_SECOND;
                (
                    format!(
                        "cap={} credited={} capped={} ticks={}",
                        report.cap_secs, report.credited_secs, report.capped_secs, report.ticks
                    ),
                    pass,
                )
            }
            None => ("no result".into(), false),
        },
        BandCheck::OfflineDescentsGE(min) => match result {
            Some(r) => {
                let report = r.offline_report.as_ref().unwrap();
                let pass = report.descents >= *min;
                (format!("{} descents (min {})", report.descents, min), pass)
            }
            None => ("no result".into(), false),
        },
    };
    let (measured_str, pass_val) = measured;
    Band {
        name: bd.name,
        required: bd.check.description(),
        measured: measured_str,
        pass: pass_val,
    }
}

impl BandCheck {
    fn description(&self) -> String {
        match self {
            BandCheck::FirstDescentSecs(lo, hi) => {
                format!("{lo}–{hi} s (15–35 min)")
            }
            BandCheck::FirstAvailabilitySecs(lo, hi) => {
                format!("{lo}–{hi} s (30–90 min)")
            }
            BandCheck::MaxGapSecs(max) => {
                format!("≤ {max} s")
            }
            BandCheck::MetricGE(field, min) => {
                format!("≥ {min} {field}")
            }
            BandCheck::OfflineCreditsEqual(expected) => {
                format!("= {} s", expected)
            }
            BandCheck::OfflineFreshIntegrity => {
                "cap=8 h, credited=8 h, no double credit".to_string()
            }
            BandCheck::OfflineDescentsGE(min) => {
                format!("≥ {min} descents")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Online/offline equivalence
// ---------------------------------------------------------------------------

/// Check that 8 h of online ticks with the same automation yields the same
/// state hash as 8 h of offline simulation.  Both start from the identical
/// demo fixture; the online engine runs `advance()` directly, the offline
/// engine runs `simulate_offline()` (which itself calls `advance()` with the
/// same automation).
pub fn check_online_offline_equivalence() -> Band {
    let secs = 8 * 3600;

    let mut online = demo_fixture();
    online.advance(secs * TICKS_PER_SECOND);

    let mut offline = demo_fixture();
    let report = simulate_offline(&mut offline, secs);

    let hash_online = online.state_hash();
    let hash_offline = offline.state_hash();
    let pass = hash_online == hash_offline && report.credited_secs == secs;
    Band {
        name: "online/offline 8 h equivalence",
        required: "identical state hash".to_string(),
        measured: format!(
            "online={}, offline={}, match={}",
            &hash_online[..16.min(hash_online.len())],
            &hash_offline[..16.min(hash_offline.len())],
            pass
        ),
        pass,
    }
}

// ---------------------------------------------------------------------------
// Clock skew tests
// ---------------------------------------------------------------------------

/// Test clock skew edge cases: negative gap credits 0, 30-day gap capped.
pub fn check_clock_skew() -> Vec<Band> {
    let mut bands = Vec::new();

    // Negative gap → 0 credited.
    {
        let mut engine = Engine::new();
        let report = simulate_offline(&mut engine, 0);
        bands.push(Band {
            name: "clock skew: negative gap → 0 credited",
            required: "credited_secs == 0".to_string(),
            measured: format!("credited={} s", report.credited_secs),
            pass: report.credited_secs == 0,
        });
    }

    // 30-day gap → capped at the base 8 h cap, no panic.
    {
        let mut engine = Engine::new();
        let report = simulate_offline(&mut engine, 30 * 86400);
        let base_cap = 8 * 3600;
        let pass = report.cap_hit
            && report.capped_secs > 0
            && report.credited_secs == base_cap
            && report.cap_secs == base_cap;
        bands.push(Band {
            name: "clock skew: 30-day gap capped",
            required: "credited == base 8 h cap, cap_hit".to_string(),
            measured: format!(
                "credited={} s, cap={} s, capped={} s, hit={}",
                report.credited_secs, report.cap_secs, report.capped_secs, report.cap_hit
            ),
            pass,
        });
    }

    bands
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Print a per-band PASS/FAIL table.
pub fn print_bands(bands: &[Band]) {
    let mut total_pass = 0;
    let mut total_fail = 0;

    println!("\n{}", "=".repeat(50));
    println!("PACING BAND RESULTS");
    println!("{}", "=".repeat(50));
    for b in bands {
        let status = if b.pass { "PASS" } else { "FAIL" };
        if b.pass {
            total_pass += 1;
        } else {
            total_fail += 1;
        }
        println!("  [{:>4}] {:<35} {}", status, b.name, b.measured);
    }
    println!("{}", "=".repeat(50));
    println!(
        "Summary: {} passed, {} failed — {}",
        total_pass,
        total_fail,
        if total_fail == 0 {
            String::from("pacing: all targets met")
        } else {
            format!("pacing: {total_fail} bands failed")
        }
    );
    println!("{}\n", "=".repeat(50));
}

/// Print metrics for a single SimResult.
pub fn print_result(result: &SimResult) {
    println!("\n{} {} h", result.profile, result.hours);
    println!("{}", "-".repeat(40));
    for m in &result.metrics {
        println!(
            "  {:>6}s  echo={}  rate={}  peak={}  descents={}  era={}  hulls={}  depth={}m  gens={}  unowned_h={}  unowned_u={}",
            m.elapsed_secs,
            m.echo,
            m.rate,
            m.peak,
            m.descents,
            m.era_fathoms,
            m.hulls_owned,
            m.depth_m,
            m.distinct_generators_owned,
            m.unowned_hulls,
            m.unowned_upgrades,
        );
        if let Some(ref anomaly) = m.anomaly {
            println!("    ⚠ {}", anomaly);
        }
    }
    if let Some(ref report) = result.offline_report {
        println!("  Offline report:");
        println!(
            "    requested={} s  credited={} s  capped={} s",
            report.requested_secs, report.credited_secs, report.capped_secs
        );
        println!(
            "    cap_secs={}  cap_hit={}  ticks={}",
            report.cap_secs, report.cap_hit, report.ticks
        );
        println!(
            "    echo_delta={}  purchases={}  descents={}",
            report.echo_delta, report.purchases, report.descents
        );
        println!(
            "    fathoms_gained={}  achievements={:?}",
            report.fathoms_gained, report.achievements
        );
    }
    if let Some(secs) = result.idle_descent_performed_secs {
        println!("  Idle descent performed at {} s", secs);
    }
}

impl std::fmt::Display for SimProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimProfile::Active => write!(f, "active"),
            SimProfile::Idle => write!(f, "idle"),
            SimProfile::Offline => write!(f, "offline"),
        }
    }
}

// ---------------------------------------------------------------------------
// JSON / CSV output
// ---------------------------------------------------------------------------

/// Serialize results to JSON.
pub fn to_json(results: &[SimResult]) -> String {
    let mut objects = Vec::new();
    for r in results {
        let mut obj = serde_json::Map::new();
        obj.insert("profile".into(), r.profile.to_string().into());
        obj.insert("hours".into(), r.hours.into());

        let metrics: Vec<serde_json::Value> = r
            .metrics
            .iter()
            .map(|m| {
                let mut o = serde_json::Map::new();
                o.insert("elapsed_secs".into(), m.elapsed_secs.into());
                o.insert("echo".into(), m.echo.clone().into());
                o.insert("rate".into(), m.rate.clone().into());
                o.insert("peak".into(), m.peak.clone().into());
                o.insert("descents".into(), m.descents.into());
                o.insert("era_fathoms".into(), m.era_fathoms.into());
                o.insert("hulls_owned".into(), m.hulls_owned.into());
                o.insert("depth_m".into(), m.depth_m.into());
                o.insert(
                    "distinct_generators_owned".into(),
                    m.distinct_generators_owned.into(),
                );
                o.insert("unowned_hulls".into(), m.unowned_hulls.into());
                o.insert("unowned_upgrades".into(), m.unowned_upgrades.into());
                if let Some(ref a) = m.anomaly {
                    o.insert("anomaly".into(), a.clone().into());
                }
                serde_json::Value::Object(o)
            })
            .collect();
        obj.insert("metrics".into(), serde_json::Value::Array(metrics));

        if let Some(ref report) = r.offline_report {
            let mut o = serde_json::Map::new();
            o.insert("requested_secs".into(), report.requested_secs.into());
            o.insert("credited_secs".into(), report.credited_secs.into());
            o.insert("capped_secs".into(), report.capped_secs.into());
            o.insert("cap_secs".into(), report.cap_secs.into());
            o.insert("cap_hit".into(), report.cap_hit.into());
            o.insert("ticks".into(), report.ticks.into());
            o.insert("echo_delta".into(), format!("{}", report.echo_delta).into());
            o.insert("purchases".into(), report.purchases.into());
            o.insert("descents".into(), report.descents.into());
            o.insert("fathoms_gained".into(), report.fathoms_gained.into());
            obj.insert("offline_report".into(), serde_json::Value::Object(o));
        }

        if let Some(secs) = r.idle_descent_performed_secs {
            obj.insert("idle_descent_performed_secs".into(), secs.into());
        }

        objects.push(serde_json::Value::Object(obj));
    }
    serde_json::to_string_pretty(&objects).unwrap_or_default()
}

/// Serialize results to CSV.
pub fn to_csv(results: &[SimResult]) -> String {
    let mut lines = Vec::new();
    lines.push("profile,hours,elapsed_secs,echo,rate,peak,descents,era_fathoms,hulls_owned,depth_m,distinct_generators_owned,unowned_hulls,unowned_upgrades,anomaly".into());
    for r in results {
        for m in &r.metrics {
            let anomaly = m.anomaly.as_deref().unwrap_or("");
            lines.push(format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                r.profile,
                r.hours,
                m.elapsed_secs,
                m.echo,
                m.rate,
                m.peak,
                m.descents,
                m.era_fathoms,
                m.hulls_owned,
                m.depth_m,
                m.distinct_generators_owned,
                m.unowned_hulls,
                m.unowned_upgrades,
                anomaly,
            ));
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn profile_parsing() {
        assert_eq!(SimProfile::from_str("active").unwrap(), SimProfile::Active);
        assert_eq!(SimProfile::from_str("idle").unwrap(), SimProfile::Idle);
        assert_eq!(
            SimProfile::from_str("offline").unwrap(),
            SimProfile::Offline
        );
        assert!(SimProfile::from_str("bogus").is_err());
    }

    #[test]
    fn empty_hours_yields_empty_result() {
        assert!(run(SimProfile::Active, &[]).is_empty());
        assert!(run(SimProfile::Offline, &[]).is_empty());
    }

    #[test]
    fn active_first_descent_in_band() {
        let results = run(SimProfile::Active, &[1]);
        let first = results[0]
            .metrics
            .iter()
            .find_map(|m| m.first_descent_seconds)
            .expect("a descent should happen within the first hour");
        assert!(
            (900..=2100).contains(&first),
            "first active descent at {first} s is outside 15–35 min"
        );
    }

    #[test]
    fn idle_never_pulses_but_uses_passive_only() {
        // Idle availability must be materially later than active's first
        // descent, which is the documented idle interpretation.
        let active = run(SimProfile::Active, &[1]);
        let idle = run(SimProfile::Idle, &[1]);
        let a = active[0]
            .metrics
            .iter()
            .find_map(|m| m.first_descent_seconds)
            .unwrap();
        let i = idle[0]
            .metrics
            .iter()
            .find_map(|m| m.first_availability_seconds)
            .expect("idle should eventually reach 1e6");
        assert!(
            i > a,
            "idle availability {i} should be later than active {a}"
        );
    }

    #[test]
    fn offline_fresh_credits_full_cap_once() {
        let results = run(SimProfile::Offline, &[8]);
        let report = results[0].offline_report.as_ref().unwrap();
        assert_eq!(report.credited_secs, 8 * 3600);
        assert_eq!(report.capped_secs, 0);
        assert_eq!(report.cap_secs, 8 * 3600);
        assert_eq!(report.ticks, 8 * 3600 * TICKS_PER_SECOND);
    }
    #[test]
    fn offline_metric_era_fathoms_matches_report() {
        // C4 fix: the era_fathoms metric must reflect the real fathoms
        // gained during the offline run, not a hard-coded zero.
        let results = run(SimProfile::Offline, &[168]);
        let metric = &results[0].metrics[0];
        let report = results[0].offline_report.as_ref().unwrap();
        assert_eq!(
            metric.era_fathoms, report.fathoms_gained,
            "era_fathoms metric must equal fathoms_gained from offline report"
        );
        assert_ne!(
            metric.era_fathoms, 0,
            "168h offline should gain era fathoms"
        );
    }

    #[test]
    fn serializers_emit_profile_rows() {
        let results = run(SimProfile::Active, &[1]);
        let json = to_json(&results);
        let csv = to_csv(&results);
        assert!(json.contains("\"profile\""));
        assert!(csv.starts_with("profile,hours"));
        assert!(csv.contains("active,1,"));
    }

    #[test]
    fn online_offline_equivalence_holds() {
        let band = check_online_offline_equivalence();
        assert!(band.pass, "equivalence failed: {}", band.measured);
    }

    #[test]
    fn clock_skew_bands_hold() {
        for band in check_clock_skew() {
            assert!(band.pass, "{} failed: {}", band.name, band.measured);
        }
    }
}
