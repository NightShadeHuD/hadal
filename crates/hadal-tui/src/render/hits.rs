//! Screen hit-testing for mouse input.
//!
//! Renderers fill a [`HitMap`] with the exact rectangles they drew; mouse
//! events are translated into the same [`Command`]s the keymap produces, so
//! there is a single dispatch path.

use crossterm::event::MouseButton;
use hadal_core::BuyQuantity;
use hadal_core::content::ids::GeneratorId;
use ratatui::layout::Rect;

use crate::input::Command;

/// A top-level screen tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// SONAR.
    Sonar,
    /// DESCENT.
    Descent,
    /// ARCHIVE.
    Archive,
    /// SYSTEM.
    System,
}

/// Clickable regions produced by the last render.
#[derive(Clone, Debug, Default)]
pub struct HitMap {
    pub tabs: Vec<(Rect, Tab)>,
    pub generators: Vec<(Rect, GeneratorId)>,
    pub upgrades: Vec<(Rect, usize)>,
    pub hulls: Vec<(Rect, usize)>,
    pub system: Vec<(Rect, usize)>,
    /// Total ARCHIVE content rows (for scroll clamping).
    pub archive_total: usize,
    /// Visible ARCHIVE content rows (for scroll clamping).
    pub archive_visible: usize,
    /// Total HELP document rows (for scroll clamping).
    pub help_total: usize,
    /// Visible HELP document rows (for scroll clamping).
    pub help_visible: usize,
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

impl HitMap {
    /// Tab under a cell, if any.
    pub fn tab_at(&self, x: u16, y: u16) -> Option<Tab> {
        self.tabs
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, tab)| *tab)
    }

    /// Generator row under a cell.
    pub fn generator_at(&self, x: u16, y: u16) -> Option<GeneratorId> {
        self.generators
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, id)| *id)
    }

    /// Signal Chain row under a cell.
    pub fn upgrade_at(&self, x: u16, y: u16) -> Option<usize> {
        self.upgrades
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, index)| *index)
    }

    /// Hull row under a cell.
    pub fn hull_at(&self, x: u16, y: u16) -> Option<usize> {
        self.hulls
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, index)| *index)
    }

    /// SYSTEM setting/automation row under a cell.
    pub fn system_at(&self, x: u16, y: u16) -> Option<usize> {
        self.system
            .iter()
            .find(|(rect, _)| contains(*rect, x, y))
            .map(|(_, index)| *index)
    }
}

/// Translate a click into a command.
///
/// Left-click buys one generator; right-click or Shift-click buys max. Other
/// rows focus/activate through the same command path as the keyboard.
pub fn command_for_click(
    hit: &HitMap,
    column: u16,
    row: u16,
    button: MouseButton,
    shift: bool,
) -> Option<Command> {
    if let Some(tab) = hit.tab_at(column, row) {
        return Some(match tab {
            Tab::Sonar => Command::OpenSonar,
            Tab::Descent => Command::OpenDescent,
            Tab::Archive => Command::OpenArchive,
            Tab::System => Command::OpenSystem,
        });
    }
    if let Some(id) = hit.generator_at(column, row) {
        let quantity = if shift || button == MouseButton::Right {
            BuyQuantity::Max
        } else {
            BuyQuantity::One
        };
        return Some(Command::Buy(id, quantity));
    }
    if let Some(index) = hit.upgrade_at(column, row) {
        return Some(Command::FocusUpgradeAt(index));
    }
    if let Some(index) = hit.hull_at(column, row) {
        return Some(Command::FocusHullAt(index));
    }
    if let Some(index) = hit.system_at(column, row) {
        return Some(Command::SystemFocusAt(index));
    }
    None
}

/// Centre cell of a rectangle (used by mouse tests).
#[cfg(test)]
pub fn center(rect: Rect) -> (u16, u16) {
    (
        rect.x.saturating_add(rect.width / 2),
        rect.y.saturating_add(rect.height / 2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect::new(x, y, width, height)
    }

    #[test]
    fn maps_tabs_to_open_commands() {
        let hit = HitMap {
            tabs: vec![
                (rect(0, 0, 5, 1), Tab::Sonar),
                (rect(6, 0, 7, 1), Tab::Descent),
                (rect(14, 0, 7, 1), Tab::Archive),
                (rect(22, 0, 6, 1), Tab::System),
            ],
            ..HitMap::default()
        };
        assert_eq!(
            command_for_click(&hit, 15, 0, MouseButton::Left, false),
            Some(Command::OpenArchive)
        );
        assert_eq!(
            command_for_click(&hit, 23, 0, MouseButton::Left, false),
            Some(Command::OpenSystem)
        );
    }

    #[test]
    fn generator_click_buys_one_or_max() {
        let hit = HitMap {
            generators: vec![
                (rect(1, 10, 40, 1), GeneratorId::Hydrophone),
                (rect(1, 11, 40, 1), GeneratorId::Drifter),
            ],
            ..HitMap::default()
        };
        assert_eq!(
            command_for_click(&hit, 5, 10, MouseButton::Left, false),
            Some(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::One))
        );
        assert_eq!(
            command_for_click(&hit, 5, 10, MouseButton::Right, false),
            Some(Command::Buy(GeneratorId::Hydrophone, BuyQuantity::Max))
        );
        assert_eq!(
            command_for_click(&hit, 5, 11, MouseButton::Left, true),
            Some(Command::Buy(GeneratorId::Drifter, BuyQuantity::Max))
        );
    }

    #[test]
    fn list_clicks_focus_rows() {
        let hit = HitMap {
            upgrades: vec![(rect(1, 2, 30, 1), 3)],
            hulls: vec![(rect(1, 4, 30, 1), 7)],
            system: vec![(rect(1, 6, 30, 1), 2)],
            ..HitMap::default()
        };
        assert_eq!(
            command_for_click(&hit, 4, 2, MouseButton::Left, false),
            Some(Command::FocusUpgradeAt(3))
        );
        assert_eq!(
            command_for_click(&hit, 4, 4, MouseButton::Left, false),
            Some(Command::FocusHullAt(7))
        );
        assert_eq!(
            command_for_click(&hit, 4, 6, MouseButton::Left, false),
            Some(Command::SystemFocusAt(2))
        );
    }

    #[test]
    fn clicks_outside_regions_do_nothing() {
        let hit = HitMap::default();
        assert_eq!(
            command_for_click(&hit, 100, 100, MouseButton::Left, false),
            None
        );
    }
}
