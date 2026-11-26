//! Derived, presentation-facing computations over [`StateView`].
//!
//! No economy state lives here: every value is recomputed from public
//! accessors and the content tables. This module exists so the renderers and
//! the session share one implementation of names, requirements, contributions,
//! governor validation, bucket progress, and the tutorial hint.

use hadal_core::StateView;
use hadal_core::content::ids::{GeneratorId, HullId, UpgradeId};
use hadal_core::content::tables::{self, Unlock, UpgradeEffect};
use hadal_core::decimal::{to_human_ceil, to_human_floor};
use hadal_core::{BuyQuantity, Decimal};

/// What a Descent preserves (the reset half comes from `view.reset_scope()`).
pub const PERSIST_SCOPE: &[&str] = &[
    "Fathoms",
    "Era Fathoms",
    "Lifetime Fathoms",
    "Hulls",
    "Achievements",
    "Logs",
    "Depth",
];

/// Converts a finite content `f64` into a [`Decimal`].
pub fn dec_f64(value: f64) -> Decimal {
    Decimal::try_from_f64(value).unwrap_or(Decimal::ZERO)
}

// ---------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------

/// Display name for a generator.
pub fn generator_name(id: GeneratorId) -> &'static str {
    match id {
        GeneratorId::Hydrophone => "Hydrophone",
        GeneratorId::Drifter => "Drifter",
        GeneratorId::SonarMast => "Sonar Mast",
        GeneratorId::TidalCore => "Tidal Core",
    }
}

/// Plural display name for a generator.
pub fn generator_plural(id: GeneratorId) -> &'static str {
    match id {
        GeneratorId::Hydrophone => "Hydrophones",
        GeneratorId::Drifter => "Drifters",
        GeneratorId::SonarMast => "Sonar Masts",
        GeneratorId::TidalCore => "Tidal Cores",
    }
}

/// The lowercase buy key for a generator.
pub fn generator_key(id: GeneratorId) -> char {
    match id {
        GeneratorId::Hydrophone => 'h',
        GeneratorId::Drifter => 'd',
        GeneratorId::SonarMast => 'm',
        GeneratorId::TidalCore => 'c',
    }
}

/// Singular/plural name for a generator count.
pub fn generator_count_name(id: GeneratorId, count: u32) -> &'static str {
    if count == 1 {
        generator_name(id)
    } else {
        generator_plural(id)
    }
}

/// Echo/second produced by one unit of a generator tier, including tier and
/// global multipliers. Independent of how many units are owned.
pub fn generator_unit_rate(view: StateView<'_>, id: GeneratorId) -> Decimal {
    let mut tier = Decimal::from_u64(1);
    let mut all = Decimal::from_u64(1);
    for upgrade in UpgradeId::ALL {
        if !view.upgrade_owned(*upgrade) {
            continue;
        }
        match tables::upgrade_def(*upgrade).effect {
            UpgradeEffect::All(factor) => all = all.mul(dec_f64(factor)),
            UpgradeEffect::Tier(target, factor) if target == id => {
                tier = tier.mul(dec_f64(factor));
            }
            UpgradeEffect::Tier(..) => {}
        }
    }
    let def = tables::generator_def(id);
    dec_f64(def.base_rate)
        .mul(tier)
        .mul(all)
        .mul(view.global_multiplier())
}

/// Echo/second produced by all owned units of one generator tier.
/// Mirrors the core `production_rate` term exactly.
pub fn generator_contribution(view: StateView<'_>, id: GeneratorId) -> Decimal {
    let owned = view.generator_owned(id);
    if owned == 0 {
        return Decimal::ZERO;
    }
    generator_unit_rate(view, id).mul(Decimal::from_u64(u64::from(owned)))
}

/// Requirement text for a generator's unlock, if any.
pub fn generator_requirement_text(id: GeneratorId) -> Option<String> {
    unlock_requirement(tables::generator_def(id).unlock)
}

// ---------------------------------------------------------------------
// Signal Chain upgrades
// ---------------------------------------------------------------------

/// Display name for an upgrade.
pub fn upgrade_name(id: UpgradeId) -> &'static str {
    match id {
        UpgradeId::ImpedanceMatch => "Impedance Match",
        UpgradeId::SweepProfile => "Sweep Profile",
        UpgradeId::CascadedGain => "Cascaded Gain",
        UpgradeId::StableDrift => "Stable Drift",
        UpgradeId::CoherentAperture => "Coherent Aperture",
        UpgradeId::MastPhasing => "Mast Phasing",
        UpgradeId::CoreResonance => "Core Resonance",
        UpgradeId::FullSpectrum => "Full Spectrum",
    }
}

/// Effect text derived from the upgrade table.
pub fn upgrade_effect_text(id: UpgradeId, ascii: bool) -> String {
    match tables::upgrade_def(id).effect {
        UpgradeEffect::All(factor) => {
            format!("all production {}{factor}", multiplier_glyph(ascii))
        }
        UpgradeEffect::Tier(target, factor) => {
            format!(
                "{} {}{factor}",
                generator_plural(target),
                multiplier_glyph(ascii)
            )
        }
    }
}

/// Compact effect text for narrow panels, derived from the upgrade table.
pub fn upgrade_effect_compact(id: UpgradeId, ascii: bool) -> String {
    match tables::upgrade_def(id).effect {
        UpgradeEffect::All(factor) => {
            format!("all {}{factor}", multiplier_glyph(ascii))
        }
        UpgradeEffect::Tier(target, factor) => {
            format!(
                "{} {}{factor}",
                generator_name(target),
                multiplier_glyph(ascii)
            )
        }
    }
}

/// Requirement text for an upgrade's unlock, if any.
pub fn upgrade_requirement(id: UpgradeId) -> Option<String> {
    unlock_requirement(tables::upgrade_def(id).unlock)
}

fn multiplier_glyph(ascii: bool) -> &'static str {
    if ascii { "x" } else { "×" }
}

/// Requirement text for an [`Unlock`] condition, if not always available.
pub fn unlock_requirement(unlock: Unlock) -> Option<String> {
    match unlock {
        Unlock::Always => None,
        Unlock::GeneratorsAtLeast(id, count) => {
            Some(format!("{count} {}", generator_count_name(id, count)))
        }
    }
}

// ---------------------------------------------------------------------
// Hull tree
// ---------------------------------------------------------------------

/// Display name for a hull.
pub fn hull_name(id: HullId) -> &'static str {
    match id {
        HullId::AutoPulse => "Auto-Pulse",
        HullId::BallastRecall => "Ballast Recall",
        HullId::TunedBallast => "Tuned Ballast",
        HullId::AutoBuyI => "Auto-Buy I",
        HullId::DeepMemory => "Deep Memory",
        HullId::CoiledHull => "Coiled Hull",
        HullId::AutoBuyII => "Auto-Buy II",
        HullId::BallastGovernor => "Ballast Governor",
        HullId::LongStorage => "Long Storage",
        HullId::ChainAutomator => "Chain Automator",
        HullId::NullArray => "Null Array",
        HullId::AutoBuyIII => "Auto-Buy III",
        HullId::ExtendedBallast => "Extended Ballast",
        HullId::TidalLoop => "Tidal Loop",
        HullId::AbyssalReservoir => "Abyssal Reservoir",
        HullId::DeepField => "Deep Field",
        HullId::FractalHull => "Fractal Hull",
    }
}

/// Exact effect text for a hull, transliterated for ASCII mode.
pub fn hull_effect(id: HullId, ascii: bool) -> String {
    ascii_text(tables::hull_effect_text(id), ascii)
}

/// Unmet prerequisite name for a hull, if it has one.
pub fn hull_requirement(id: HullId) -> Option<&'static str> {
    tables::hull_def(id).prereq.map(hull_name)
}

/// The seventeen hulls grouped under the four DESCENT branches.
///
/// Every [`HullId`] appears exactly once; a unit test enforces it.
pub const HULL_BRANCHES: [(&str, &[HullId]); 4] = [
    (
        "AUTOMATION",
        &[
            HullId::AutoPulse,
            HullId::BallastRecall,
            HullId::TunedBallast,
            HullId::AutoBuyI,
            HullId::AutoBuyII,
            HullId::BallastGovernor,
            HullId::ChainAutomator,
            HullId::AutoBuyIII,
        ],
    ),
    (
        "SIGNAL",
        &[
            HullId::DeepMemory,
            HullId::CoiledHull,
            HullId::NullArray,
            HullId::DeepField,
        ],
    ),
    (
        "ENDURANCE",
        &[
            HullId::LongStorage,
            HullId::ExtendedBallast,
            HullId::AbyssalReservoir,
        ],
    ),
    ("DESCENT", &[HullId::TidalLoop, HullId::FractalHull]),
];

/// All hulls in branch display order.
pub fn flattened_hulls() -> Vec<HullId> {
    HULL_BRANCHES
        .iter()
        .flat_map(|(_, hulls)| hulls.iter().copied())
        .collect()
}

/// Flat focus index of a hull (used by tests to seed focus).
#[cfg(test)]
pub fn hull_flat_index(id: HullId) -> Option<usize> {
    flattened_hulls().iter().position(|hull| *hull == id)
}

// ---------------------------------------------------------------------
// Descent
// ---------------------------------------------------------------------

/// The current bucket's lower bound, the next threshold, and the fraction
/// between them (presentation only).
#[derive(Clone, Copy, Debug)]
pub struct BucketProgress {
    pub next: Decimal,
    pub fraction: f64,
}

/// Computes progress toward the next Descent-gain bucket.
pub fn descent_progress(view: StateView<'_>) -> BucketProgress {
    let peak = view.peak();
    let next = view.next_bucket_threshold();
    let million = Decimal::from_u64(1_000_000);
    let lower = if peak < million {
        Decimal::ZERO
    } else {
        let exponent = peak.exponent10();
        let mantissa = peak.mantissa();
        if mantissa < tables::DESCENT_BUCKET0 {
            Decimal::from_parts(1.0, exponent).unwrap_or(Decimal::ZERO)
        } else if mantissa < tables::DESCENT_BUCKET1 {
            Decimal::from_parts(tables::DESCENT_BUCKET0, exponent).unwrap_or(Decimal::ZERO)
        } else {
            Decimal::from_parts(tables::DESCENT_BUCKET1, exponent).unwrap_or(Decimal::ZERO)
        }
    };
    BucketProgress {
        next,
        fraction: fraction(peak, lower, next),
    }
}

/// `(value - lower) / (upper - lower)` clamped to `0.0..=1.0`.
pub fn fraction(value: Decimal, lower: Decimal, upper: Decimal) -> f64 {
    let span = upper - lower;
    if span.is_zero() {
        return 1.0;
    }
    let offset = value - lower;
    offset
        .div(span)
        .map(decimal_to_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn decimal_to_f64(value: Decimal) -> f64 {
    if value.is_zero() {
        0.0
    } else {
        value.mantissa() * 10.0_f64.powi(value.exponent10() as i32)
    }
}

/// One derived next action for SONAR and HELP.
///
/// The first matching precondition wins; there is never more than one action.
pub fn objective_line(view: StateView<'_>, ascii: bool) -> String {
    let mult = multiplier_glyph(ascii);
    let hydrophone = GeneratorId::Hydrophone;
    let hydrophone_cost = view.generator_cost(hydrophone, BuyQuantity::One);

    if view.can_pulse() && view.pulses_manual() == 0 {
        return "SPACE  pulse the capacitor. A pulse pays 10 s of production.".to_owned();
    }
    if view.generator_owned(hydrophone) == 0 && view.echo() < hydrophone_cost {
        return "SPACE  pulse until you can afford a Hydrophone (10 Echo).".to_owned();
    }
    if view.generator_affordable(hydrophone, BuyQuantity::One) {
        return "h  buy a Hydrophone. It produces while you do not.".to_owned();
    }
    if view.upgrade_count() == 0 && view.upgrade_affordable(UpgradeId::ImpedanceMatch) {
        return format!("u  buy Impedance Match: all production {mult}2 this run.");
    }
    if !view.carrier_matched() {
        return format!(
            "f  tune to CARRIER {:02}. A match is worth {mult}1.5.",
            view.carrier_band() + 1
        );
    }
    let progress = descent_progress(view);
    if !view.can_descend() {
        let pct = (progress.fraction * 100.0).round() as u32;
        return format!(
            "2  DESCENT needs {} Peak Echo. You are at {pct}%.",
            to_human_floor(progress.next)
        );
    }
    if view.can_descend() {
        return format!(
            "2 > p > y  descend for +{} Fathoms and reset the run.",
            view.descent_gain()
        );
    }

    let next_hull = flattened_hulls()
        .into_iter()
        .find(|id| !view.hull_owned(*id));
    match next_hull {
        Some(id) => format!(
            "Next hull  {}: {} Fathoms.",
            hull_name(id),
            view.hull_cost(id)
        ),
        None => "All hulls owned. Keep descending for Era Fathoms.".to_owned(),
    }
}

/// One dim line describing the focused Signal Chain row or hull.
pub fn selection_line(
    view: StateView<'_>,
    focus: crate::render::Focus,
    ascii: bool,
) -> Option<String> {
    match focus {
        crate::render::Focus::Upgrade(index) => {
            let id = *UpgradeId::ALL.get(index)?;
            let effect = upgrade_effect_text(id, ascii);
            let cost = crate::render::cost(view.upgrade_cost(id));
            if view.upgrade_owned(id) {
                Some(format!(
                    "{}: {effect} this run; {cost} Echo (owned).",
                    upgrade_name(id)
                ))
            } else {
                Some(format!(
                    "{}: {effect} this run; costs {cost} Echo.",
                    upgrade_name(id)
                ))
            }
        }
        crate::render::Focus::Hull(index) => {
            let id = *flattened_hulls().get(index)?;
            let effect = hull_effect(id, ascii);
            let effect = effect.trim_end_matches('.').to_owned();
            let cost = view.hull_cost(id);
            let unit = if cost == 1 { "Fathom" } else { "Fathoms" };
            if view.hull_owned(id) {
                Some(format!(
                    "{}: {effect}; {cost} {unit} (owned).",
                    hull_name(id)
                ))
            } else {
                Some(format!("{}: {effect}; costs {cost} {unit}.", hull_name(id)))
            }
        }
        _ => None,
    }
}

/// One dim line describing a generator row.
pub fn generator_selection(view: StateView<'_>, id: GeneratorId) -> String {
    format!(
        "{}: +{}/s each; next unit costs {} Echo.",
        generator_name(id),
        crate::render::rate_text(generator_unit_rate(view, id)),
        to_human_ceil(view.generator_cost(id, BuyQuantity::One))
    )
}

// ---------------------------------------------------------------------
// Ballast Governor
// ---------------------------------------------------------------------

/// Validates a proposed governor target.
///
/// The valid set is exactly `0` or `3..=1_000_000`. Out-of-range proposals are
/// rejected with a readable message and are never silently clamped.
pub fn propose_governor_target(current: u32, delta: i64) -> Result<u32, String> {
    let proposed = i64::from(current) + delta;
    if proposed < 0 {
        return Err("Descent target cannot go below OFF (0).".to_owned());
    }
    if proposed > i64::from(tables::GOVERNOR_TARGET_MAX) {
        return Err(format!(
            "Descent target max is {}.",
            format_count(tables::GOVERNOR_TARGET_MAX)
        ));
    }
    let proposed = proposed as u32;
    if proposed != 0 && proposed < tables::GOVERNOR_TARGET_MIN {
        return Err(format!(
            "Valid targets are OFF or {}-{}.",
            tables::GOVERNOR_TARGET_MIN,
            format_count(tables::GOVERNOR_TARGET_MAX)
        ));
    }
    Ok(proposed)
}

// ---------------------------------------------------------------------
// Text formatting
// ---------------------------------------------------------------------

/// Transliterate non-ASCII presentation glyphs when ASCII mode is active.
pub fn ascii_text(text: &str, ascii: bool) -> String {
    if !ascii {
        return text.to_owned();
    }
    text.chars()
        .map(|c| match c {
            '×' => 'x',
            '·' | '•' => '.',
            '—' | '–' => '-',
            '’' => '\'',
            '“' | '”' => '"',
            '█' | '▀' | '▄' => '#',
            '░' => ':',
            '✓' => '+',
            '✗' => 'x',
            _ => c,
        })
        .collect()
}

/// Thousands-separated integer, e.g. `1,000,000`.
pub fn format_count(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let len = digits.len();
    for (i, byte) in digits.bytes().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(byte as char);
    }
    out
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_branches_cover_every_hull_exactly_once() {
        let flat = flattened_hulls();
        assert_eq!(flat.len(), HullId::ALL.len());
        assert_eq!(flat.len(), 17);
        for id in HullId::ALL {
            let count = flat.iter().filter(|hull| *hull == id).count();
            assert_eq!(count, 1, "{id} appears {count} times");
        }
        assert_eq!(HULL_BRANCHES.len(), 4);
    }

    #[test]
    fn governor_rejects_invalid_without_clamping() {
        assert_eq!(propose_governor_target(0, 10), Ok(10));
        assert_eq!(propose_governor_target(10, -7), Ok(3));
        assert_eq!(propose_governor_target(10, -10), Ok(0));
        assert_eq!(propose_governor_target(3, 0), Ok(3));

        assert!(propose_governor_target(0, 1).is_err());
        assert!(propose_governor_target(0, 2).is_err());
        assert!(propose_governor_target(3, -1).is_err());
        assert!(propose_governor_target(0, -1).is_err());
        assert!(propose_governor_target(1_000_000, 1).is_err());
        assert!(propose_governor_target(999_990, 10).is_ok());
    }

    #[test]
    fn governor_messages_are_readable() {
        let err = propose_governor_target(0, 1).expect_err("invalid");
        assert!(err.contains("OFF or 3"), "{err}");
        let err = propose_governor_target(0, -1).expect_err("invalid");
        assert!(err.contains("OFF"), "{err}");
        let err = propose_governor_target(1_000_000, 100).expect_err("invalid");
        assert!(err.contains("max"), "{err}");
    }

    #[test]
    fn effect_text_derives_from_tables() {
        assert_eq!(
            upgrade_effect_text(UpgradeId::ImpedanceMatch, false),
            "all production ×2"
        );
        assert_eq!(
            upgrade_effect_text(UpgradeId::SweepProfile, false),
            "Hydrophones ×3"
        );
        assert_eq!(
            upgrade_effect_text(UpgradeId::SweepProfile, true),
            "Hydrophones x3"
        );
        assert_eq!(
            upgrade_effect_compact(UpgradeId::ImpedanceMatch, false),
            "all ×2"
        );
        assert_eq!(
            upgrade_effect_compact(UpgradeId::MastPhasing, false),
            "Sonar Mast ×4"
        );
    }

    #[test]
    fn unlock_requirement_text() {
        assert_eq!(
            upgrade_requirement(UpgradeId::ImpedanceMatch),
            Some("1 Drifter".to_owned())
        );
        assert_eq!(
            generator_requirement_text(GeneratorId::Drifter),
            Some("10 Hydrophones".to_owned())
        );
        assert_eq!(generator_requirement_text(GeneratorId::Hydrophone), None);
    }

    #[test]
    fn ascii_transliteration() {
        assert_eq!(
            ascii_text("Global production ×1.25.", true),
            "Global production x1.25."
        );
        assert_eq!(
            ascii_text("Global production ×1.25.", false),
            "Global production ×1.25."
        );
    }

    #[test]
    fn count_formatting() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(7912), "7,912");
        assert_eq!(format_count(1_000_000), "1,000,000");
    }

    fn engine_with(mutate: impl FnOnce(&mut serde_json::Value)) -> hadal_core::Engine {
        let engine = hadal_core::Engine::new();
        let mut value = serde_json::to_value(engine.state()).expect("state json");
        mutate(&mut value);
        let state: hadal_core::GameState = serde_json::from_value(value).expect("state");
        hadal_core::Engine::from_state(state)
    }

    #[test]
    fn objective_line_priority_and_states() {
        let line = |engine: &hadal_core::Engine| objective_line(engine.view(), false);

        // 1. A fresh full capacitor asks for the first pulse.
        assert!(line(&hadal_core::Engine::new()).contains("pulse the capacitor"));

        // 2. No Hydrophone and too little Echo: keep pulsing.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([0, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
        });
        assert!(line(&engine).contains("pulse until you can afford a Hydrophone"));

        // 3. Affordability arrives.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("10");
            value["run"]["generators"] = serde_json::json!([0, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
        });
        assert!(line(&engine).contains("buy a Hydrophone"));

        // 4. Impedance Match is affordable before the carrier.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("100");
            value["run"]["generators"] = serde_json::json!([20, 1, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
        });
        assert!(
            line(&engine).contains("buy Impedance Match"),
            "{}",
            line(&engine)
        );

        // 5. Carrier mismatch.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
            value["run"]["carrier_band"] = serde_json::json!(2);
            value["run"]["tuned_band"] = serde_json::json!(0);
        });
        let mismatch = line(&engine);
        assert!(mismatch.contains("tune to CARRIER 03"), "{mismatch}");
        assert!(mismatch.contains("1.5"));

        // 6. Peak below threshold.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
            value["run"]["carrier_band"] = serde_json::json!(0);
            value["run"]["tuned_band"] = serde_json::json!(0);
            value["run"]["peak_echo"] = serde_json::json!("0");
        });
        let peak = line(&engine);
        assert!(peak.contains("DESCENT needs 1.00M Peak Echo"), "{peak}");
        assert!(peak.contains("You are at 0%"));

        // 7. Descent available.
        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["charge_ticks"] = serde_json::json!(0);
            value["run"]["echo"] = serde_json::json!("0");
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!(["impedance_match"]);
            value["run"]["carrier_band"] = serde_json::json!(0);
            value["run"]["tuned_band"] = serde_json::json!(0);
            value["run"]["peak_echo"] = serde_json::json!("5000000");
        });
        let descent = line(&engine);
        assert!(descent.starts_with("2 > p > y"), "{descent}");
        assert!(descent.contains("descend for +"), "{descent}");

        // ASCII multiplier variant.
        let ascii = objective_line(engine.view(), true);
        assert!(ascii.contains("x1.5") || ascii.contains("descend"));
    }

    #[test]
    fn selection_lines_name_cost_and_effect() {
        let engine = hadal_core::Engine::new();
        let generator = generator_selection(engine.view(), GeneratorId::Hydrophone);
        assert_eq!(
            generator,
            "Hydrophone: +0.75/s each; next unit costs 10 Echo."
        );

        let engine = engine_with(|value| {
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!([]);
            value["run"]["echo"] = serde_json::json!("0");
        });
        let upgrade = selection_line(engine.view(), crate::render::Focus::Upgrade(0), false)
            .expect("upgrade line");
        assert!(upgrade.contains("Impedance Match"));
        assert!(upgrade.contains("×2"));

        let hull =
            selection_line(engine.view(), crate::render::Focus::Hull(0), false).expect("hull line");
        assert!(hull.contains("Auto-Pulse"));
        assert!(hull.contains("1 Fathom"));
    }
}
