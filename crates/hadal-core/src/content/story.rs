//! Narrative text for Archive logs and boot sequence.
//!
//! Restrained, procedural, eerie. No mascots, no jokes. Logs never gate
//! mechanics; they are prose only.

use super::ids::LogId;

/// Boot lines shown before the console settles.
pub const BOOT_LINES: &[&str] = &[
    "RELAY K-9 // DEPTH 7,912 M",
    "HYDROPHONE ARRAY: ONLINE",
    "CARRIER DETECTED. IT IS REPEATING.",
    "OPERATOR 7, THE STATION IS LISTENING.",
];

/// The text for a log entry.
pub fn log_text(id: LogId) -> &'static str {
    match id {
        LogId::Boot => "You wake the console. The trench is already speaking.",
        LogId::FirstPulse => "A pulse leaves the hull. Something answers it with the same shape.",
        LogId::FirstHydro => {
            "The first hydrophone lowers into the dark and begins to drink the carrier."
        }
        LogId::CarrierLock => {
            "The carrier holds a band long enough to be a word. You write it down."
        }
        LogId::FirstDescent => "BALLAST RELEASED. K-9 SINKS THROUGH ITS OWN SHADOW.",
        LogId::Depth9000 => {
            "NINE THOUSAND METRES. The pressure writes on the hull in a language you almost read."
        }
        LogId::Peak1e9 => "THE SIGNAL IS NO LONGER A SINGLE VOICE.",
        LogId::Fathoms100 => "A HUNDRED FATHOMS. Voss would have stopped here. You do not.",
        LogId::AllBands => {
            "Every band has been tuned. The carrier is not a distress call. It is a rehearsal."
        }
        LogId::AutoPulse => "The station pulses without you now. It has learned the rhythm.",
    }
}
