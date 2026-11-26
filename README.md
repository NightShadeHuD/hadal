# HADAL — Relay K-9

A terminal-native deep-sea signals incremental. You operate Relay K-9, a derelict station 7,912 m down in the Kermadec Trench, coaxing Echo out of a repeating carrier wave. Each Descent sinks the station 500 m and rewrites the run. The terminal is the station console.

**Status:** v0.1.0 — M1 vertical slice (pulse, buy, tune, descend, hulls, offline progress).

## Install

Download release archives from the [Releases page](https://github.com/NightShadeHuD/hadal/releases); they will appear there once a tag is cut. Installers use `HADAL_DOWNLOAD_BASE` to locate the archive:

- **Linux:** `scripts/install.sh` — verifies SHA-256, installs to `~/.local/bin` by default.
- **Windows:** `scripts/install.ps1` — verifies with `Get-FileHash`, installs to `%LOCALAPPDATA%\hadal\bin`.

## Build from source

Requires Rust 1.89+ (edition 2024).

```
cargo build --release -p hadal
```

## Run

```
cargo run -p hadal --
```

Flags:

| Flag | Description |
|---|---|
| `--demo` | Seeded late-game state; no save, no lock |
| `--data-dir PATH` | Override data directory |
| `--ascii` | ASCII-only glyphs |
| `--theme NAME` | Theme: `abyss`, `high-contrast`, `amber-crt`, `mono` |
| `--color DEPTH` | Color depth: `auto`, `truecolor`, `256`, `16`, `none` |
| `--no-mouse` | Disable mouse input |
| `--version` | Print version and exit |

Headless subcommands:

| Command | Description |
|---|---|
| `save path` | Print the resolved save-file path |
| `export PATH` | Export the current save to a file |
| `import PATH` | Import a save from a file (validated before any write) |
| `verify PATH` | Verify a save file's format and checksum |

`hadal-sim` is a separate binary: `cargo run -p hadal-sim --release -- --help`.

## Controls

The keymap is screen-independent; the sections below show where each key is meaningful.

Global:

| Key | Action |
|---|---|
| `1`–`4` | Switch screen (SONAR, DESCENT, ARCHIVE, SYSTEM) |
| `Tab` / `Shift+Tab` | Cycle tabs |
| `?` | Help overlay |
| `Esc` | Close overlay / cancel / leave focus |
| `Ctrl+S` / `s` | Save immediately |
| `q` | Save and quit |
| `Ctrl+C` | Best-effort save and quit |

SONAR:

| Key | Action |
|---|---|
| `SPACE` | Pulse (requires a full capacitor) |
| `f` | Cycle the tuned carrier band |
| `h` / `H` | Buy 1 / max Hydrophone |
| `d` / `D` | Buy 1 / max Drifter |
| `m` / `M` | Buy 1 / max Sonar Mast |
| `c` / `C` | Buy 1 / max Tidal Core |
| `u` | Enter Signal Chain (upgrade) focus |
| `Up` / `Down` | Move focus |
| `Enter` | Buy the focused upgrade |

DESCENT:

| Key | Action |
|---|---|
| `Up` / `Down` | Move hull focus |
| `Enter` | Buy the focused hull |
| `p` | Open the Descent confirmation (also opens DESCENT from SONAR) |
| `y` | Confirm the pending Descent |
| `+` / `-` (or `=` / `_`) | Adjust the Ballast Governor target; `Shift` steps by 10 |

ARCHIVE:

| Key | Action |
|---|---|
| `Up` / `Down` | Scroll |
| `PgUp` / `PgDn` | Page |

SYSTEM:

| Key | Action |
|---|---|
| `Up` / `Down` | Move focus |
| `Enter` / `Space` | Change the focused setting |
| `t` | Toggle the focused automation |
| `e` | Export |
| `i` | Import |
| `R` | Reset (typed `DISCARD` confirmation) |

Text-entry overlays consume all keys except `Esc` (cancel) and `Enter` (submit).

## Screens and overlays

- **Boot/title** (first run): wordmark, station/depth readout, typewriter boot lines; any key skips.
- **SONAR (1):** resource band (Echo/rate, Peak, Fathom, next Descent threshold), sonar scope, pulse capacitor, carrier strip, generator rows, Signal Chain list.
- **DESCENT (2):** depth gauge, current/next Fathom gain, hull tree grouped by branch, Ballast Governor target, confirmation dialog.
- **ARCHIVE (3):** scrollable logs, achievement grid with lock state, lifetime stats.
- **SYSTEM (4):** theme/reduce-motion/ASCII/mouse/color/bell settings, per-generator auto-buy toggles, an owned-hull automation summary (with the governor target), and save/export/import/reset via `s`/`e`/`i`/`R`. Settings apply live and persist to `config.json`.

Overlays: help, offline summary, descent confirmation, import/export text input, reset confirmation, too-small terminal.

## Numbers

Amounts floor; costs ceil; affordability compares exact values.

| Range | Display |
|---|---|
| `< 1e6` | grouped integer with commas (`999,999`) |
| `1e6` to `< 1e36` | three significant figures with suffix `M/B/T/Qa/Qi/Sx/Sp/Oc/No/Dc` (`1.00M`, `12.3M`, `123M`, `1.00Dc`); Dc covers the 1e33–1e36 group |
| `≥ 1e36` | scientific, three significant figures (`1.00e36`) |

Rates show two decimals below 1e6. Display only; underlying values stay exact.

## Save locations

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\hadal` |
| macOS | `~/Library/Application Support/hadal` |
| Linux | `$XDG_DATA_HOME/hadal` or `~/.local/share/hadal` |

Override with `--data-dir PATH` or the `HADAL_DATA_DIR` environment variable. Files: `save.json`, `save.bak.json`, `save.lock`, `config.json`, `exports/`, and `crash-<unix>.log` on panic.

## Save safety

- Each save carries a SHA-256 checksum over the canonical compact JSON (minus the checksum field).
- Writes are atomic (temp file, `fsync`, rename); the previous save is kept as `save.bak.json`.
- A corrupt primary falls back to the backup and is preserved as `save.corrupt-<unix>.json`, never overwritten.
- A save from a newer schema version is refused with a readable recovery message.
- A single-instance advisory lock (`save.lock`, via `File::try_lock`) makes a second instance exit cleanly.
- Imports reject files over 4 MiB and validate format, schema and checksum before writing.

## Offline progress

Offline runs the exact online tick with automation only: auto-pulse, auto-tune, auto-buy, auto-upgrade and auto-descent happen only if the corresponding hull is owned. The rate is 100% of automated production.

- Base cap **8 hours**; hulls extend it to **24 h** (Long Storage), **72 h** (Extended Ballast), **168 h** (Abyssal Reservoir).
- Negative clock jumps credit nothing; long absences are capped.
- A welcome-back summary shows time credited, cap hit, Echo delta, purchases, descents, Fathoms gained and achievements unlocked.
- The game saves immediately after applying offline progress, before accepting input.

## Uninstall

Delete the binary and the data directory for your platform (see above).

## Platform notes

- **Linux:** a single static `linux-musl` binary covers Ubuntu 24+, Debian, Arch, Fedora and Gentoo.
- **Windows:** Windows Terminal is recommended.
- **Legacy consoles:** `--ascii` provides a full fallback without block/box glyphs.
- **Unsigned artifacts:** no signing certificates are available. macOS may show a quarantine prompt; remove with `xattr -d com.apple.quarantine hadal`. Windows may show SmartScreen; proceed manually. No notarization claim is made.

## Privacy

HADAL makes no network calls and collects no telemetry. All state lives in the local save file described above.

## Verification status

Windows verified locally (release build, all test suites, simulator pacing, and CLI smoke via `--version`, `--help`, `save path`). CI passes on Ubuntu, Windows and macOS — fmt, clippy, workspace tests, MSRV 1.89, release pacing, and `cargo-deny` (see the [Actions page](https://github.com/NightShadeHuD/hadal/actions)). No release tag exists yet, so no release artifacts are claimed.

## License

Dual **MIT** OR **Apache-2.0**.
