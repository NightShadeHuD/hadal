#![forbid(unsafe_code)]
//! # HADAL Core
//!
//! Headless simulation core for the HADAL incremental game.
//!
//! ## Boundary law
//!
//! This crate contains **no** terminal UI, input, or system-time dependencies.
//! Specifically, it depends on none of:
//! `ratatui`, `crossterm`, `clap`, `rand`, or any wall-clock source.
//! The only public numeric type is [`Decimal`], which is the sole currency
//! representation throughout the game.
//!
//! ## Decimal
//!
//! [`Decimal`] stores values as a normalized `(mantissa, exponent10)` pair:
//! - Mantissa ∈ `[1, 10)` for nonzero values; canonical zero is `(0.0, 0)`.
//! - All arithmetic is deterministic: only IEEE-754 `+ - * / sqrt` are used.
//! - No `NaN` or `Infinity` can enter state.
//! - Precision: ~15–17 significant digits; adding a value >17 decades smaller
//!   than the accumulator is a no-op by definition.
//!
//! See the [`Decimal`] documentation for the full API.

pub mod content;
pub mod decimal;
pub mod engine;
pub mod offline;
pub mod save;
pub mod state;
pub mod view;

/// Simulation tick rate (fixed 10 Hz).
pub const TICKS_PER_SECOND: u64 = 10;

pub use decimal::Decimal;
pub use engine::{Action, ActionError, BuyQuantity, Engine, Event};
pub use offline::{OfflineReport, simulate_offline};
pub use save::{
    Config, ConfigWarning, Envelope, LoadWarning, LoadedSave, SaveError, SaveInfo, SaveLock,
    SaveStore, hex_digest, verify_save_bytes,
};
pub use state::GameState;
pub use view::StateView;
