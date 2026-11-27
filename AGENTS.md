# HADAL — Engineering Laws

Thirteen laws govern every change. Read this file before editing anything.

## 1. Architecture boundary

`hadal-core` is headless: no `ratatui`, `crossterm`, `clap`, wall-clock, RNG, environment,
or filesystem in `tick()`. `hadal-sim` depends on core only; `hadal-tui` holds no economy
state.

All `GameState` fields are `pub(crate)`. The UI reads through `StateView` accessors and
mutates only by sending `Action`s through `Engine::apply()`. `Engine::state()` and
`Engine::from_state()` are the **persistence boundary only** — used by save/load and the
demo fixture, never by gameplay or rendering. No gameplay may branch on state read through
`Engine::state()` that `StateView` does not already expose.

## 2. Numeric law

`Decimal` is the sole economy type. No `f64` fields exist in `GameState`, `MetaState`, or
`RunState`; `f64` never enters state.

`f64` appears only in `content/tables.rs` constants and in the derived-multiplier arithmetic
that is converted to `Decimal` at the point of use: the era passive `1 + 0.05 × era_fathoms`,
the achievement passive `1 + 0.01 × count`, the carrier `1.5`, and the hull/tier multipliers.
All of that is plain IEEE-754 arithmetic, so it is bit-identical on every platform.

No `powf`, `ln`, `exp`, `log10`, `sin`, or locale formatting in simulation. Determinism
contract: only IEEE-754 `+ - * / sqrt` and integer operations — the same scripted ops produce
the same state hash everywhere. `descent_gain` additionally multiplies an **integer** base by
the exactly-representable factors `1.5`/`1.0` and `2.0`/`1.0` and floors the product, so its
result is exact and deterministic.

## 3. Tick law

Fixed **10 Hz** (100 ms). The UI never advances partial ticks. Exact ten-step order in
`Engine::tick()`:

1. Carrier timer — every 900 ticks advance band `0→1→…→5→0`; if Tuned Ballast is owned,
   set tuned band to carrier. Emit `CarrierChanged`.
2. Increment the pulse capacitor by one tick (cap 250).
3. Every `run_ticks % 10 == 0`: (a) auto-buy one best-ROI generator among enabled tiers,
   then (b) Chain Automator buys the cheapest affordable unowned upgrade.
4. Compute the production rate from counts, upgrades, hulls, era Fathoms, achievements,
   and carrier match.
5. Add one tick of production; add it to lifetime Echo.
6. If Auto-Pulse is owned and the capacitor is full, fire a pulse, add its yield, reset
   the capacitor.
7. Update peak Echo and best peak.
8. Refresh achievements and logs; latch the `DescentAvailable` edge.
9. If Ballast Governor is enabled, target > 0, and current gain ≥ target, descend.
10. Increment `run_ticks` and `total_ticks`.

Achievements unlocked in step 8 therefore raise the passive multiplier starting on the
**next** tick (production in step 4 already ran). Manual actions run between ticks and use
the same validators. Carrier match is ×1.5 while `tuned == carrier`. A band is recorded in
`bands_matched` only when a tuning action (manual or auto) produces a match. Pulse
capacitor starts full on a new run; yield is `max(10, rate × 10)` × hull pulse multiplier.
No wall clock, RNG, filesystem, environment, or UI input inside `tick()`.

## 4. Save law

Schema **v2**. Envelope fields, in canonical file order: `format`, `schema_version`,
`app_version`, `saved_at_unix`, `save_counter`, `checksum`, `state`.

- **Checksum** — SHA-256 over the canonical compact JSON of the envelope minus `checksum`
  (object keys sorted). Verified before use.
- **Load order** — parse JSON → format marker → schema gate (a newer schema is refused with
  a recovery message and never overwritten) → for the current schema, deserialize with
  unknown-field rejection then verify the checksum; for an older schema, verify the
  checksum **before** migrating, then migrate via a `serde_json::Value` transform chain,
  then deserialize.
- **Atomic write** — write + `sync_all` a temp file, copy the current file to
  `save.bak.json` and sync it (abort the save if the backup fails), rename the temp over
  the current file, then best-effort directory sync on Unix.
- **Corruption** — a corrupt primary falls back to the backup and is preserved as
  `save.corrupt-<unix>.json`; it is never silently overwritten.
- **Counter** — `save_counter` is monotonic. A disk counter newer than the in-memory one
  is refused (`NewerSaveOnDisk`); an older save never clobbers a newer one.
- **Lock** — one advisory lock on `save.lock` via `File::try_lock`; a second instance
  exits cleanly (`WouldBlock` → `AlreadyRunning`). Released on drop.
- **Migrations** — production ships a real `1 -> 2` step that inserts a completed tutorial
  into existing saves; a synthetic 0→1 step remains the test injection point. Every schema
  change bumps the version, adds a migration step and a fixture.
- **Config** — `config.json` (theme, reduce motion, ASCII, mouse, color depth, bell) is
  separate, unchecksummed, forward-compatible, and never deleted on parse failure.

## 5. Offline law

Offline runs the **exact online tick** headlessly with automation only and a 100% rate; no
manual actions are credited. `elapsed = max(0, now_unix - saved_at_unix)`;
`credited = min(elapsed, cap)`; `ticks = credited × 10`.

- Caps: **8 h** base; **24 h** Long Storage; **72 h** Extended Ballast; **168 h** Abyssal
  Reservoir (the highest owned endurance hull wins).
- Negative clock jumps credit nothing; long absences are capped.
- A welcome-back summary appears for absences ≥ 60 s.
- The game **saves once immediately** after applying offline progress, before accepting
  input; a failed save is a persistent warning with retry, never silent loss.
- Equivalence: the same state and automation over the same duration must yield an
  identical state hash online and offline (pinned in the pacing test).

## 6. Tutorial law

- **Single table.** Copy, order, targets and completion kinds live in one table,
  `crates/hadal-core/src/content/tutorial.rs`.
- **Persisted, production-neutral.** Progress is `MetaState::tutorial` (`TutorialState`) saved
  with the game; it never changes production, costs or prestige, and advances deterministically
  from player actions.
- **Engine-driven.** Steps advance by an observe-on-tick/apply predicate (capped at 16 per
  call); UI-only steps go through `Engine::tutorial_ui`, completion is
  `Engine::tutorial_acknowledge`, and `tutorial_skip`/`tutorial_replay` are the only other
  transitions. Replay is allowed only before the first Descent.
- **Tested per step.** Every step has a predicate test; skip/replay and persistence have
  tests; the v1→v2 fixture proves existing saves are marked complete.

## 7. Testing law

| Tier | Coverage |
|---|---|
| Unit | Decimal arithmetic/serde/edges; cost curves; production; unlocks; buy one/ten/max; tick order; carrier; pulse; achievements/logs; descent buckets; hull effects; reset scope; offline cap/report |
| Determinism | Fixed action script → pinned SHA-256 state hash |
| Save/FS | Round-trip, checksum tamper, future schema, unknown fields, backup fallback, corrupt preservation, import/export, migration injection, atomic write, lock mapping |
| Pacing | Release-mode `active`/`idle`/`offline` sim profiles and the `pacing` integration test |
| TUI | insta goldens at 80×24, 100×30, 140×40; overlays; boot; mono/ASCII; keymap; too-small; terminal restore; E2E session; resize storm |
| CI | fmt, clippy `-D warnings`, tests, pacing, MSRV, `cargo-deny`, release builds |

Measured counts: core **210**, TUI **230**, sim **8**, and the release pacing test **1** — total
**449**. `tools/test-floor.txt` currently reads `449`; the floor may only rise, and the infra
worker raises it to the measured **449** (the gate refuses a count below the floor). Every new
mechanic gets a unit test; every save change gets a round-trip and a migration fixture;
every UI change updates its goldens. Pacing is a release-mode integration test, not an
`#[ignore]`d manual script. No tautologies or source-text assertions.

`cargo mutants --in-diff <diff-file>` is the per-change test-strength check (install pending
owner approval; the check prints `SKIPPED` loudly until then).

## 8. Content law

One table source of truth: `crates/hadal-core/src/content/tables.rs`. 4 generators, 8
Signal Chain upgrades, 17 hulls, 11 achievements, 10 logs. The **final** hull costs end at
**Fractal Hull = 50,000** Fathoms (raised from the early 1,000 after pacing measurement showed
the top of the tree was reached too early). Content IDs are stable strings; new variants
append and never rename (a rename needs a migration). Every unlock has a test; there are no
dead upgrades.

## 9. UX law

- Minimum layout **80×24**; below that render only a clear too-small screen — no partial
  layout, no panic.
- The terminal is restored on **every** exit path, normal or panic (RAII guard + panic hook),
  and startup refuses a non-TTY with guidance.
- Colour is **never** the sole encoding of state; matched bands, alerts and locks always
  carry text labels.
- Reduce motion, ASCII-only glyphs (`--ascii`, the mono theme, and `--theme mono`) are
  available and change presentation only, never information.
- Contrast: body text ≥ 4.5:1, large numerals ≥ 3:1, both enforced by tests.

## 10. Pre-commit and the gate

Before every commit: read the full diff against this file; confirm every new public
behaviour has a test; confirm no `f64` state, no wall clock/RNG/UI in core, no TODOs/stubs/
debug output. Run `cargo xtask gate` (fmt, clippy, tests, floor, privacy where applicable,
pacing, content-digest receipt). The gate writes a **receipt v2** anchored to a digest of
the working-tree file contents; `cargo xtask gate-status` verifies the receipt still matches
the tree and exits non-zero on a stale receipt. No WIP commits, no generated artifacts.

## 11. Git

Conventional atomic commits, one behaviour per commit. `main` is always green. Never
force-push. Release tags (`v*`) are created only after `cargo xtask gate` passes. Add a
remote only when a URL is supplied; never fabricate one.

## 12. Performance budgets

Measured on Windows (release, thin LTO, stripped symbols):

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

## 13. Definition of done

- `cargo xtask gate` passes (fmt, clippy, tests, floor, pacing, receipt).
- Pacing bands in `docs/BALANCE.md` are met for `active`, `idle` and `offline`.
- Manual playthrough: pulse → buy → tune → first Descent → hull → save → quit → relaunch →
  offline summary.
- Goldens approved (37); no unverified claims in docs (CI verifies Ubuntu, Windows and macOS;
  release artifacts are not claimed until a tag is cut).
- No `f64` state, no transcendental functions in simulation, no wall clock/RNG in core.

## File ownership — where to change what

| Area | Primary file(s) |
|---|---|
| Generator/upgrade/hull/achievement/log constants, descent gain | `crates/hadal-core/src/content/tables.rs` |
| Content IDs | `crates/hadal-core/src/content/ids.rs` |
| Narrative text, boot lines | `crates/hadal-core/src/content/story.rs` |
| Tick order, actions, purchase math, reset, automation | `crates/hadal-core/src/engine.rs` |
| State structs | `crates/hadal-core/src/state.rs` |
| Read-only accessors | `crates/hadal-core/src/view.rs` |
| Offline catch-up and report | `crates/hadal-core/src/offline.rs` |
| Decimal type, formatting, serde | `crates/hadal-core/src/decimal.rs` |
| Save envelope, checksum, atomic write, load, import/export | `crates/hadal-core/src/save/mod.rs` |
| Save paths, platform data dir | `crates/hadal-core/src/save/paths.rs` |
| Config | `crates/hadal-core/src/save/config.rs` |
| Advisory lock | `crates/hadal-core/src/save/lock.rs` |
| Migrations | `crates/hadal-core/src/save/migrate.rs` |
| Session lifecycle, accumulator, offline, terminal restore | `crates/hadal-tui/src/app.rs` |
| CLI parsing | `crates/hadal-tui/src/cli.rs` |
| Key mapping, command queue | `crates/hadal-tui/src/input.rs` |
| Derived presentation (names, requirements, progress, governor) | `crates/hadal-tui/src/model.rs` |
| Screens and overlays | `crates/hadal-tui/src/render/{sonar,descent,archive,system,help,dive}.rs` |
| Click hit-testing | `crates/hadal-tui/src/render/hits.rs` |
| Shared visual vocabulary, too-small screen | `crates/hadal-tui/src/render/mod.rs` |
| Themes, colour depth, contrast | `crates/hadal-tui/src/theme.rs` |
| Glyphs | `crates/hadal-tui/src/glyphs.rs` |
| Control-char sanitisation | `crates/hadal-tui/src/sanitize.rs` |
| Terminal guard, panic hook | `crates/hadal-tui/src/terminal.rs` |
| Pacing strategies, sim CLI | `crates/hadal-sim/src/{lib,main}.rs` |
| Dev gate, checks, privacy, receipt, dist, mutate | `crates/xtask/src/main.rs` |
| CI / release | `.github/workflows/` |
| Installers | `scripts/` |

## 14. Studio workflow

The project runs as a studio of role passes, not a free-for-all of concurrent editors.

- **One active agent at a time.** Exactly one agent edits the tree at any moment. Parallel
  read-only scouting is allowed; concurrent writes are not.
- **Sequential role passes with handoff artifacts.** Work proceeds
  `Art/UX -> QA -> Systems -> QA -> CTO`. Each pass leaves a named handoff artifact (diff,
  report, or snapshot set) that the next pass consumes; nothing is "carried in someone's head".
- **Cross-model review.** Every artifact is reviewed by a **different model** than the one that
  authored it. A model never signs off on its own work.
- **Reviewer must be a different base model family.** A different reasoning mode/effort label
  on the same weights is not a cross-model review. Record the reviewer's base model id, not the
  harness label.
- In this environment `studio:high` and `studio:off` are both the `[studio]` preset
  (Qwen3.6-35B, 131072 ctx) and must not review each other. The designated cross-family
  reviewer is `reviewer-20b` (gpt-oss-20b, 65536 ctx). QA passes that used `studio:off` are
  same-base mechanical checks, not independent cross-model reviews.
- **Model allocation.** Execution roles (Art/UX, Systems, content, QA, release engineering) run
  local models. Deepseek is reserved for executive roles (Vision Guardian, Studio Operations,
  CTO sign-off). The local reviewer of record for this setup is `reviewer-20b`; `studio:high`/
  `studio:off` are the same base and are not reviewers.
- **QA grades every handoff.** Each handoff is graded 1-100 against `docs/QA-RUBRIC.md` and must
  pass the gate set: build, `fmt`, `clippy -D warnings`, tests, determinism, no placeholders, and
  snapshot/performance proof where relevant. The score and per-gate pass/fail are recorded.
- **Cross-model diff audit before any commit.** A reviewer with a different model audits the full
  staged diff against this file before it is committed.
- **No WIP commits.** `main` stays green; incomplete or unformatted work is never committed.
