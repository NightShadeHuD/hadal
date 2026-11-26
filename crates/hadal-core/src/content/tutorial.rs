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
    /// UI target for a coach mark.
    pub target: TutorialTarget,
    /// Completion kind.
    pub kind: TutorialKind,
}

/// `step == COMPLETED_STEP` means the tutorial is finished.
pub const COMPLETED_STEP: u8 = 13;

/// All thirteen steps, in order.
pub const STEPS: [TutorialStep; 13] = [
    TutorialStep {
        id: TutorialStepId::Pulse,
        chapter: 1,
        title: "PULSE THE ARRAY",
        instruction: "Press SPACE to pulse the capacitor.",
        why: "A pulse turns 10 seconds of production into Echo, the currency you spend this run.",
        target: TutorialTarget::PulseBar,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::FirstGenerator,
        chapter: 1,
        title: "THE FIRST HYDROPHONE",
        instruction: "Press h to buy a Hydrophone for 10 Echo.",
        why: "It produces Echo every second, even while you do nothing.",
        target: TutorialTarget::Generator(GeneratorId::Hydrophone),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::SecondGenerator,
        chapter: 1,
        title: "BUILD THE ARRAY",
        instruction: "Wait for the next price, then press h again.",
        why: "Production is passive, but each unit costs 15% more than the last.",
        target: TutorialTarget::Generator(GeneratorId::Hydrophone),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Drifter,
        chapter: 1,
        title: "CALL THE DRIFTER",
        instruction: "Reach 10 Hydrophones, then press d for a Drifter.",
        why: "Each tier produces far more and unlocks at a count of the tier below.",
        target: TutorialTarget::Generator(GeneratorId::Drifter),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Upgrade,
        chapter: 1,
        title: "SIGNAL CHAIN",
        instruction: "Press u, then Enter, to buy Impedance Match.",
        why: "100 Echo doubles every generator for this run. Upgrades reset when you descend.",
        target: TutorialTarget::ChainPanel,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Tune,
        chapter: 1,
        title: "TUNE THE CARRIER",
        instruction: "When the carrier drifts, press f until it reads TUNED.",
        why: "The carrier changes every 90 seconds. Matching it multiplies production by 1.5 until it drifts again.",
        target: TutorialTarget::TuneStrip,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Peak,
        chapter: 1,
        title: "PEAK ECHO",
        instruction: "Reach 1.00M Peak Echo.",
        why: "Peak is the highest Echo you have held this run. Descent gain is set by it, and the bar tracks your progress.",
        target: TutorialTarget::PeakReadout,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Descend,
        chapter: 1,
        title: "DESCEND",
        instruction: "Open DESCENT with 2, press p, then y to confirm.",
        why: "You keep Fathoms and sink 500 m. Echo, generators, upgrades and tuning reset.",
        target: TutorialTarget::DescentTab,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::Hull,
        chapter: 1,
        title: "PERMANENT HULLS",
        instruction: "Press 2, select Auto-Pulse and press Enter.",
        why: "Hulls are permanent. Auto-Pulse fires the capacitor for you from now on.",
        target: TutorialTarget::Hull(HullId::AutoPulse),
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::SecondDescent,
        chapter: 2,
        title: "THE SECOND DESCENT",
        instruction: "Reach 1.00M Peak again and descend a second time.",
        why: "Fathoms and hulls persist. Each run starts stronger and earns more Fathoms.",
        target: TutorialTarget::DescentTab,
        kind: TutorialKind::State,
    },
    TutorialStep {
        id: TutorialStepId::System,
        chapter: 2,
        title: "THE SYSTEM CONSOLE",
        instruction: "Press 4 to open SYSTEM.",
        why: "Automation toggles, the save path, export/import and settings live here. The game saves itself.",
        target: TutorialTarget::SystemTab,
        kind: TutorialKind::Ui,
    },
    TutorialStep {
        id: TutorialStepId::Manual,
        chapter: 2,
        title: "FIELD MANUAL",
        instruction: "Press ? to open the field manual, then Esc to close it.",
        why: "The manual explains every screen and defines every term in the Glossary.",
        target: TutorialTarget::HelpKey,
        kind: TutorialKind::Ui,
    },
    TutorialStep {
        id: TutorialStepId::Offline,
        chapter: 2,
        title: "LEAVE IT LISTENING",
        instruction: "Close the game whenever you like.",
        why: "Offline progress runs the same simulation at full rate, capped at 8 hours until hulls extend it. Press Enter to finish the tutorial.",
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
            assert!(
                step.instruction.chars().count() <= 70,
                "step {index} instruction too long"
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
    }
}
