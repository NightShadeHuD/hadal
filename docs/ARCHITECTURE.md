# HADAL — Architecture

Crate and module map, dependency direction, core API, tick order, state model, save format,
offline flow, TUI session, simulator, tooling, and performance budgets.

## Crates and dependency direction

```
hadal-tui (bin: hadal) ──► hadal-core ◄── hadal-sim (bin: hadal-sim)
                               ▲
                               └── (dev-dependency) hadal-sim, for the pacing test
xtask (bin: xtask) ── standalone dev tool
```

- **hadal-core** — `decimal`, `content/{ids,tables,story,tutorial}`, `engine`, `offline`,
  `save/{mod,paths,config,lock,migrate}`, `state`, `view`. No `ratatui`, `crossterm`,
  `clap`, wall-clock, RNG, environment, or filesystem in `tick()`.
- **hadal-tui** (package `hadal`, bin `hadal`) — `main`, `app`, `boot`, `cli`, `glyphs`,
  `input`, `model`, `render/{mod,sonar,descent,archive,system,help,coach,dive,hits}`, `sanitize`,
  `terminal`, `theme`, and `golden` (test only). Mutates state only through `Engine::apply`.
- **hadal-sim** (`lib` + `main`) — headless pacing profiles (`active`, `idle`, `offline`),
  serialisers, and assertions. Depends on core only.
- **xtask** — `setup`, `gate`, `checks`, `privacy`, `gate-status`, `floor`, `mutate`, `dist`.

The virtual workspace sets `default-members = ["crates/hadal-tui"]`, so bare `cargo run`
launches the TUI; the simulator is invoked with `-p hadal-sim`. The `hadal-core → hadal-sim`
edge is a **dev-dependency**, so the release dependency direction stays core-only while the
release-mode `pacing` integration test can drive the simulator.

## Core API

```rust
pub const TICKS_PER_SECOND: u64 = 10;

pub enum Action {
    Pulse,
    SetTuning { band: u8 },                 // 0..=5
    BuyGenerator { id: GeneratorId, quantity: BuyQuantity },
    BuyUpgrade(UpgradeId),
    BuyHull(HullId),
    SetAutoBuy { id: GeneratorId, enabled: bool },
    SetAutoDescendTarget { fathoms: u32 },  // 0 = off, else 3..=1_000_000
    Descend,
}
pub enum BuyQuantity { One, Ten, Max }

impl Engine {
    pub fn new() -> Self;
    pub fn demo() -> Self;                  // deterministic late-game fixture, not persisted
    pub fn tick(&mut self);
    pub fn advance(&mut self, ticks: u64);
    pub fn apply(&mut self, action: Action) -> Result<(), ActionError>;
    pub fn view(&self) -> StateView<'_>;
    pub fn drain_events(&mut self) -> impl Iterator<Item = Event> + '_;
    pub fn state_hash(&self) -> String;     // SHA-256 of canonical state JSON
    pub fn state(&self) -> &GameState;      // persistence boundary only
    pub fn from_state(state: GameState) -> Self; // load/demo constructor
    pub fn tutorial(&self) -> TutorialView<'_>;
    pub fn tutorial_ui(&mut self, event: TutorialUiEvent);
    pub fn tutorial_acknowledge(&mut self);
    pub fn tutorial_skip(&mut self);
    pub fn tutorial_replay(&mut self);
}

pub fn simulate_offline(engine: &mut Engine, elapsed_secs: u64) -> OfflineReport;
```

Tutorial steps advance by an observe-on-tick/apply predicate, at most 16 advances per call;
`tutorial_replay` is honoured only before the first Descent.

`StateView` accessor groups (read-only):

| Group | Accessors |
|---|---|
| Resources | `echo`, `rate`, `peak`, `fathoms`, `era_fathoms`, `lifetime_fathoms`, `depth_m`, `offline_cap_hours`, `global_multiplier` |
| Generators | `generator_unlocked`, `generator_owned`, `generator_counts`, `auto_buy`, `generator_cost`, `generator_affordable`, `generator_roi` |
| Upgrades | `upgrade_owned`, `upgrade_unlocked`, `upgrade_cost`, `upgrade_affordable`, `upgrade_count` |
| Hulls | `hull_owned`, `hull_count`, `hull_prereq_met`, `hull_cost`, `hull_effect_text`, `hull_affordable` |
| Descent | `can_descend`, `descent_gain`, `next_bucket_threshold`, `reset_scope`, `auto_descend_at` |
| Carrier/pulse | `carrier_band`, `tuned_band`, `carrier_matched`, `pulse_charge_ticks`, `pulse_capacity`, `pulse_charge_fraction`, `can_pulse` |
| Statistics | `lifetime_echo`, `best_peak`, `pulses_manual`, `pulses_auto`, `purchases`, `descents`, `total_ticks`, `bands_matched` |
| Achievements/logs | `achievement_unlocked`, `log_recorded`, `achievement_count` |

`auto_buy` is a read-only display of the per-generator toggle; the UI changes it only via
`Action::SetAutoBuy`.

### Persistence boundary

`Engine::state()` and `Engine::from_state()` exist **only** for save/load and the demo
fixture. `from_state` performs no validation — the caller must pass a state that already
passed checksum, schema and migration checks (or a trusted fixture such as `Engine::demo`).
It rebuilds an engine with an empty event buffer, an invalidated rate cache, and the
`DescentAvailable` latch seeded from the loaded peak so no spurious event fires on the first
tick. No gameplay may branch on state read through `state()` that `StateView` does not
already expose.

## Tick order (as implemented, 10 Hz)

1. **Carrier timer:** `carrier_ticks += 1`; at `CARRIER_CYCLE_TICKS` (900) reset to 0 and
   advance the band `(band + 1) % 6`; mark the rate dirty and emit `CarrierChanged`; if
   Tuned Ballast is owned, set the tuned band to the carrier and count the match.
2. **Capacitor:** `charge_ticks += 1`, capped at 250.
3. **Automation, once per second:** when `run_ticks % 10 == 0`, `auto_buy_one()` then
   `chain_automator()`.
4. **Rate:** compute the current production rate (single shared implementation, cached and
   invalidated on dirty).
5. **Production:** add `rate / 10` to Echo and to lifetime Echo.
6. **Auto-pulse:** if Auto-Pulse is owned and the capacitor is full, add the pulse yield,
   increment auto-pulse count, reset the capacitor.
7. **Peak:** update `peak_echo` and `best_peak`.
8. **Achievements/logs:** refresh unlocks and recorded logs, then latch the `DescentAvailable`
   edge.
9. **Ballast Governor:** if owned, target > 0, and current gain ≥ target, descend.
10. **Counters:** `run_ticks += 1`, `total_ticks += 1`.

Manual actions run between ticks and use the same validators. Achievements unlocked in step 8
raise the multiplier starting on the next tick.

## Determinism contract

Only IEEE-754 `+ - * / sqrt` and integer operations are used in numeric paths. `f64` appears
in content constants and in the derived multipliers (era `1 + 0.05 × era_fathoms`,
achievement `1 + 0.01 × count`, carrier `1.5`, hull/tier multipliers), each converted to
`Decimal` at the point of use and bit-identical across platforms; `f64` never enters state.
`descent_gain` multiplies an integer base by the exactly-representable `1.5`/`2.0` and floors.
`state_hash()` serialises the canonical JSON of `GameState` and returns a
SHA-256 hex digest. Pinned-hash tests: `engine::tests::test_scripted_determinism_and_hash`,
`test_full_automation_hash`, `state_roundtrip_through_save_load_preserves_hash`, and
`from_state_first_tick_matches_continuation` (a loaded engine continues identically to one
that never left memory).

## State model

Three structs, every field `pub(crate)`:

- **Stats** — `lifetime_echo`, `best_peak`, `pulses_manual`, `pulses_auto`, `purchases`,
  `descents`, `total_ticks`, `bands_matched`.
- **MetaState** — `fathoms`, `era_fathoms`, `lifetime_fathoms`, `depth_m`, `auto_descend_at`,
  `hulls` (`BTreeSet<HullId>`), `auto_buy` (`[bool; 4]`), `achievements`, `logs`,
  `bands_seen` (`[bool; 6]`), `tutorial` (`TutorialState`: step, completed, skipped, and
  per-step flags), `stats`.
- **RunState** — `echo`, `peak_echo`, `generators` (`[u32; 4]`), `upgrades`
  (`BTreeSet<UpgradeId>`), `charge_ticks`, `carrier_band`, `tuned_band`, `carrier_ticks`,
  `run_ticks`.
- **GameState** — `{ meta: MetaState, run: RunState }`, owned by `Engine`.

All serialise with `deny_unknown_fields`; economy values are `Decimal`; no `f64` fields.

## Save format and load flow

Schema **v2** (`SUPPORTED_SCHEMA = 2`). Envelope (canonical field order):

```json
{
  "format": "hadal-save",
  "schema_version": 2,
  "app_version": "0.2.1",
  "saved_at_unix": <unix>,
  "save_counter": <u64>,
  "checksum": "sha256:<hex>",
  "state": { "meta": { ... }, "run": { ... } }
}
```

- **Checksum canonicalisation:** SHA-256 over serde_json's compact serialisation of the
  envelope value with the `checksum` field removed (object keys sorted). Identical across
  platforms.
- **Load flow:** parse JSON → check `format` marker → schema gate (a newer schema returns a
  readable `FutureSchema` error and is never overwritten) → current schema: deserialize with
  unknown-field rejection, then verify checksum; older schema: verify checksum **first**,
  migrate via a `serde_json::Value` chain, then deserialize.
- **Atomic write:** create temp file, write, `sync_all`; copy current → `save.bak.json` and
  sync (abort the save if this fails); rename temp over current; best-effort directory sync
  on Unix.
- **Backup/corruption:** a missing primary loads the backup; a corrupt primary is copied to
  `save.corrupt-<unix>.json`, then the backup is loaded; if both fail, a `NoValidSave` error.
- **Counter:** `save_counter` is monotonic. A disk counter newer than the in-memory one is
  refused (`NewerSaveOnDisk`), so an older save never clobbers a newer one.
- **Lock:** the data directory is created first, then `File::try_lock` on `save.lock`;
  `TryLockError::WouldBlock` maps to `AlreadyRunning`; released when the handle drops.
- **Migration:** `MigrationChain` transforms the envelope `Value` before deserialization.
  Production registers a real `1 -> 2` step that inserts a completed `TutorialState` into
  existing saves; tests cover the v1 fixture and a synthetic 0→1 step.
- **Config:** `config.json` is separate, unchecksummed, `#[serde(default)]`,
  forward-compatible, and never deleted on parse failure.
- **Import/export:** import rejects files over 4 MiB and validates format, schema and checksum
  before backing up and writing; it sets `saved_at_unix = now` without granting offline
  credit. Export writes the same envelope to a chosen path.

Default paths: Windows `%APPDATA%\hadal`, macOS `~/Library/Application Support/hadal`, Linux
`$XDG_DATA_HOME/hadal` or `~/.local/share/hadal`. Resolution order: explicit `--data-dir`,
then `HADAL_DATA_DIR`, then the platform default. Files: `save.json`, `save.bak.json`,
`save.lock`, `config.json`, `exports/`, `crash-<unix>.log`, `save.corrupt-<unix>.json`,
`save.json.tmp`.

## Offline flow

1. On load, `elapsed = max(0, now_unix - saved_at_unix)`.
2. Cap from the highest owned endurance hull (8 h base → 24 → 72 → 168 h).
3. `credited = min(elapsed, cap)`; `ticks = credited × 10`.
4. `engine.advance(ticks)` — the exact online tick, no manual actions.
5. Build an `OfflineReport` (requested/credited/capped seconds, cap hit, ticks, Echo delta,
   purchases, descents, Fathoms gained, achievements) for absences ≥ 60 s.
6. **Save immediately** before accepting input. A failed save is a persistent warning with
   retry, never silent credit loss.
7. The same catch-up path handles a >2 s event-loop gap (process suspension) with a toast.

## TUI session

- **CLI** (`cli.rs`): global flags `--data-dir`, `--demo`, `--ascii`, `--theme`, `--color`,
  `--no-mouse`, `--version`, plus the headless subcommands `save path`, `export`, `import`,
  `verify`.
- **Data directory** (`app::resolve_options`): explicit `--data-dir` → `HADAL_DATA_DIR` →
  platform default; config is loaded and CLI overrides applied.
- **Lock and load:** acquire the `save.lock` (skip for `--demo`), load primary/backup,
  handle future-schema and corruption, apply offline, then save once. An absent save file is a
  typed `NoSave` (fresh install) and an empty directory starts a fresh save and the tutorial;
  `--demo` creates nothing.
- **Tutorial coach:** `render/coach.rs` draws a reserved coach band (never covering its
  target) plus the highlighted control; UI-only steps (SYSTEM, manual) are reported through
  `Engine::tutorial_ui`; the session saves when the step changes.
- **Fixed-step loop:** `STEP = 100 ms`, `MAX_STEPS = 20` per frame, `POLL = 33 ms`,
  `FRAME = 33 ms` (≈30 FPS), `AUTOSAVE = 30 s`, `GAP = 2 s`. A gap above `GAP` routes through
  offline catch-up; otherwise fixed 100 ms ticks are accumulated. Autosave fires every 30 s
  and after a Descent or import.
- **Terminal safety** (`terminal.rs`): a TTY check refuses non-interactive startup with
  guidance; an RAII `TerminalGuard` restores raw mode, the alternate screen and mouse exactly
  once (explicit restore or drop); a panic hook restores the terminal, writes
  `crash-<unix>.log`, and prints a recovery message.
- **Config** (`save/config.rs`, mirrored in `theme.rs`): theme, reduce motion, ASCII, mouse,
  colour depth, bell — persisted separately from the save.
- **TUI-only modules:** `boot` (title/boot lines), `glyphs` (Unicode/ASCII sets), `input`
  (pure keymap + bounded queue), `model` (derived names, requirements, contributions,
  governor validation, bucket progress, the objective line and selection explainers),
  `render/*` (screens including the `coach` tutorial band and highlights; overlays including
  the scrollable `help`/glossary and `clear_overlay_band`; shared vocabulary, too-small
  screen, click hit-testing), `sanitize` (C0/C1 control-character scrubbing), `theme`
  (palettes, colour depth, WCAG contrast).

## Simulator

`hadal-sim` exposes strategies through its lib (`SimProfile`, `assert_bands`,
`check_clock_skew`, `check_online_offline_equivalence`, JSON/CSV serialisers) and a `main`
CLI (`--profile active|idle|offline`, `--hours`, `--assert`, `--json`, `--csv`, `--out`). The
release-mode `crates/hadal-core/tests/pacing.rs` integration test depends on the simulator via
the dev-dependency edge and asserts the same bands.

## Tooling (xtask)

- **`gate`** — sequentially runs fmt check, clippy `-D warnings`, tests, the test-floor check,
  and (when applicable) release pacing; writes a **receipt v2** into `target/gate-receipt`.
- **`gate-status`** — recomputes a digest of the working-tree file **contents** (tracked,
  staged, untracked-but-not-ignored, deduped) and compares it with the receipt; exits non-zero
  on a stale receipt.
- **`checks`** — source scans (forbidden patterns, test counting, floor checks).
- **`privacy`** — scans for local absolute paths, a local-only private-pattern list kept in
  `.git/hooks`-adjacent config (never committed), and e-mail domains outside an allow-list
  (`@users.noreply.github.com`, `noreply@anthropic.com`, `noreply@github.com`); also checks
  commit author/committer identities.
- **`floor`** — reads or sets `tools/test-floor.txt`, which may only rise.
- **`mutate`** — `cargo mutants --in-diff` wrapper (install pending owner approval; prints
  `SKIPPED` loudly until then).
- **`dist`** — builds the release binary for the host triple and packages an archive.

## Performance budgets

Measured on Windows (release, thin LTO, `strip = "symbols"`):

| Metric | Budget | Measured / notes |
|---|---|---|
| 168 h offline (6.05M ticks) | < 2 s (goal < 1 s) | ~0.67 s wall incl. startup |
| 8 h offline catch-up (288k ticks) | < 100 ms | ~23 ms |
| Tick average (release) | < 10 µs | ~0.08 µs |
| SONAR render | < 33 ms/frame | ~150 µs/frame |
| Demo startup to first frame | < 150 ms | ~1.0 ms |
| Save | < 25 ms typical | atomic temp+fsync+rename |
| Stripped binary | < 20 MB | 1.71 MB |
| Resident memory | < 50 MB | no unbounded growth |

Reproduce with `cargo test -p hadal --release --bins measurement -- --nocapture`,
`cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert`, and
`cargo run -p hadal-sim --release -- --profile offline --hours 168 --assert`.
