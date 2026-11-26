//! Derived, presentation-facing computations over [`StateView`].
//!
//! No economy state lives here: every value is recomputed from public
//! accessors and the content tables. This module exists so the renderers and
//! the session share one implementation of names, requirements, contributions,
//! governor validation, bucket progress, and the tutorial hint.

use hadal_core::Decimal;
use hadal_core::StateView;
use hadal_core::content::ids::{GeneratorId, HullId, UpgradeId};
use hadal_core::content::tables::{self, Unlock, UpgradeEffect};
use hadal_core::decimal::to_human_floor;

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

/// Echo/second produced by one generator tier, including tier + global
/// multipliers. Mirrors the core `production_rate` term exactly.
pub fn generator_contribution(view: StateView<'_>, id: GeneratorId) -> Decimal {
    let owned = view.generator_owned(id);
    if owned == 0 {
        return Decimal::ZERO;
    }
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
        .mul(Decimal::from_u64(u64::from(owned)))
        .mul(tier)
        .mul(all)
        .mul(view.global_multiplier())
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

/// A short, derived, non-blocking tutorial line; `None` once all steps are done.
pub fn tutorial_hint(view: StateView<'_>) -> Option<String> {
    if view.pulses_manual() == 0 {
        return Some("SPACE: pulse the capacitor to wake the hydrophones.".to_owned());
    }
    if view.generator_owned(GeneratorId::Hydrophone) == 0 {
        return Some("h: buy a Hydrophone with Echo.".to_owned());
    }
    if view.upgrade_count() == 0 {
        return Some("u: open the Signal Chain and buy an upgrade.".to_owned());
    }
    if view.bands_matched() == 0 {
        return Some("f: tune to the carrier band for ×1.5 output.".to_owned());
    }
    if !view.can_descend() {
        return Some(format!(
            "Reach {} peak Echo to descend.",
            to_human_floor(view.next_bucket_threshold())
        ));
    }
    None
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
    fn tutorial_steps_advance_and_disappear() {
        let hint = |engine: &hadal_core::Engine| tutorial_hint(engine.view());

        assert!(
            hint(&hadal_core::Engine::new())
                .expect("pulse hint")
                .contains("pulse")
        );

        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
        });
        assert!(
            hint(&engine)
                .expect("hydrophone hint")
                .contains("Hydrophone")
        );

        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
        });
        assert!(
            hint(&engine)
                .expect("upgrade hint")
                .contains("Signal Chain")
        );

        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!(["impedance_match"]);
        });
        assert!(hint(&engine).expect("tune hint").contains("tune"));

        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["meta"]["stats"]["bands_matched"] = serde_json::json!(1);
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!(["impedance_match"]);
            value["run"]["peak_echo"] = serde_json::json!("0");
        });
        assert!(hint(&engine).expect("threshold hint").contains("Reach"));

        let engine = engine_with(|value| {
            value["meta"]["stats"]["pulses_manual"] = serde_json::json!(1);
            value["meta"]["stats"]["bands_matched"] = serde_json::json!(1);
            value["run"]["generators"] = serde_json::json!([1, 0, 0, 0]);
            value["run"]["upgrades"] = serde_json::json!(["impedance_match"]);
            value["run"]["peak_echo"] = serde_json::json!("1000000");
        });
        assert!(hint(&engine).is_none(), "hint disappears at the threshold");
    }
}
