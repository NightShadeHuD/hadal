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
use hadal_core::SaveStore;
use hadal_core::save::{FORMAT_MARKER, SavePaths};
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

/// Verifies a save file using the core library's byte-level verification.
///
/// This uses [`verify_save_bytes`] which validates the stored bytes exactly
/// (format marker, schema gate, checksum over raw JSON, full deserialization
/// with migration), matching what [`parse_envelope`] does for load/import.
fn verify_save(path: &std::path::Path) -> Result<(), String> {
    let (schema, app_version, save_counter) =
        hadal_core::verify_save_bytes(path).map_err(|err| err.to_string())?;

    println!("format: {FORMAT_MARKER}");
    println!("schema_version: {schema}");
    println!("app_version: {app_version}");
    println!("save_counter: {save_counter}");
    println!("checksum: valid");

    Ok(())
}

// ---------------------------------------------------------------------
// Interactive TUI
// ---------------------------------------------------------------------

fn run_tui(cli: &Cli) -> ExitCode {
    run_tui_with(cli, terminal::is_interactive)
}

fn run_tui_with<F: Fn() -> bool>(cli: &Cli, is_tty: F) -> ExitCode {
    let opts = app::resolve_options(cli);

    // Provision only for a real game. Demo is a seeded, non-persistent preview:
    // it must never create a data directory or lock/save file.
    if provisions_data_dir(cli)
        && let Err(err) = provision_data_dir(&opts.data_dir)
    {
        eprintln!(
            "could not create data directory {}: {}",
            sanitize(&opts.data_dir.display().to_string()),
            err
        );
        return ExitCode::FAILURE;
    }

    if let Err(err) = ensure_interactive(is_tty) {
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

/// Whether this launch may write persistent state.
fn provisions_data_dir(cli: &Cli) -> bool {
    !cli.demo
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
    use hadal_core::Envelope;
    use hadal_core::GameState;

    fn write_envelope(path: &std::path::Path, envelope: &Envelope) {
        let bytes = serde_json::to_vec(envelope).expect("serialize envelope");
        std::fs::write(path, bytes).expect("write envelope");
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
    fn verify_accepts_v1_save() {
        // P1-01: verify must accept a valid v1 (pre-migration) save and
        // report the stored schema_version (1), not the migrated one (2).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("v1.json");
        let state = GameState::new();
        let mut v1 = serde_json::to_value(&state).unwrap();
        // Remove the tutorial field so it looks like a real v1 save.
        if let Some(obj) = v1.as_object_mut() {
            let meta = obj.get_mut("meta").unwrap().as_object_mut().unwrap();
            meta.remove("tutorial");
        }
        let mut envelope = serde_json::json!({
            "format": "hadal-save",
            "schema_version": 1,
            "app_version": hadal_core::save::APP_VERSION,
            "saved_at_unix": 1000,
            "save_counter": 1,
            "state": v1,
        });
        // Compute checksum over the envelope minus the checksum field.
        let checksum_bytes = serde_json::to_vec(&envelope).unwrap();
        let hex = hadal_core::hex_digest(&checksum_bytes);
        envelope["checksum"] = format!("sha256:{hex}").into();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        std::fs::write(&path, bytes).unwrap();
        assert!(verify_save(&path).is_ok(), "v1 save must verify");
        // Assert the stored schema is 1 (not migrated to 2).
        let (schema, _, _) = hadal_core::verify_save_bytes(&path).expect("verify_save_bytes");
        assert_eq!(schema, 1, "stored schema_version must be 1 for a v1 file");
    }

    #[test]
    fn verify_accepts_v2_envelope() {
        // A v2 (current-schema) sealed envelope must verify and report schema 2.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("v2.json");
        let envelope = Envelope::sealed(&GameState::new(), 1_000, 1);
        let bytes = serde_json::to_vec(&envelope).unwrap();
        std::fs::write(&path, bytes).unwrap();
        assert!(verify_save(&path).is_ok());
        let (schema, _, _) = hadal_core::verify_save_bytes(&path).expect("verify_save_bytes");
        assert_eq!(schema, 2, "stored schema_version must be 2 for a v2 file");
    }

    #[test]
    fn verify_rejects_tampered_v1_checksum() {
        // Same v1 save but with a tampered checksum → must fail.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("v1_tampered.json");
        let state = GameState::new();
        let mut v1 = serde_json::to_value(&state).unwrap();
        if let Some(obj) = v1.as_object_mut() {
            let meta = obj.get_mut("meta").unwrap().as_object_mut().unwrap();
            meta.remove("tutorial");
        }
        let envelope = serde_json::json!({
            "format": "hadal-save",
            "schema_version": 1,
            "app_version": hadal_core::save::APP_VERSION,
            "saved_at_unix": 1000,
            "save_counter": 1,
            "checksum": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "state": v1,
        });
        let bytes = serde_json::to_vec(&envelope).unwrap();
        std::fs::write(&path, bytes).unwrap();
        assert!(verify_save(&path).is_err(), "tampered v1 must be rejected");
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
    fn verify_rejects_too_large_file() {
        // P1-02: verify must enforce the 4 MiB size gate.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("big.json");
        // Write a file > 4 MiB with minimal JSON so it passes JSON parsing
        // but is rejected by the size gate.
        let payload = serde_json::json!({"format":"hadal-save","schema_version":2});
        let header = serde_json::to_vec(&payload).unwrap();
        let mut bytes = header.clone();
        bytes.extend(vec![b' '; 4 * 1024 * 1024 + 1024]);
        std::fs::write(&path, bytes).unwrap();
        let err = verify_save(&path).expect_err("must reject oversized file");
        assert!(err.contains("too large"), "{err}");
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

    #[test]
    fn provisioning_decision_is_demo_aware() {
        let demo = Cli::try_parse_from(["hadal", "--demo"]).expect("demo cli");
        let normal = Cli::try_parse_from(["hadal"]).expect("normal cli");
        assert!(!provisions_data_dir(&demo));
        assert!(provisions_data_dir(&normal));
    }

    #[test]
    fn demo_never_provisions_but_non_tty_normal_does() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("fresh").join("nested");
        let nested_text = nested.to_str().expect("nested utf8").to_owned();

        let demo = Cli::try_parse_from(["hadal", "--demo", "--data-dir", nested_text.as_str()])
            .expect("demo cli");
        assert!(!provisions_data_dir(&demo));
        let _ = run_tui_with(&demo, || false);
        assert!(!nested.exists(), "demo must not create the data dir");

        let normal =
            Cli::try_parse_from(["hadal", "--data-dir", nested_text.as_str()]).expect("normal cli");
        assert!(provisions_data_dir(&normal));
        let _ = run_tui_with(&normal, || false);
        assert!(
            nested.is_dir(),
            "a normal non-TTY launch provisions before the gate exits"
        );
        std::fs::remove_dir_all(&nested).expect("cleanup nested data dir");
        assert!(!nested.exists());
    }
}
