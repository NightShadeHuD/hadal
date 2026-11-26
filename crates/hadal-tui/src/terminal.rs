//! Terminal safety: TTY check, RAII raw-mode guard, panic hook for crash logging.
//!
//! We deliberately do not use `ratatui::init`, which installs its own panic
//! hook; the guard here owns setup/teardown and [`install_panic_hook`] owns
//! crash handling, so there is exactly one hook and one restoration path.

use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::panic;
use std::path::Path;

use crossterm::cursor::Show;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

// ---------------------------------------------------------------------
// TTY errors
// ---------------------------------------------------------------------

/// Error returned when interactive mode is requested without a TTY.
#[derive(Debug, thiserror::Error)]
#[error(
    "interactive mode requires a TTY on both stdin and stdout.\n\
     HADAL is a terminal game — run it from a real terminal.\n\
     For headless operations, use a subcommand: hadal save path, \
     hadal export <path>, hadal import <path>, or hadal verify <path>."
)]
pub struct NotInteractive;

/// Check whether both stdin and stdout are TTYs.
pub fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Enable or disable mouse capture on the live terminal.
///
/// Used when the SYSTEM setting changes; best-effort result for the caller.
pub fn set_mouse(enabled: bool) -> io::Result<()> {
    if enabled {
        crossterm::execute!(io::stdout(), EnableMouseCapture)
    } else {
        crossterm::execute!(io::stdout(), DisableMouseCapture)
    }
}

/// Enforce interactivity with an injected predicate.
///
/// The predicate keeps the check unit-testable without a real terminal.
pub fn ensure_interactive<F: Fn() -> bool>(is_tty: F) -> Result<(), NotInteractive> {
    if is_tty() {
        Ok(())
    } else {
        Err(NotInteractive)
    }
}

// ---------------------------------------------------------------------
// RAII terminal guard
// ---------------------------------------------------------------------

/// RAII guard that enables raw mode + alternate screen (and optionally mouse).
///
/// Restoration happens exactly once, on explicit [`TerminalGuard::restore`] or
/// on drop, whichever comes first; repeated restores are no-ops.
pub struct TerminalGuard {
    restored: bool,
    restore_fn: fn(),
}

impl TerminalGuard {
    /// Enter raw mode, the alternate screen, and optionally mouse capture.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if raw mode or the alternate screen cannot be entered.
    pub fn new(mouse: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        crossterm::execute!(io::stdout(), EnterAlternateScreen)?;
        if mouse {
            crossterm::execute!(io::stdout(), EnableMouseCapture)?;
        }
        Ok(Self {
            restored: false,
            restore_fn: restore_real,
        })
    }

    /// Create a guard with an injected restore function for tests.
    #[cfg(test)]
    pub fn new_test(restore_fn: fn()) -> Self {
        Self {
            restored: false,
            restore_fn,
        }
    }

    /// Restore the terminal. Idempotent.
    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        (self.restore_fn)();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn restore_real() {
    let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
    let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(io::stdout(), Show);
}

// ---------------------------------------------------------------------
// Panic hook
// ---------------------------------------------------------------------

/// Install a panic hook that restores the terminal and writes a crash log.
///
/// The log is `<data_dir>/crash-<unix>.log` and records the panic message,
/// location, thread, and data directory. The previously installed hook is
/// chained afterwards.
pub fn install_panic_hook(data_dir: &Path) {
    let previous = panic::take_hook();
    let dir = data_dir.to_path_buf();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_safely();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let crash_path = dir.join(format!("crash-{timestamp}.log"));
        if let Some(parent) = crash_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = File::create(&crash_path) {
            let _ = writeln!(file, "HADAL crash report");
            let _ = writeln!(file, "Time: {timestamp}");
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_owned());
            let _ = writeln!(file, "Message: {message}");
            if let Some(loc) = info.location() {
                let _ = writeln!(
                    file,
                    "Location: {}:{}:{}",
                    loc.file(),
                    loc.line(),
                    loc.column()
                );
            }
            let thread = std::thread::current();
            if let Some(name) = thread.name() {
                let _ = writeln!(file, "Thread: {name}");
            }
            let _ = writeln!(file);
            let _ = writeln!(
                file,
                "Recovery: the terminal was restored. Restart HADAL; if it recurs, \
                 move this log aside and report it."
            );
            let _ = writeln!(file, "Data directory: {}", dir.display());
        }

        previous(info);
    }));
}

fn restore_terminal_safely() -> io::Result<()> {
    let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
    let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(io::stdout(), Show);
    Ok(())
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static RESTORES: AtomicU32 = AtomicU32::new(0);

    fn count_restore() {
        RESTORES.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn guard_restores_exactly_once_on_drop() {
        RESTORES.store(0, Ordering::SeqCst);
        {
            let guard = TerminalGuard::new_test(count_restore);
            assert_eq!(RESTORES.load(Ordering::SeqCst), 0);
            drop(guard);
        }
        assert_eq!(RESTORES.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_restore_then_drop_is_idempotent() {
        RESTORES.store(0, Ordering::SeqCst);
        let mut guard = TerminalGuard::new_test(count_restore);
        guard.restore();
        guard.restore();
        drop(guard);
        assert_eq!(RESTORES.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn non_tty_is_typed_error_with_guidance() {
        let err = ensure_interactive(|| false).expect_err("no tty");
        let msg = err.to_string();
        assert!(msg.contains("TTY"), "{msg}");
        assert!(msg.contains("hadal save path"), "{msg}");
        assert!(ensure_interactive(|| true).is_ok());
    }
}
