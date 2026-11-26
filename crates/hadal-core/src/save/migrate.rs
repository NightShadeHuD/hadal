//! Schema migration machinery.
//!
//! Migrations transform the parsed envelope as a [`serde_json::Value`] before
//! final deserialization. Production ships schema v1 with an empty chain; tests
//! register a synthetic `0 -> 1` step.

use serde_json::Value;
use thiserror::Error;

/// A migration step: transforms an envelope value.
pub type MigrationStep = fn(Value) -> Result<Value, String>;

/// Error applying a migration chain.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MigrationError {
    /// No registered step can advance the schema version.
    #[error("missing migration step from schema {from} to {}", to)]
    MissingStep {
        /// Current version.
        from: u32,
        /// Required next version.
        to: u32,
    },
    /// A registered step failed.
    #[error("migration step from schema {from} failed: {detail}")]
    Failed {
        /// Version the failing step started from.
        from: u32,
        /// Step error detail.
        detail: String,
    },
}

/// Ordered set of registered migrations.
#[derive(Default)]
pub struct MigrationChain {
    steps: Vec<(u32, u32, MigrationStep)>,
}

impl MigrationChain {
    /// An empty chain (production ships none for v1).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Registers a migration step. `to` must be greater than `from`.
    pub fn register(&mut self, from: u32, to: u32, step: MigrationStep) {
        debug_assert!(to > from, "migration must advance the version");
        self.steps.push((from, to, step));
    }

    /// Applies steps until `value` reaches schema `to`.
    ///
    /// # Errors
    ///
    /// Returns [`MigrationError::MissingStep`] if the chain has a gap, or
    /// [`MigrationError::Failed`] if a step returns an error.
    pub fn migrate(&self, mut value: Value, from: u32, to: u32) -> Result<Value, MigrationError> {
        let mut version = from;
        while version < to {
            // Prefer a single-step advance; otherwise take the first step from
            // this version.
            let step = self
                .steps
                .iter()
                .find(|(a, b, _)| *a == version && *b == version + 1)
                .or_else(|| self.steps.iter().find(|(a, _, _)| *a == version));
            let Some((start, next, apply)) = step else {
                return Err(MigrationError::MissingStep {
                    from: version,
                    to: version + 1,
                });
            };
            if *next <= *start {
                return Err(MigrationError::Failed {
                    from: version,
                    detail: "migration did not advance the version".to_owned(),
                });
            }
            value = apply(value).map_err(|detail| MigrationError::Failed {
                from: *start,
                detail,
            })?;
            version = *next;
        }
        Ok(value)
    }
}
