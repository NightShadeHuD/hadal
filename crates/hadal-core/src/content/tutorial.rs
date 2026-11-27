//! Tutorial copy, ordering, and UI targets.
//!
//! This module is the single source of truth for tutorial copy. The engine
//! advances steps; the TUI renders [`TutorialStep`] entries and uses
//! [`TutorialTarget`] to place coach marks.

use super::ids::{GeneratorId, HullId};

/// Tutorial step identity (index order matches [`STEPS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialStepId {
    Pulse,
    FirstGenerator,
    SecondGenerator,
    Drifter,
    Upgrade,
    Tune,
    Peak,
    Descend,
    Hull,
    SecondDescent,
    System,
    Manual,
    Offline,
}

impl TutorialStepId {
    /// Dense step index (`0..=12`).
    pub const fn index(self) -> u8 {
        self as u8
    }
}

/// What kind of interaction completes a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialKind {
    /// Completed by game state (counts, ownership, peak, descents).
    State,
    /// Completed by opening a UI surface.
    Ui,
    /// Completed by an explicit acknowledgement.
    Acknowledge,
}

/// Where the coach mark should point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialTarget {
    PulseBar,
    Generator(GeneratorId),
    ChainPanel,
    TuneStrip,
    DescentTab,
    Hull(HullId),
    SystemTab,
    HelpKey,
    PeakReadout,
    None,
}

/// One tutorial step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TutorialStep {
    /// Stable identity.
    pub id: TutorialStepId,
    /// Chapter number (1 or 2).
    pub chapter: u8,
    /// Short all-caps heading.
    pub title: &'static str,
    /// One-line instruction (kept short; the TUI wraps).
    pub instruction: &'static str,
    /// Why this matters.
    pub why: &'static str,
    /// Full explanation (2-4 sentences; <= 300 chars, ASCII-only).
    pub detail: &'static str,
    /// UI target for a coach mark.
    pub target: TutorialTarget,
    /// Completion kind.
    pub kind: TutorialKind,
}

/// `step == COMPLETED_STEP` means the tutorial is finished.
pub const COMPLETED_STEP: u8 = 13;

/// A tutorial chapter with its title and story-framed intro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chapter {
    /// Chapter number (1 or 2).
    pub number: u8,
    /// Display title.
    pub title: &'static str,
    /// One-line story-framed intro (<= 100 chars).
    pub intro: &'static str,
}

/// The two tutorial chapters.
pub const CHAPTERS: [Chapter; 2] = [
    Chapter {
        number: 1,
        title: "FIRST SIGNAL",
        intro: "K-9 hears a six-band signal. Learn to pulse, listen, and build the array.",
    },
    Chapter {
        number: 2,
        title: "SECOND SIGNAL",
        intro: "Each Descent sinks deeper. Hulls keep K-9 listening while you are gone.",
    },
];
/// All thirteen steps, in order.
pub const STEPS: [TutorialStep; 13] = [
    TutorialStep {
        id: TutorialStepId::Pulse,
        chapter: 1,
        title: "PULSE THE ARRAY",
        instruction: "Press SPACE. The capacitor is full: one pulse pays at least 10 Echo.",
        why: "Echo is the only run currency. A pulse pays 10 s of production, minimum 10; generators add Echo every second.",
        detail: "Echo is the only run currency. A full capacitor converts 10 s of current production into Echo (floor 10). Every generator adds Echo per second. The trench answers every pulse with its own echo - pulsing is always the right action.",
        target: TutorialTarget::PulseBar,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::FirstGenerator,
        chapter: 1,
        title: "THE FIRST HYDROPHONE",
        instruction: "Press h. A Hydrophone costs 10 Echo and adds 0.50/s base.",
        why: "It listens while you do nothing: 0.50/s every second, more when tuned. The capacitor refills for the next pulse.",
        detail: "Hydrophone costs 10 Echo, adds 0.50/s base. Production accrues while you do nothing - each second between pulses generates more Echo. K-9 lowers its hydrophone into the trench and begins drinking the carrier; buying this is the first step toward automation.",
        target: TutorialTarget::Generator(GeneratorId::Hydrophone),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::SecondGenerator,
        chapter: 1,
        title: "BUILD THE ARRAY",
        instruction: "Press h at 12 Echo. Each Hydrophone costs 15% more than the last.",
        why: "Two Hydrophones add 1.00/s base. Buy early: each purchase raises the next cost by 15%.",
        detail: "Second Hydrophone costs 12 Echo (10 * 1.15). Two Hydrophones produce 1.00/s base, doubling passive income. Each purchase raises the next cost by 15%, so buying early maximises the compounding window across the full run.",
        target: TutorialTarget::Generator(GeneratorId::Hydrophone),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Drifter,
        chapter: 1,
        title: "CALL THE DRIFTER",
        instruction: "Press h until 10 Hydrophones, then d for a Drifter.",
        why: "Drifters unlock at 10 Hydrophones and add 8/s base each. One tier beats cheap spam.",
        detail: "Drifter unlocks at 10 Hydrophones, costs 250 Echo, adds 8/s base with 1.18 cost ratio per unit. Larger tiers raise output far beyond cheap Hydrophones; the strategy is tiers over cheap spam. K-9 deploys a drift array deeper into the trench.",
        target: TutorialTarget::Generator(GeneratorId::Drifter),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Upgrade,
        chapter: 1,
        title: "SIGNAL CHAIN",
        instruction: "Press u to focus SIGNAL CHAIN; Enter buys Impedance Match.",
        why: "100 Echo doubles all production this run. Upgrades reset on Descent, so buy early.",
        detail: "Signal Chain is one-time run upgrades bought with Echo. Press u to open, Enter to buy the highlighted row. Impedance Match costs 100 Echo, doubles all production for the run, resets on Descent. Affordability is shown exactly; buying early doubles the compounding window.",
        target: TutorialTarget::ChainPanel,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Tune,
        chapter: 1,
        title: "TUNE THE CARRIER",
        instruction: "The carrier drifts every 90 s: press f until the band reads TUNED.",
        why: "A match multiplies all output by 1.5. A mismatch costs nothing, so checking is free.",
        detail: "The Carrier shifts bands every 90 seconds. Press f to move your tuner; match multiplies all output by 1.5, mismatch costs nothing. Checking after each drift is free value - K-9 rehearses the signal across bands, and tuning aligns the array to catch it on the right frequency.",
        target: TutorialTarget::TuneStrip,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Peak,
        chapter: 1,
        title: "PEAK ECHO",
        instruction: "Reach 1.00M Peak Echo. Buy any rate increase you can afford.",
        why: "Peak is the highest Echo this run; Descent gain scales from it. The NEXT bar tracks it.",
        detail: "Peak is the highest Echo balance this run. Descent requires 1.00M Peak; the NEXT bar shows how close you are. Buying rate increases raises your Echo/s, which is how you reach Peak faster. The Carrier counts in Echo - peak is the measure of how loud you have been.",
        target: TutorialTarget::PeakReadout,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Descend,
        chapter: 1,
        title: "DESCEND",
        instruction: "Press 2, then p, then y. Fathoms stay; the run resets.",
        why: "Echo, generators, upgrades and tuning reset. Fathoms, hulls, depth and logs remain.",
        detail: "Press 2, then p, then y to confirm descent. Echo, generators, upgrades, and tuning reset. Fathoms, hulls, depth record, and logs persist. Each Descent sinks K-9 another 500 metres into the trench. The first bucket awards +3 Fathoms based on your Peak Echo.",
        target: TutorialTarget::DescentTab,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Hull,
        chapter: 1,
        title: "PERMANENT HULLS",
        instruction: "On DESCENT, select Auto-Pulse and press Enter. It costs 1 Fathom.",
        why: "Auto-Pulse fires the capacitor whenever it fills, so K-9 pulses without you. Hulls persist.",
        detail: "Fathoms buy permanent hull upgrades. Auto-Pulse costs 1 Fathom and fires the capacitor whenever it fills - pulses happen without you. Hulls persist across descents; automation is the point of Fathoms. K-9 learns to pulse on its own.",
        target: TutorialTarget::Hull(HullId::AutoPulse),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::SecondDescent,
        chapter: 2,
        title: "THE SECOND DESCENT",
        instruction: "Reach 1.00M Peak and descend again. Hulls make this run faster.",
        why: "Fathoms and hulls persist, so each run starts stronger and reaches a higher Peak.",
        detail: "Second Descent reaches 1.00M faster because hulls and Fathoms from the first run carry over. Higher Peak means more Fathoms earned per Descent, which buys more hulls, which automate more actions. The trench gets deeper; K-9 listens further down.",
        target: TutorialTarget::DescentTab,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::System,
        chapter: 2,
        title: "THE SYSTEM CONSOLE",
        instruction: "Press 4: SYSTEM holds automation, saves and settings.",
        why: "Auto-buy, the governor, save/export/import and reset live here. Autosave runs every 30 s.",
        detail: "Press 4 to open SYSTEM. Toggle auto-buy per generator, set the Ballast Governor target, manage saves (export/import), and reset the run. The offline cap is raised by hulls (8, 24, 72, or 168 h), not set here. Autosave runs every 30 s and on exit.",
        target: TutorialTarget::SystemTab,
        kind: TutorialKind::Ui,
    },
    TutorialStep {
        id: TutorialStepId::Manual,
        chapter: 2,
        title: "FIELD MANUAL",
        instruction: "Press ?. Arrows scroll; read the TUTORIAL REFERENCE, then Esc.",
        why: "Every step's full explanation and every term are documented; reopen it any time.",
        detail: "The field manual is complete. TUTORIAL REFERENCE lists all 13 steps with instruction, why, and full detail. The GLOSSARY defines every term: Echo, Peak, Fathom, carrier, hull, and more. Press Esc to return; reopen the manual any time to review K-9's operations.",
        target: TutorialTarget::HelpKey,
        kind: TutorialKind::Ui,
    },
    TutorialStep {
        id: TutorialStepId::Offline,
        chapter: 2,
        title: "LEAVE IT LISTENING",
        instruction: "Press Enter to finish; close the game whenever you like.",
        why: "Away time runs at full rate, automation only, capped at 8 h until hulls raise it.",
        detail: "Away time simulates at full production using automation only - no manual actions. Base cap is 8 hours; Long Storage 24 h, Extended Ballast 72 h, Abyssal Reservoir 168 h. A welcome-back summary credits the time away and shows the Echo gained. The station keeps listening while you are gone.",
        target: TutorialTarget::None,
        kind: TutorialKind::Acknowledge,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steps_table() {
        assert_eq!(STEPS.len(), 13);
        assert_eq!(COMPLETED_STEP, 13);
        let mut seen = Vec::new();
        for (index, step) in STEPS.iter().enumerate() {
            assert_eq!(step.id.index() as usize, index);
            assert!(!seen.contains(&step.id), "duplicate id {:?}", step.id);
            seen.push(step.id);
            assert!(!step.title.is_empty(), "step {index} title");
            assert!(!step.instruction.is_empty(), "step {index} instruction");
            assert!(!step.why.is_empty(), "step {index} why");
            assert!(!step.detail.is_empty(), "step {index} detail");
            assert!(
                step.instruction.chars().count() <= 72,
                "step {index} instruction too long"
            );
            assert!(step.why.chars().count() <= 120, "step {index} why too long");
            assert!(
                step.detail.chars().count() <= 300,
                "step {index} detail too long"
            );
            assert!(step.detail.is_ascii(), "step {index} detail is not ASCII");
            assert!(
                step.instruction.is_ascii(),
                "step {index} instruction is not ASCII"
            );
            assert!(step.why.is_ascii(), "step {index} why is not ASCII");
            assert!(
                !step.instruction.contains("simply"),
                "step {index} instruction uses 'simply'"
            );
            assert!(
                !step.why.contains("simply"),
                "step {index} why uses 'simply'"
            );
            assert!(
                !step.detail.contains("simply"),
                "step {index} detail uses 'simply'"
            );
            let expected_chapter = if index <= 8 { 1 } else { 2 };
            assert_eq!(step.chapter, expected_chapter, "step {index} chapter");
        }
        assert_eq!(STEPS[0].target, TutorialTarget::PulseBar);
        assert_eq!(
            STEPS[1].target,
            TutorialTarget::Generator(GeneratorId::Hydrophone)
        );
        assert_eq!(
            STEPS[3].target,
            TutorialTarget::Generator(GeneratorId::Drifter)
        );
        assert_eq!(STEPS[4].target, TutorialTarget::ChainPanel);
        assert_eq!(STEPS[6].target, TutorialTarget::PeakReadout);
        assert_eq!(STEPS[8].target, TutorialTarget::Hull(HullId::AutoPulse));
        assert_eq!(STEPS[10].kind, TutorialKind::Ui);
        assert_eq!(STEPS[11].kind, TutorialKind::Ui);
        assert_eq!(STEPS[12].kind, TutorialKind::Acknowledge);
        assert_eq!(STEPS[12].target, TutorialTarget::None);
        // CHAPTERS covers numbers 1 and 2 with non-empty intro <= 100 chars.
        let chapter_numbers: Vec<u8> = CHAPTERS.iter().map(|c| c.number).collect();
        assert!(chapter_numbers.contains(&1), "CHAPTERS missing number 1");
        assert!(chapter_numbers.contains(&2), "CHAPTERS missing number 2");
        for (i, ch) in CHAPTERS.iter().enumerate() {
            assert!(!ch.intro.is_empty(), "chapter {i} intro empty");
            assert!(
                ch.intro.chars().count() <= 100,
                "chapter {i} intro too long"
            );
        }
        // Every step's chapter exists in CHAPTERS.
        for (i, step) in STEPS.iter().enumerate() {
            assert!(
                CHAPTERS.iter().any(|c| c.number == step.chapter),
                "step {i} chapter {} not in CHAPTERS",
                step.chapter
            );
        }
    }
}
