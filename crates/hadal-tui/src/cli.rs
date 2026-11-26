//! HADAL CLI argument parsing.
//!
//! `hadal` launches the TUI. Headless subcommands are `save path`,
//! `export <path>`, `import <path>`, and `verify <path>`.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::theme::{ColorDepth, Theme};

/// HADAL — terminal incremental game.
#[derive(Debug, Parser)]
#[command(
    name = "hadal",
    about = "HADAL — terminal incremental game",
    disable_version_flag = true
)]
pub struct Cli {
    /// Data directory (overrides HADAL_DATA_DIR and the platform default).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Run in demo mode: no save, no lock.
    #[arg(long)]
    pub demo: bool,

    /// Use ASCII-only glyphs.
    #[arg(long)]
    pub ascii: bool,

    /// Visual theme (overrides config).
    #[arg(long, value_enum)]
    pub theme: Option<ThemeArg>,

    /// Color depth (overrides config).
    #[arg(long, value_enum)]
    pub color: Option<ColorArg>,

    /// Disable mouse input.
    #[arg(long)]
    pub no_mouse: bool,

    /// Print version and exit.
    #[arg(long, help = "Print version and exit")]
    pub version: bool,

    /// Headless subcommand (runs without a TTY).
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Theme values accepted on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ThemeArg {
    Abyss,
    #[value(name = "high-contrast")]
    HighContrast,
    #[value(name = "amber-crt")]
    AmberCrt,
    Mono,
}

impl From<ThemeArg> for Theme {
    fn from(value: ThemeArg) -> Self {
        match value {
            ThemeArg::Abyss => Theme::Abyss,
            ThemeArg::HighContrast => Theme::HighContrast,
            ThemeArg::AmberCrt => Theme::AmberCrt,
            ThemeArg::Mono => Theme::Mono,
        }
    }
}

/// Color-depth values accepted on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorArg {
    #[value(name = "auto")]
    Auto,
    #[value(name = "truecolor")]
    TrueColor,
    #[value(name = "256")]
    Ansi256,
    #[value(name = "16")]
    Ansi16,
    #[value(name = "none")]
    None,
}

impl From<ColorArg> for ColorDepth {
    fn from(value: ColorArg) -> Self {
        match value {
            ColorArg::Auto => ColorDepth::Auto,
            ColorArg::TrueColor => ColorDepth::TrueColor,
            ColorArg::Ansi256 => ColorDepth::Ansi256,
            ColorArg::Ansi16 => ColorDepth::Ansi16,
            ColorArg::None => ColorDepth::None,
        }
    }
}

/// Headless subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Save-file utilities.
    Save {
        /// Save-file action.
        #[command(subcommand)]
        action: SaveAction,
    },
    /// Export the current save to a file.
    Export {
        /// Target file path.
        path: PathBuf,
    },
    /// Import a save from a file (validated before any write).
    Import {
        /// Source file path.
        path: PathBuf,
    },
    /// Verify a save file's format and checksum.
    Verify {
        /// Save file path.
        path: PathBuf,
    },
}

/// Actions under `save`.
#[derive(Debug, Subcommand)]
pub enum SaveAction {
    /// Print the resolved save-file path.
    Path,
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_invocation_as_tui() {
        let cli = Cli::try_parse_from(["hadal"]).expect("parse");
        assert!(cli.command.is_none());
        assert!(!cli.demo);
        assert!(cli.theme.is_none());
        assert!(cli.color.is_none());
    }

    #[test]
    fn parses_every_flag() {
        let cli = Cli::try_parse_from([
            "hadal",
            "--data-dir",
            "/tmp/hadal",
            "--demo",
            "--ascii",
            "--theme",
            "amber-crt",
            "--color",
            "256",
            "--no-mouse",
        ])
        .expect("parse");
        assert_eq!(cli.data_dir, Some(PathBuf::from("/tmp/hadal")));
        assert!(cli.demo);
        assert!(cli.ascii);
        assert_eq!(cli.theme, Some(ThemeArg::AmberCrt));
        assert_eq!(cli.color, Some(ColorArg::Ansi256));
        assert!(cli.no_mouse);
    }

    #[test]
    fn parses_theme_aliases() {
        for (value, want) in [
            ("abyss", ThemeArg::Abyss),
            ("high-contrast", ThemeArg::HighContrast),
            ("amber-crt", ThemeArg::AmberCrt),
            ("mono", ThemeArg::Mono),
        ] {
            let cli = Cli::try_parse_from(["hadal", "--theme", value]).expect("parse");
            assert_eq!(cli.theme, Some(want));
        }
    }

    #[test]
    fn parses_color_aliases() {
        for (value, want) in [
            ("auto", ColorArg::Auto),
            ("truecolor", ColorArg::TrueColor),
            ("256", ColorArg::Ansi256),
            ("16", ColorArg::Ansi16),
            ("none", ColorArg::None),
        ] {
            let cli = Cli::try_parse_from(["hadal", "--color", value]).expect("parse");
            assert_eq!(cli.color, Some(want));
        }
    }

    #[test]
    fn rejects_unknown_theme() {
        assert!(Cli::try_parse_from(["hadal", "--theme", "neon"]).is_err());
    }

    #[test]
    fn parses_save_path() {
        let cli = Cli::try_parse_from(["hadal", "save", "path"]).expect("parse");
        match cli.command {
            Some(Command::Save {
                action: SaveAction::Path,
            }) => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_export_import_verify() {
        let export = Cli::try_parse_from(["hadal", "export", "out.json"]).expect("parse");
        assert!(matches!(
            export.command,
            Some(Command::Export { path }) if path.as_path() == std::path::Path::new("out.json")
        ));
        let import = Cli::try_parse_from(["hadal", "import", "in.json"]).expect("parse");
        assert!(matches!(
            import.command,
            Some(Command::Import { path }) if path.as_path() == std::path::Path::new("in.json")
        ));
        let verify = Cli::try_parse_from(["hadal", "verify", "check.json"]).expect("parse");
        assert!(matches!(
            verify.command,
            Some(Command::Verify { path }) if path.as_path() == std::path::Path::new("check.json")
        ));
    }

    #[test]
    fn missing_path_is_an_error() {
        assert!(Cli::try_parse_from(["hadal", "export"]).is_err());
        assert!(Cli::try_parse_from(["hadal", "import"]).is_err());
        assert!(Cli::try_parse_from(["hadal", "verify"]).is_err());
        assert!(Cli::try_parse_from(["hadal", "save"]).is_err());
    }

    #[test]
    fn parses_version_flag() {
        let cli = Cli::try_parse_from(["hadal", "--version"]).expect("parse");
        assert!(cli.version);
        assert!(cli.command.is_none());
    }
}
