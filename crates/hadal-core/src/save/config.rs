//! User configuration, stored separately from the game save.
//!
//! Config is not checksummed and is not part of `GameState`. Unknown fields are
//! ignored for forward compatibility; a corrupt file falls back to defaults and
//! is never deleted.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Visual theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Default deep-sea phosphor palette.
    #[default]
    Abyss,
    /// High-contrast palette.
    HighContrast,
    /// Amber CRT palette.
    AmberCrt,
    /// Monochrome palette.
    Mono,
}

/// Terminal color-depth override.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorDepth {
    /// Detect automatically.
    #[default]
    Auto,
    /// 16 colors.
    Ansi16,
    /// 256 colors.
    Ansi256,
    /// 24-bit color.
    TrueColor,
    /// No color output (monochrome).
    None,
}

/// Error parsing a [`ColorDepth`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown color depth: {0}")]
pub struct ParseColorDepthError(pub String);

impl fmt::Display for ColorDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Auto => "auto",
            Self::Ansi16 => "ansi16",
            Self::Ansi256 => "ansi256",
            Self::TrueColor => "true_color",
            Self::None => "none",
        };
        f.write_str(text)
    }
}

impl FromStr for ColorDepth {
    type Err = ParseColorDepthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "ansi16" => Ok(Self::Ansi16),
            "ansi256" => Ok(Self::Ansi256),
            "true_color" => Ok(Self::TrueColor),
            "none" => Ok(Self::None),
            _ => Err(ParseColorDepthError(s.to_owned())),
        }
    }
}

/// User configuration.
///
/// Missing fields fall back to defaults so older configs keep loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Theme.
    pub theme: Theme,
    /// Reduce motion: disables transitions/particles/typewriter.
    pub reduce_motion: bool,
    /// Prefer ASCII-only glyphs.
    pub ascii_only: bool,
    /// Enable mouse input.
    pub mouse: bool,
    /// Color-depth override.
    pub color_depth: ColorDepth,
    /// Optional terminal bell on major transitions.
    pub bell: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Abyss,
            reduce_motion: false,
            ascii_only: false,
            mouse: true,
            color_depth: ColorDepth::Auto,
            bell: false,
        }
    }
}

/// Warning produced while loading config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigWarning {
    /// The file was unreadable or invalid and defaults were used.
    Corrupt {
        /// The offending path (never deleted).
        path: std::path::PathBuf,
        /// Human-readable detail.
        detail: String,
    },
}

/// Loads config from `path`.
///
/// A missing file yields defaults with no warning. A corrupt file yields
/// defaults plus a [`ConfigWarning`]; the file is left untouched.
pub fn load_config(path: &Path) -> (Config, Option<ConfigWarning>) {
    match std::fs::read(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
        Err(err) => (
            Config::default(),
            Some(ConfigWarning::Corrupt {
                path: path.to_path_buf(),
                detail: err.to_string(),
            }),
        ),
        Ok(bytes) => match serde_json::from_slice::<Config>(&bytes) {
            Ok(config) => (config, None),
            Err(err) => (
                Config::default(),
                Some(ConfigWarning::Corrupt {
                    path: path.to_path_buf(),
                    detail: err.to_string(),
                }),
            ),
        },
    }
}

/// Saves config to `path`, creating parent directories as needed.
///
/// # Errors
///
/// Returns an I/O error if the directory or file cannot be written.
pub fn save_config(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let temp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)?;
    Ok(())
}
