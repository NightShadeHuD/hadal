//! Pure key mapping and a bounded command queue.
//!
//! [`map_key`] is a pure function from a crossterm key event and an input mode
//! to an optional [`Command`]. It never touches game state, so every binding
//! and filter is unit-testable without a terminal.
//!
//! [`InputQueue`] enforces the frame cap (20 commands) and buy key-repeat
//! coalescing (≤10 buy actions per 1 s window) using a logical millisecond
//! clock supplied by the caller, keeping tests deterministic.

use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use hadal_core::BuyQuantity;
use hadal_core::content::ids::GeneratorId;

/// Maximum commands dequeued per frame.
pub const MAX_COMMANDS_PER_FRAME: usize = 20;
/// Maximum buy commands accepted per 1 s window (key-repeat coalescing).
pub const MAX_BUYS_PER_SECOND: usize = 10;
/// The buy-rate window length in milliseconds.
const BUY_WINDOW_MS: u64 = 1000;

/// A game command produced by the keymap (or by a mouse hit-test).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Fire a manual pulse (requires a full capacitor).
    Pulse,
    /// Advance the tuned band to the next carrier band.
    TuneNext,
    /// Buy `quantity` of a generator.
    Buy(GeneratorId, BuyQuantity),
    /// Persist the current state immediately.
    Save,
    /// Save and exit.
    Quit,
    /// Switch to the SONAR screen.
    OpenSonar,
    /// Switch to the DESCENT screen.
    OpenDescent,
    /// Switch to the ARCHIVE screen.
    OpenArchive,
    /// Switch to the SYSTEM screen.
    OpenSystem,
    /// On DESCENT: open the confirmation; on SONAR: open DESCENT.
    DescentConfirm,
    /// Confirm the pending Descent.
    ConfirmDescent,
    /// Close a confirmation/help/edit, leave focus, or back out a screen.
    Cancel,
    /// Enter Signal Chain focus on SONAR.
    FocusUpgrades,
    /// Move focus up / previous / scroll up.
    FocusPrev,
    /// Move focus down / next / scroll down.
    FocusNext,
    /// Activate the focused row (buy / toggle / cycle).
    FocusActivate,
    /// Adjust the Ballast Governor target by a signed amount.
    GovernorAdjust(i64),
    /// Open the help overlay.
    Help,
    /// Scroll a page up.
    PageUp,
    /// Scroll a page down.
    PageDown,
    /// Cycle to the next tab.
    NextTab,
    /// Cycle to the previous tab.
    PrevTab,
    /// Open the export text-entry overlay.
    StartExport,
    /// Open the import text-entry overlay.
    StartImport,
    /// Open the reset (DISCARD) overlay.
    StartReset,
    /// Open the tutorial skip confirmation.
    TutorialSkip,
    /// Toggle the focused toggle row.
    ToggleFocused,
    /// Mouse: focus and buy an upgrade row by index.
    FocusUpgradeAt(usize),
    /// Mouse: focus a hull row by index (a double-click activates it).
    FocusHullAt(usize),
    /// Mouse: focus and activate a SYSTEM row by index.
    SystemFocusAt(usize),
}

/// The active input mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Gameplay bindings are active.
    Normal,
    /// A text-entry overlay owns the keyboard.
    TextEntry,
}

/// Map a key event to a command in the given mode.
///
/// Rules:
/// - `KeyEventKind::Release` is always ignored.
/// - In [`Mode::TextEntry`] every key is consumed and no game command is
///   emitted; the overlay's editor handles characters, `Esc`, and `Enter`.
pub fn map_key(key: KeyEvent, mode: Mode) -> Option<Command> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match mode {
        Mode::TextEntry => None,
        Mode::Normal => map_normal(key),
    }
}

fn map_normal(key: KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => Some(Command::Save),
            KeyCode::Char('c') | KeyCode::Char('C') => Some(Command::Quit),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char(' ') => Some(Command::Pulse),
        KeyCode::Char('f') => Some(Command::TuneNext),
        KeyCode::Char('h') => Some(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One)),
        KeyCode::Char('H') => Some(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::Max)),
        KeyCode::Char('d') => Some(Command::Buy(GeneratorId::Drifter, BuyQuantity::One)),
        KeyCode::Char('D') => Some(Command::Buy(GeneratorId::Drifter, BuyQuantity::Max)),
        KeyCode::Char('m') => Some(Command::Buy(GeneratorId::SonarMast, BuyQuantity::One)),
        KeyCode::Char('M') => Some(Command::Buy(GeneratorId::SonarMast, BuyQuantity::Max)),
        KeyCode::Char('c') => Some(Command::Buy(GeneratorId::TidalCore, BuyQuantity::One)),
        KeyCode::Char('C') => Some(Command::Buy(GeneratorId::TidalCore, BuyQuantity::Max)),
        KeyCode::Char('q') => Some(Command::Quit),
        KeyCode::Char('1') => Some(Command::OpenSonar),
        KeyCode::Char('2') => Some(Command::OpenDescent),
        KeyCode::Char('3') => Some(Command::OpenArchive),
        KeyCode::Char('4') => Some(Command::OpenSystem),
        KeyCode::Char('p') => Some(Command::DescentConfirm),
        KeyCode::Char('y') => Some(Command::ConfirmDescent),
        KeyCode::Char('u') => Some(Command::FocusUpgrades),
        KeyCode::Char('s') => Some(Command::Save),
        KeyCode::Char('S') => Some(Command::TutorialSkip),
        KeyCode::Char('t') => Some(Command::ToggleFocused),
        KeyCode::Char('e') => Some(Command::StartExport),
        KeyCode::Char('i') => Some(Command::StartImport),
        KeyCode::Char('R') => Some(Command::StartReset),
        KeyCode::Char('?') => Some(Command::Help),
        KeyCode::Esc => Some(Command::Cancel),
        KeyCode::Up => Some(Command::FocusPrev),
        KeyCode::Down => Some(Command::FocusNext),
        KeyCode::Enter => Some(Command::FocusActivate),
        KeyCode::Tab => Some(Command::NextTab),
        KeyCode::BackTab => Some(Command::PrevTab),
        KeyCode::PageUp => Some(Command::PageUp),
        KeyCode::PageDown => Some(Command::PageDown),
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                10
            } else {
                1
            };
            Some(Command::GovernorAdjust(step))
        }
        KeyCode::Char('-') | KeyCode::Char('_') => {
            let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                10
            } else {
                1
            };
            Some(Command::GovernorAdjust(-step))
        }
        _ => None,
    }
}

/// Bounded command queue with buy-rate coalescing.
#[derive(Debug, Default)]
pub struct InputQueue {
    pending: VecDeque<Command>,
    buy_times_ms: VecDeque<u64>,
}

impl InputQueue {
    /// Create an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer a command at logical time `now_ms`.
    ///
    /// Returns `true` if it was queued. Buy commands are dropped once ten have
    /// been accepted within the trailing one-second window; all commands are
    /// dropped once twenty are pending.
    pub fn push(&mut self, command: Command, now_ms: u64) -> bool {
        if self.pending.len() >= MAX_COMMANDS_PER_FRAME {
            return false;
        }
        if matches!(command, Command::Buy(..)) && !self.accept_buy(now_ms) {
            return false;
        }
        self.pending.push_back(command);
        true
    }

    fn accept_buy(&mut self, now_ms: u64) -> bool {
        while let Some(&front) = self.buy_times_ms.front() {
            if now_ms.saturating_sub(front) >= BUY_WINDOW_MS {
                self.buy_times_ms.pop_front();
            } else {
                break;
            }
        }
        if self.buy_times_ms.len() >= MAX_BUYS_PER_SECOND {
            return false;
        }
        self.buy_times_ms.push_back(now_ms);
        true
    }

    /// Drain up to [`MAX_COMMANDS_PER_FRAME`] commands in order.
    pub fn drain_frame(&mut self) -> Vec<Command> {
        let take = self.pending.len().min(MAX_COMMANDS_PER_FRAME);
        self.pending.drain(..take).collect()
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn maps_every_phase_one_binding() {
        let cases = [
            (KeyCode::Char(' '), Command::Pulse),
            (KeyCode::Char('f'), Command::TuneNext),
            (
                KeyCode::Char('h'),
                Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One),
            ),
            (
                KeyCode::Char('H'),
                Command::Buy(GeneratorId::Hydrophone, BuyQuantity::Max),
            ),
            (
                KeyCode::Char('d'),
                Command::Buy(GeneratorId::Drifter, BuyQuantity::One),
            ),
            (
                KeyCode::Char('D'),
                Command::Buy(GeneratorId::Drifter, BuyQuantity::Max),
            ),
            (
                KeyCode::Char('m'),
                Command::Buy(GeneratorId::SonarMast, BuyQuantity::One),
            ),
            (
                KeyCode::Char('M'),
                Command::Buy(GeneratorId::SonarMast, BuyQuantity::Max),
            ),
            (
                KeyCode::Char('c'),
                Command::Buy(GeneratorId::TidalCore, BuyQuantity::One),
            ),
            (
                KeyCode::Char('C'),
                Command::Buy(GeneratorId::TidalCore, BuyQuantity::Max),
            ),
            (KeyCode::Char('q'), Command::Quit),
        ];
        for (code, expected) in cases {
            assert_eq!(
                map_key(press(code), Mode::Normal),
                Some(expected),
                "code {code:?}"
            );
        }
    }

    #[test]
    fn maps_screens_tabs_and_focus() {
        let cases = [
            (KeyCode::Char('1'), Command::OpenSonar),
            (KeyCode::Char('2'), Command::OpenDescent),
            (KeyCode::Char('3'), Command::OpenArchive),
            (KeyCode::Char('4'), Command::OpenSystem),
            (KeyCode::Char('p'), Command::DescentConfirm),
            (KeyCode::Char('y'), Command::ConfirmDescent),
            (KeyCode::Char('u'), Command::FocusUpgrades),
            (KeyCode::Esc, Command::Cancel),
            (KeyCode::Up, Command::FocusPrev),
            (KeyCode::Down, Command::FocusNext),
            (KeyCode::Enter, Command::FocusActivate),
            (KeyCode::Tab, Command::NextTab),
            (KeyCode::BackTab, Command::PrevTab),
            (KeyCode::PageUp, Command::PageUp),
            (KeyCode::PageDown, Command::PageDown),
            (KeyCode::Char('?'), Command::Help),
        ];
        for (code, expected) in cases {
            assert_eq!(
                map_key(press(code), Mode::Normal),
                Some(expected),
                "code {code:?}"
            );
        }
    }

    #[test]
    fn maps_system_and_text_entry_keys() {
        let cases = [
            (KeyCode::Char('s'), Command::Save),
            (KeyCode::Char('S'), Command::TutorialSkip),
            (KeyCode::Char('t'), Command::ToggleFocused),
            (KeyCode::Char('e'), Command::StartExport),
            (KeyCode::Char('i'), Command::StartImport),
            (KeyCode::Char('R'), Command::StartReset),
        ];
        for (code, expected) in cases {
            assert_eq!(
                map_key(press(code), Mode::Normal),
                Some(expected),
                "code {code:?}"
            );
        }
        // Lowercase r is not reset.
        assert_eq!(map_key(press(KeyCode::Char('r')), Mode::Normal), None);
    }

    #[test]
    fn maps_governor_adjust_and_shift() {
        assert_eq!(
            map_key(press(KeyCode::Char('+')), Mode::Normal),
            Some(Command::GovernorAdjust(1))
        );
        assert_eq!(
            map_key(
                press_mod(KeyCode::Char('+'), KeyModifiers::SHIFT),
                Mode::Normal
            ),
            Some(Command::GovernorAdjust(10))
        );
        assert_eq!(
            map_key(press(KeyCode::Char('=')), Mode::Normal),
            Some(Command::GovernorAdjust(1))
        );
        assert_eq!(
            map_key(press(KeyCode::Char('-')), Mode::Normal),
            Some(Command::GovernorAdjust(-1))
        );
        assert_eq!(
            map_key(press(KeyCode::Char('_')), Mode::Normal),
            Some(Command::GovernorAdjust(-1))
        );
        assert_eq!(
            map_key(
                press_mod(KeyCode::Char('-'), KeyModifiers::SHIFT),
                Mode::Normal
            ),
            Some(Command::GovernorAdjust(-10))
        );
    }

    #[test]
    fn maps_ctrl_save_and_ctrl_quit() {
        assert_eq!(
            map_key(
                press_mod(KeyCode::Char('s'), KeyModifiers::CONTROL),
                Mode::Normal
            ),
            Some(Command::Save)
        );
        assert_eq!(
            map_key(
                press_mod(KeyCode::Char('c'), KeyModifiers::CONTROL),
                Mode::Normal
            ),
            Some(Command::Quit)
        );
    }

    #[test]
    fn release_events_ignored() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(map_key(release, Mode::Normal), None);
        let release_enter =
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(map_key(release_enter, Mode::Normal), None);
    }

    #[test]
    fn repeat_events_still_map() {
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Char('h'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(
            map_key(repeat, Mode::Normal),
            Some(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One))
        );
    }

    #[test]
    fn unmapped_keys_return_none() {
        for code in [
            KeyCode::Char('z'),
            KeyCode::Char('x'),
            KeyCode::Char('k'),
            KeyCode::Char('5'),
            KeyCode::F(1),
            KeyCode::Insert,
        ] {
            assert_eq!(map_key(press(code), Mode::Normal), None, "code {code:?}");
        }
    }

    #[test]
    fn text_entry_consumes_all_game_keys() {
        for code in [
            KeyCode::Char(' '),
            KeyCode::Char('h'),
            KeyCode::Char('q'),
            KeyCode::Char('f'),
            KeyCode::Char('2'),
            KeyCode::Char('u'),
            KeyCode::Char('e'),
            KeyCode::Char('R'),
        ] {
            assert_eq!(
                map_key(press(code), Mode::TextEntry),
                None,
                "text entry must not emit a game command for {code:?}"
            );
        }
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Tab,
            KeyCode::Backspace,
        ] {
            assert_eq!(
                map_key(press(code), Mode::TextEntry),
                None,
                "text entry must consume {code:?}"
            );
        }
        assert_eq!(
            map_key(
                press_mod(KeyCode::Char('s'), KeyModifiers::CONTROL),
                Mode::TextEntry
            ),
            None
        );
    }

    #[test]
    fn queue_caps_at_twenty_per_frame() {
        let mut q = InputQueue::new();
        let mut accepted = 0;
        for _ in 0..25 {
            if q.push(Command::Pulse, 0) {
                accepted += 1;
            }
        }
        assert_eq!(accepted, MAX_COMMANDS_PER_FRAME);
        assert_eq!(q.drain_frame().len(), MAX_COMMANDS_PER_FRAME);
        assert_eq!(q.drain_frame().len(), 0);
    }

    #[test]
    fn queue_coalesces_buys_to_ten_per_second() {
        let mut q = InputQueue::new();
        let mut accepted = 0;
        for _ in 0..15 {
            if q.push(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One), 0) {
                accepted += 1;
            }
        }
        assert_eq!(accepted, MAX_BUYS_PER_SECOND);
        let _ = q.drain_frame();

        assert!(q.push(
            Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One),
            BUY_WINDOW_MS
        ));
    }

    #[test]
    fn non_buy_commands_not_rate_limited() {
        let mut q = InputQueue::new();
        let mut accepted = 0;
        for i in 0..10 {
            if q.push(Command::Save, i) {
                accepted += 1;
            }
        }
        assert_eq!(accepted, 10);
    }

    #[test]
    fn buy_rate_window_slides() {
        let mut q = InputQueue::new();
        for i in 0..10u64 {
            assert!(q.push(Command::Buy(GeneratorId::Drifter, BuyQuantity::Max), i * 90));
        }
        let _ = q.drain_frame();
        assert!(q.push(Command::Buy(GeneratorId::Drifter, BuyQuantity::Max), 1000));
        assert!(!q.push(Command::Buy(GeneratorId::Drifter, BuyQuantity::Max), 1000));
    }

    #[test]
    fn drain_respects_order() {
        let mut q = InputQueue::new();
        assert!(q.push(Command::Pulse, 0));
        assert!(q.push(Command::OpenArchive, 0));
        assert!(q.push(Command::FocusNext, 0));
        assert_eq!(
            q.drain_frame(),
            vec![Command::Pulse, Command::OpenArchive, Command::FocusNext]
        );
    }

    #[test]
    fn paste_event_not_mapped() {
        let key = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('\u{1b}'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyEventState::empty(),
        );
        assert_eq!(map_key(key, Mode::Normal), None);
    }
}
