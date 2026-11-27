# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2] - 2026-11-27

### Fixed

- Default export wrote to a missing `exports/` directory; it is now provisioned at start.
- `--demo` wrote `config.json` and could contaminate real sessions; config persistence is disabled in demo.
- `verify` rejected valid v1 saves (it re-encoded instead of checking stored bytes) and bypassed the 4 MiB cap; it now verifies stored-bytes checksums, reports the stored schema, and enforces the cap.
- Toast/banner overlays erased the frame border, footer and coach band; they now draw inside the frame.
- Y/N confirmation dialogs were lowercase-only; `Y`/`N` are now accepted.
- `q` was inert in modals; it now quits them.
- Reduce-motion contacts still blinked; they now freeze.
- The offline WELCOME BACK overlay drew over the 400 ms boot; it is now deferred until boot finishes.
- A stale save temp could linger after SIGKILL; it is removed on start.
- The replay copy implied a restart; it now reads honestly.
- The sim's offline era-fathoms metric displayed 0; it now reports the real value.
- A stale view comment was corrected.
- `tutorial_active_system_replay_is_immediate` captured the clock once so a second-boundary straddle cannot trip offline catch-up (test determinism).

## [0.2.1] - 2026-11-27

### Fixed

- Boot intro now repaints while active, so the three `BOOT_BEATS` reveal at 800/1400/2000 ms
  and the radar/wave animate. `App::tick` marks the frame dirty every tick during boot instead
  of gating redraw on `boot_done`. Regression test `boot_stages_redraw_and_reveal_beats`
  asserts the rendered buffer at 900/1500/2100 ms.

## [0.2.0] - 2026-11-27

### Added

- **Graphical overhaul**: redesigned boot sequence (wordmark + radar dome + wave animation, staged beats), SONAR radar scope and carrier strip, signal map art in ARCHIVE.
- **Story/tutorial depth**: 13-step interactive tutorial in two chapters (FIRST SIGNAL, SECOND SIGNAL) with spotlight coach that waits for each action; narrative text expanded (MISSION, BOOT_BEATS).
- **DESCENT/ARCHIVE art**: graphical descent confirmation with depth visualization, ARCHIVE signal map, hull schematic with branch grouping.
- **Input gating**: tutorial actions gated until coach advances; nudge system with expiry and redraw.
- **Gate and receipt**: `cargo xtask gate` (fmt, clippy, tests, floor, ignore/path/no-network checks, content-digest receipt); pacing via `--release-pacing`, privacy via `cargo xtask privacy`; `gate-status` verifies receipt staleness; `tools/test-floor.txt` may only rise.

## [0.1.3] - 2026-11-26

### Added

- **Interactive 13-step tutorial** in two chapters (FIRST SIGNAL, SECOND SIGNAL): the coach
  waits for each action, highlights the exact control, shows live waiting/progress, and
  explains after the first and second Descent.
- **Persisted tutorial progress** (`meta.tutorial`) saved with the game; skip anytime, and
  replay from SYSTEM.
- **Save schema v2** with a real `1 -> 2` migration that marks existing saves' tutorial
  complete, so upgrading players are not dropped into step 0.

### Removed

- The first-run briefing wall (replaced by the interactive coach).

### Fixed

- The coach reserves layout space instead of covering its target; markers and columns align.
- Tutorial order teaches the Drifter before Impedance Match.
- The tuning step requires a real post-drift keypress.

## [0.1.2] - 2026-11-26

### Fixed

- A brand-new `--data-dir` (including nested paths) now works on first launch: the directory
  is created before the lock is acquired.
- Startup errors name the data-directory path and the reason (a regular file in the way, or a
  directory that cannot be created).
- An absent save directory is treated as a fresh install (`NoSave`) rather than an I/O error;
  an empty directory boots to the briefing.
- Exporting to a missing parent directory reports the path.

## [0.1.1] - 2026-11-26

### Added

- **First-run briefing** — a one-panel onboarding shown after boot for a new save, and always
  with `--demo`; it is derived from state and has no saved "seen" flag.
- **Scrollable in-game help** — `?` opens a briefing with BASICS → WHAT TO DO NOW → CONTROLS
  → DESCENT & FATHOMS → OFFLINE → NUMBERS → GLOSSARY → VERSION, including a 14-term glossary.
- **Always-on objective line** — SONAR shows one next action with its reason (pulse →
  affordable Hydrophone → Impedance Match → tune → reach 1.00M → descend → nearest hull).
- **Panel and selection explainers** — panels describe their contents at ≥100×30; a focused
  or hovered generator, upgrade or hull shows a plain-words effect and its exact cost.
- **Honest demo banner** — `--demo` shows a persistent `DEMO // NOTHING IS SAVED` banner and a
  demo briefing.

### Fixed

- Modal overlays no longer leave screen separators visible at the frame edge (the overlay
  band behind the popup is cleared).

## [0.1.0] - 2026-11-26

M1 vertical slice: a headless deterministic core, a full terminal client, a pacing
simulator, and the release tooling.

### Added

- **Decimal** — arbitrary-range number (`mantissa ∈ [1,10)`, `exponent10`) using only
  IEEE-754 `+ - * / sqrt`; no `NaN`/`Infinity` can enter state; ~15–17 significant
  digits; canonical zero; strict string serde; floor/ceil/rate human formatting with
  grouped integers and `M…Dc` suffixes to 1e33, scientific above.
- **Engine and tick** — fixed 10 Hz simulation with the ten-step order (carrier,
  capacitor, per-second automation, production, auto-pulse, peak, achievements/logs,
  governor, counters); action validation (`Pulse`, `SetTuning`, `BuyGenerator`,
  `BuyUpgrade`, `BuyHull`, `SetAutoBuy`, `SetAutoDescendTarget`, `Descend`);
  exact one/five/max purchase math; bounded event buffer; canonical SHA-256 state hash.
- **Content** — one table source of truth: 4 generators, 8 Signal Chain upgrades,
  17 hulls (Fractal Hull at 50,000 Fathoms), 11 achievements and 10 logs; stable
  string IDs; Descent gain from Peak Echo buckets.
- **Save** — schema-v1 envelope with `save_counter`; SHA-256 checksum over canonical
  compact JSON minus the checksum; atomic temp+fsync+rename write; backup fallback;
  corrupt-file preservation; migration chain with a synthetic-step injection point;
  single-instance advisory lock via `File::try_lock`; import/export with size,
  format, schema and checksum validation; separate non-checksummed `config.json`.
- **Offline** — exact-tick headless catch-up, automation only, hull-derived caps
  (8/24/72/168 h), 100% rate, `OfflineReport` summary, and an online/offline
  equivalence test pinning an identical state hash.
- **Simulator and pacing** — `hadal-sim` with `active`, `idle` and `offline` profiles,
  JSON/CSV output and `--assert`; a release-mode `pacing` integration test asserting
  the balance bands.
- **TUI** — boot, SONAR, DESCENT, ARCHIVE and SYSTEM screens; dive transition; help,
  offline, confirmation, import/export, reset and too-small overlays; four themes
  (abyss, high-contrast, amber-crt, mono); sonar scope, depth gauge, carrier strip;
  presentation-only animation (odometer tween, pulse ring, row flash, carrier flash).
- **Accessibility** — 80×24 minimum layout with a clear too-small screen; reduce
  motion; ASCII glyph set; color is never the sole encoding of state (matched bands
  and alerts carry text labels); WCAG contrast tests (body ≥ 4.5:1, numerals ≥ 3:1);
  full keyboard path; mouse additive and optional.
- **Session safety** — fixed-step accumulator, >2 s catch-up through offline, 30 s
  autosave, save-once after offline, RAII terminal guard, panic hook that restores the
  terminal and writes a crash log, non-TTY refusal with guidance.
- **Gate and tooling** — `xtask` (`setup`, `gate`, `checks`, `privacy`, `gate-status`,
  `floor`, `mutate`, `dist`); test floor file that may only rise; privacy scan for
  paths, private patterns and e-mail domains; content-digest gate receipt with staleness
  detection; local distribution packaging.
- **CI and installers** — `.github/workflows/ci.yml` and `release.yml`;
  `scripts/install.sh` (POSIX, SHA-256, `~/.local/bin`) and `scripts/install.ps1`
  (`Get-FileHash`, `%LOCALAPPDATA%\hadal\bin`); `deny.toml` for `cargo-deny`.

### Fixed

- Rounding is explicit at the display boundary: amounts floor, costs ceil, and
  affordability compares exact `Decimal` values, so a shown cost never understates
  the requirement.
- A loaded or demo state seeds the Descent-available latch from its peak, so no
  spurious `DescentAvailable` event fires on the first tick after load.
- Auto actions are coalesced and emit no per-action events, so long offline runs
  cannot flood the bounded event buffer or the toast queue.
