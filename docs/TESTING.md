# HADAL — Testing Guide

Test tiers, exact commands, counts, goldens, robustness coverage, mutation and privacy
policy, the gate, and the number anchor rule.

## Tier table

| Tier | Coverage |
|---|---|
| **Unit** | Decimal arithmetic/serde/edge saturation; cost curves; production; unlock gates; buy one/ten/max; tick order; carrier; pulse; achievements/logs; prestige buckets; hull effects; reset scope; offline cap/report |
| **Determinism** | Fixed action scripts → pinned SHA-256 state hash; round-trip and load-continuation hashes |
| **Save/FS** | Round-trip, checksum tamper, future schema, unknown fields, backup fallback, corrupt/non-UTF8 preservation, import/export, migration injection, atomic write, lock error mapping |
| **Pacing** | Release-mode `active`/`idle`/`offline` sim profiles and the `pacing` integration test |
| **TUI** | 20 insta goldens at 80×24, 100×30, 140×40; overlays; boot; ASCII/mono; keymap; too-small; terminal restore; end-to-end session; resize storm; reduce motion |
| **CI** | fmt, clippy `-D warnings`, tests, pacing, MSRV, `cargo-deny`, release builds |

Tests are behaviour-first; no tautologies or source-text assertions. Pacing is a release-mode
integration test, not an ignored manual script.

CI is verified green on Ubuntu, Windows and macOS (run `45820719408`, commit `f0b2356`): fmt,
clippy, workspace tests on all three OSes, MSRV 1.89, release pacing, and `cargo-deny`. The
pacing job runs the release integration test; the deny job runs `cargo-deny`. Local
`cargo xtask gate` remains the single gate and its receipt is the local proof of a green tree.
Mutation testing (`cargo mutants`) is still not installed, pending owner approval; pre-push
prints `SKIPPED`.

## Exact commands

```text
cargo test -p hadal-core --lib                                   # Core unit tests
cargo test -p hadal                                              # TUI + headless-command tests
cargo test -p hadal-sim                                          # Simulator unit tests
cargo test -p hadal-core --release --test pacing -- --nocapture  # Pacing bands (release)
cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert
cargo run -p hadal-sim --release -- --profile idle --hours 1 --assert
cargo run -p hadal-sim --release -- --profile offline --hours 8,72 --assert
cargo test -p hadal --release --bins measurement -- --nocapture  # Performance measurements
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask gate-status                                          # Verify the gate receipt
cargo xtask privacy                                              # Privacy scan
cargo xtask gate                                                 # Full pre-release gate
```

`--data-dir target/qa-data` isolates manual saves; `--demo` runs a seeded late-game fixture
with no save and no lock.

## Counts

Measured (debug unless noted):

| Suite | Tests |
|---|---|
| `hadal-core --lib` | 189 |
| `hadal` (TUI + headless) | 160 |
| `hadal-sim` | 8 |
| `pacing` (release) | 1 |
| **Total** | **358** |

`tools/test-floor.txt` currently reads `247`. The floor may only **rise**; the gate refuses a
count below it, and the infra worker raises it to the measured **358**.

The `pacing` integration test takes **26.69 s in debug** but **1.73 s in release**; the gate
runs it in release. The `active` sim assertion runs in **~1.5 s** wall.

## Golden / snapshot policy

TUI layout goldens use **insta** (`=1.49.0`) with a `TestBackend` at three sizes (80×24,
100×30, 140×40). There are **27** goldens:

- `boot_100x30`, `too_small_79x24`
- `briefing_80x24_unicode`, `briefing_80x24_ascii`, `briefing_100x30`
- `demo_banner_100x30`
- `sonar_80x24_unicode`, `sonar_100x30_unicode`, `sonar_140x40`
- `sonar_80x24_ascii`, `sonar_100x30_ascii`, `sonar_80x24_mono`
- `descent_80x24`, `descent_100x30`, `descent_140x40`, `confirm_descent_80x24`
- `archive_80x24`, `archive_100x30`, `archive_locked_100x30`
- `system_80x24`, `system_100x30`, `system_save_failure_80x24`
- `help_80x24`, `help_glossary_80x24`
- `offline_summary_80x24`, `import_80x24`, `reset_80x24`

Workflow:

- Change a screen or layout, then regenerate with `INSTA_UPDATE=always cargo test -p hadal`.
- Inspect the resulting `.snap` diffs before committing; goldens verify layout structure and
  content placement, not source text.
- Snapshots render settled animation values, so goldens are stable.

Onboarding and overlay coverage: the briefing is exercised for a new save, demo and loaded
save (`app::tests::briefing_shows_for_new_save_and_demo_only`,
`briefing_any_key_dismisses_and_question_opens_help`); help scrolling
(`help_scroll_clamps_both_ends`); objective transitions
(`sonar_objective_line_three_states`, `model::tests::objective_line_priority_and_states`);
selection explainers (`selection_lines_name_cost_and_effect`); help sections and glossary
(`help::tests::help_covers_sections_objective_and_glossary`,
`help_objective_matches_sonar_helper`); and the overlay background
(`render::clear_overlay_band`, covered by `huge_overlay_string_is_clipped` and
`confirmation_overlay_renders_at_80x24`).

## Robustness coverage (existing)

| Case | Test |
|---|---|
| End-to-end session (pulse → buy → tune → descend → hull → save) | `app::tests::end_to_end_session` |
| Resize storm across every screen | `golden::resize_storm_renders_every_screen_safely` |
| Corrupt primary loads backup and preserves the corrupt file | `app::tests::corrupt_primary_loads_backup_and_recovers`; `save::tests::test_corrupt_primary_valid_backup_recovers` |
| Future schema refused without overwrite | `app::tests::future_schema_is_refused_without_overwrite`; `save::tests::test_future_schema_refused_and_file_unchanged` |
| Non-UTF8 save bytes preserved | `app::tests::non_utf8_save_bytes_are_preserved` |
| Second instance hits the lock and reports a friendly error | `app::tests::second_instance_reports_friendly_lock_error`; `save::tests::test_lock_acquire_twice_and_release_on_drop` |
| Atomic write + backup, no temp left behind | `save::tests::test_atomic_write_backup_and_no_temp` |
| Backup-copy failure aborts and keeps current | `save::tests::test_backup_copy_failure_aborts_and_keeps_current` |
| Control-character sanitisation of external strings | `sanitize::tests::*` |
| ASCII / mono modes | `sonar_80x24_ascii`, `sonar_100x30_ascii`, `sonar_80x24_mono` |
| Reduce motion keeps information, drops animation | `reduce_motion_disables_animations_not_state`, `reduce_motion_descent_is_instant`, `dive_duration_progress_and_reduce_motion` |
| Non-TTY refusal with guidance | `terminal::tests::non_tty_is_typed_error_with_guidance` |

No fuzzing harness, `nextest`, or JUnit output exists; do not claim them.

## Mutation testing

`cargo mutants --in-diff <diff-file>` is the per-change test-strength check.

- **Base:** `git merge-base HEAD origin/main`, falling back to `main`. The in-diff is the patch
  from that merge-base to `HEAD`.
- **First push:** with no common ancestor the in-diff run is skipped loudly and the weekly
  full `hadal-core` run covers it.
- **Timeouts:** per-mutant `max(20, 4 × baseline)` capped at 60 s, where baseline is a clean
  `cargo test -p hadal-core` run.
- **Exclusions:** `crates/xtask`, `crates/hadal-tui`, `crates/hadal-sim`, `**/build.rs`. Only
  `hadal-core` is mutation-tested.
- **Triage:** a surviving mutant is (1) marked equivalent code with `#[mutants::skip]` if it is,
  else (2) killed by a new behaviour test.
- **Install status:** `cargo-mutants` is not yet installed, pending owner approval. Pre-push
  prints `SKIPPED` loudly rather than pretending to run.

## Privacy policy

`cargo xtask privacy` scans for leaks:

- **Generic patterns** (defined in `xtask`) for absolute local paths and IP addresses.
- **Local-only private patterns** in `.git/hadal-private-patterns`, which is never committed.
- **E-mail domains** outside the allow-list `@users.noreply.github.com`,
  `noreply@anthropic.com`, `noreply@github.com` fail.
- **Commit identity:** every commit's author and committer e-mail must pass the same allow-list.

## Gate additions

- **Test floor.** `tools/test-floor.txt` may only rise; the gate compares the counted total
  against it. `#[ignore]` is refused unless annotated `// gate-allow: ignore`.
- **Gate receipt (v2).** `cargo xtask gate` writes `target/gate-receipt` anchored to a digest
  of the working-tree file **contents** (tracked, staged, untracked-but-not-ignored). `cargo
  xtask gate-status` recomputes the digest and exits non-zero on a stale receipt.
- **Doctests** are counted separately via `cargo test --doc -- --list`, because libtest
  `--list` omits them.

## Anchor rule

Every number quoted in a report or doc is recorded with the command and date that produced it.
Format: `value (command: ..., date: YYYY-MM-DD)`. This keeps stale numbers out of the docs.
The numbers in this file were measured on 2026-11-26.

## No unverified claims

Documentation must not claim verification that has not happened. CI now runs the fmt, clippy,
test (Ubuntu, Windows, macOS), MSRV 1.89, release pacing and `cargo-deny` jobs, so those
platforms are verified by CI. No claims of signing or notarization are made, and release
artifacts are not claimed until a tag is cut.
