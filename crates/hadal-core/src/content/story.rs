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
/// Mission briefing paragraphs (each line <= 72 chars), shown in HELP.
pub const MISSION: &[&str] = &[
    "RELAY K-9 sits on the trench wall at 7,912 metres. Its hydrophones",
    "listen for the Carrier: a six-band signal that has repeated for",
    "more than thirty years. The station went quiet, but the signal",
    "never stopped. You are Operator 7, sent to wake K-9 and answer it.",
    "Echo is your currency. Pulse the capacitor, buy generators, and",
    "tune the bands. Matching the Carrier multiplies all output by 1.5.",
    "A Descent clears Echo, generators, upgrades, and tuning. Your",
    "Fathoms, hulls, depth, and logs remain. The Carrier counts. Descend.",
];

/// Three story beats revealed by the graphical boot intro (ASCII-safe, <= 40 chars each).
pub const BOOT_BEATS: [&str; 3] = [
    "RELAY K-9 // 7,912 M",
    "CARRIER DETECTED // SOURCE BELOW",
    "OPERATOR 7 // LISTEN. DESCEND.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_lines_fit_manual_width() {
        for (i, line) in MISSION.iter().enumerate() {
            assert!(
                line.len() <= 72,
                "MISSION line {i} is {} chars (max 72)",
                line.len()
            );
            assert!(line.is_ascii(), "MISSION line {i} is not ASCII");
        }
    }

    #[test]
    fn boot_beats_are_short_and_ascii() {
        for (i, beat) in BOOT_BEATS.iter().enumerate() {
            assert!(
                beat.len() <= 40,
                "BOOT_BEATS[{i}] is {} chars (max 40)",
                beat.len()
            );
            assert!(beat.is_ascii(), "BOOT_BEATS[{i}] is not ASCII");
        }
    }
}
