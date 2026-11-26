#![forbid(unsafe_code)]
//! HADAL TUI — terminal user interface.
//!
//! Launches the ratatui console when no subcommand is given, or runs a
//! headless save utility (`save path`, `export`, `import`, `verify`).

mod app;
mod boot;
mod cli;
mod glyphs;
mod input;
mod model;
mod render;
mod sanitize;
mod terminal;
mod theme;

#[cfg(test)]
mod golden;

use std::process::ExitCode;

use clap::Parser;
use hadal_core::save::{FORMAT_MARKER, SUPPORTED_SCHEMA, SavePaths, checksum_of};
use hadal_core::{Envelope, SaveStore};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::cli::{Cli, Command, SaveAction};
use crate::sanitize::sanitize;
use crate::terminal::ensure_interactive;

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.version {
        println!("HADAL v{}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match cli.command.as_ref() {
        Some(command) => run_headless(&cli, command),
        None => run_tui(&cli),
    }
}

// ---------------------------------------------------------------------
// Headless subcommands
// ---------------------------------------------------------------------

fn run_headless(cli: &Cli, command: &Command) -> ExitCode {
    match execute_headless(cli, command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn execute_headless(cli: &Cli, command: &Command) -> Result<(), String> {
    let data_dir = hadal_core::save::resolve_data_dir(cli.data_dir.as_deref());
    match command {
        Command::Save {
            action: SaveAction::Path,
        } => {
            println!(
                "{}",
                sanitize(&SavePaths::new(&data_dir).save().display().to_string())
            );
            Ok(())
        }
        Command::Export { path } => {
            let mut store = SaveStore::new(&data_dir);
            let now = app::now_unix();
            let loaded = store
                .load(now)
                .map_err(|err| format!("could not load current save: {err}"))?;
            let info = store
                .export_to_path(&loaded.state, path, now)
                .map_err(|err| format!("export failed: {err}"))?;
            println!("exported to {}", sanitize(&info.path.display().to_string()));
            Ok(())
        }
        Command::Import { path } => {
            let mut store = SaveStore::new(&data_dir);
            let now = app::now_unix();
            store
                .import_from_path(path, now)
                .map_err(|err| format!("import failed: {err}"))?;
            println!("imported {}", sanitize(&path.display().to_string()));
            Ok(())
        }
        Command::Verify { path } => verify_save(path),
    }
}

fn verify_save(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|err| {
        format!(
            "cannot read {}: {}",
            sanitize(&path.display().to_string()),
            sanitize(&err.to_string())
        )
    })?;

    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|err| format!("invalid save JSON: {err}"))?;

    let format = value
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    if format != FORMAT_MARKER {
        return Err(format!("not a HADAL save (format: {})", sanitize(&format)));
    }

    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if schema > SUPPORTED_SCHEMA {
        return Err(format!(
            "save schema {schema} is newer than supported {SUPPORTED_SCHEMA}; update HADAL"
        ));
    }

    let envelope: Envelope =
        serde_json::from_value(value).map_err(|err| format!("invalid save envelope: {err}"))?;
    let computed = checksum_of(&envelope);
    let checksum_ok = computed == envelope.checksum;

    println!("format: {}", sanitize(&envelope.format));
    println!("schema_version: {}", envelope.schema_version);
    println!("app_version: {}", sanitize(&envelope.app_version));
    println!("save_counter: {}", envelope.save_counter);
    println!("saved_at_unix: {}", envelope.saved_at_unix);

    if checksum_ok {
        println!("checksum: valid");
        Ok(())
    } else {
        println!(
            "checksum: MISMATCH (stored {}, computed {computed})",
            sanitize(&envelope.checksum)
        );
        Err("checksum mismatch".to_owned())
    }
}

// ---------------------------------------------------------------------
// Interactive TUI
// ---------------------------------------------------------------------

fn run_tui(cli: &Cli) -> ExitCode {
    let opts = app::resolve_options(cli);

    // Provision the data directory before any early exit, so a non-TTY launch
    // (or a lock failure) still leaves the directory in place. An absent
    // directory is a fresh install, not an error.
    if let Err(err) = provision_data_dir(&opts.data_dir) {
        eprintln!(
            "could not create data directory {}: {}",
            sanitize(&opts.data_dir.display().to_string()),
            err
        );
        return ExitCode::FAILURE;
    }

    if let Err(err) = ensure_interactive(terminal::is_interactive) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    terminal::install_panic_hook(&opts.data_dir);

    let now = app::now_unix();
    let session = match app::Session::start(&opts, now) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("{}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };
    let mouse = session.mouse;

    let mut guard = match terminal::TerminalGuard::new(mouse) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!(
                "could not initialize terminal: {}",
                sanitize(&err.to_string())
            );
            return ExitCode::FAILURE;
        }
    };

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut term = match Terminal::new(backend) {
        Ok(term) => term,
        Err(err) => {
            guard.restore();
            eprintln!("could not create terminal: {}", sanitize(&err.to_string()));
            return ExitCode::FAILURE;
        }
    };

    let result = app::run(session, &mut term, opts.bell);
    guard.restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("terminal error: {}", sanitize(&err.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// Create the data directory (and parents) before the interactive session
/// starts. An absent directory is a fresh install, so this is not an error;
/// a directory that exists as a regular file is reported by the OS error.
fn provision_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

// ---------------------------------------------------------------------
// Headless verify tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod headless_tests {
    use super::*;
    use hadal_core::GameState;

    fn write_envelope(path: &std::path::Path, envelope: &Envelope) {
        let bytes = serde_json::to_vec(envelope).expect("serialize envelope");
        std::fs::write(path, bytes).expect("write envelope");
    }

    #[test]
    fn verify_accepts_valid_envelope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("save.json");
        write_envelope(&path, &Envelope::sealed(&GameState::new(), 1_000, 1));
        assert!(verify_save(&path).is_ok());
    }

    #[test]
    fn verify_rejects_bad_checksum() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("save.json");
        let mut envelope = Envelope::sealed(&GameState::new(), 1_000, 1);
        envelope.checksum = "sha256:0000".to_owned();
        write_envelope(&path, &envelope);
        assert!(verify_save(&path).is_err());
    }

    #[test]
    fn verify_rejects_wrong_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("other.json");
        std::fs::write(&path, br#"{"format":"other","schema_version":1}"#).expect("write");
        let err = verify_save(&path).expect_err("must reject");
        assert!(err.contains("not a HADAL save"), "{err}");
    }

    #[test]
    fn verify_rejects_future_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("future.json");
        std::fs::write(&path, br#"{"format":"hadal-save","schema_version":99}"#).expect("write");
        let err = verify_save(&path).expect_err("must reject");
        assert!(err.contains("newer than supported"), "{err}");
    }

    #[test]
    fn verify_reports_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(verify_save(&dir.path().join("absent.json")).is_err());
    }

    #[test]
    fn headless_export_verify_import_roundtrip() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");

        // Seed a valid save in the source data directory.
        let mut store = SaveStore::new(src.path());
        store.save(&GameState::new(), 1_000).expect("seed save");
        drop(store);

        let export_path = src.path().join("exported.json");
        let cli = Cli::try_parse_from([
            "hadal",
            "--data-dir",
            src.path().to_str().expect("src utf8"),
            "export",
            export_path.to_str().expect("export utf8"),
        ])
        .expect("cli");
        execute_headless(&cli, cli.command.as_ref().expect("command")).expect("export");
        assert!(export_path.exists());

        let verify = Cli::try_parse_from([
            "hadal",
            "verify",
            export_path.to_str().expect("export utf8"),
        ])
        .expect("cli");
        execute_headless(&verify, verify.command.as_ref().expect("command")).expect("verify");

        let import = Cli::try_parse_from([
            "hadal",
            "--data-dir",
            dst.path().to_str().expect("dst utf8"),
            "import",
            export_path.to_str().expect("export utf8"),
        ])
        .expect("cli");
        execute_headless(&import, import.command.as_ref().expect("command")).expect("import");
        assert!(SavePaths::new(dst.path()).save().exists());
    }

    #[test]
    fn provision_creates_absent_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("fresh").join("nested");
        assert!(!nested.exists());
        provision_data_dir(&nested).expect("create data dir");
        assert!(nested.is_dir(), "absent data dir must be created");
    }

    #[test]
    fn provision_leaves_existing_dir_intact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("existing");
        std::fs::create_dir_all(&path).expect("pre-create");
        std::fs::write(path.join("save.json"), b"{}").expect("write marker");

        provision_data_dir(&path).expect("idempotent");

        assert!(path.join("save.json").exists(), "contents preserved");
    }

    #[test]
    fn provision_reports_file_at_data_dir_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("blocked");
        std::fs::write(&path, b"not a dir").expect("write file");

        assert!(
            provision_data_dir(&path).is_err(),
            "a regular file at the data-dir path must surface as an error"
        );
    }
}
