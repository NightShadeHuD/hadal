# HADAL — Invariants

Checkable statements derived from the engineering laws. Each has a **Why** and a **Check**.
Every check names a real test or command that exists in this tree. Test paths are relative to
the crate that owns them; `save::tests::…` means `crates/hadal-core/src/save/mod.rs`, and so
on.

## A. Determinism

**A1** `tick()` advances exactly one 100 ms step; `advance(n)` equals `n` ticks.
- *Why:* partial or spurious steps break hashes and offline equivalence.
- *Check:* `engine::tests::test_advance_equals_ticks`, `test_carrier_cycle_timing`.

**A2** No wall clock, RNG, filesystem, environment or UI input is read inside `tick()`.
- *Why:* each is nondeterministic.
- *Check:* `rg -n 'std::time|SystemTime|Instant|rand::|std::fs|std::env' crates/hadal-core/src/engine.rs` returns no matches.

**A3** The tick order is the fixed ten steps; an achievement unlocked in step 8 raises the
multiplier on the **next** tick, and the capacitor/auto-pulse/automation cadences match.
- *Why:* order defines production and prestige outcomes.
- *Check:* `engine::tests::test_achievement_bonus_applies_next_tick`,
  `test_capacitor_refill_and_manual_threshold`, `test_auto_pulse_fires_once_per_charge`,
  `test_tick_purchase_before_production`, `test_auto_buy_no_funds_and_cadence`.

## B. Core boundary

**B1** `hadal-core` has no `ratatui`, `crossterm`, `clap`, wall-clock or RNG dependency.
- *Why:* core must stay headless and testable in isolation.
- *Check:* `cargo tree -p hadal-core` shows none of those crates.

**B2** All content lives in one table source.
- *Why:* a single source prevents drift and makes tuning auditable.
- *Check:* `tables::tests::test_generator_table_exact`, `test_upgrade_table_exact`,
  `test_hull_table_exact`, `test_achievement_and_log_tables_complete`.

**B3** State fields are private; `Engine::state`/`from_state` are the persistence boundary
only, and the UI reads `StateView` and mutates via `Action`.
- *Why:* the UI must not hold or fork economy state.
- *Check:* `crates/hadal-core/src/state.rs` declares every field `pub(crate)`;
  `engine::tests::from_state_first_tick_matches_continuation` proves a loaded engine continues
  identically.

## C. Decimal-only economy

**C1** Every economy field is `Decimal`; no `f64` field exists in `GameState`, `MetaState` or
`RunState`.
- *Why:* `f64` state would round differently across platforms.
- *Check:* `state::tests::test_state_serde_roundtrip`; `crates/hadal-core/src/state.rs` field
  types.

**C2** No `Decimal` can be `NaN`, `Infinity` or negative.
- *Why:* invalid values corrupt production, costs and prestige.
- *Check:* `decimal::tests::test_from_parts_nonfinite`, `test_sub_saturating_at_zero`,
  `test_normalize_invariant`.

**C3** Buying `n` costs exactly `n` single buys.
- *Why:* buy-ten/max must equal repeated single buys.
- *Check:* `engine::tests::test_buy_ten_equals_sum_of_singles`,
  `test_max_buy_invariant_and_apply`, `test_purchases_increment_stat`.

**C4** Costs round up, amounts round down, and affordability compares exact values.
- *Why:* a shown cost must never understate the requirement.
- *Check:* `decimal::tests::test_floor_ceil_table`, `test_floor_never_exceeds_ceil`;
  `engine::tests::test_generator_unit_cost_values`, `test_view_unlock_and_affordability`.

## D. Save integrity

**D1** A save round-trip produces an identical state hash.
- *Why:* load must restore the precise state.
- *Check:* `save::tests::test_save_load_roundtrip_same_hash`,
  `engine::tests::state_roundtrip_through_save_load_preserves_hash`.

**D2** A bad import (bad checksum, future schema, tampered envelope) changes nothing.
- *Why:* failed imports must never destroy the current save.
- *Check:* `save::tests::test_import_bad_checksum_leaves_directory_identical`,
  `test_future_schema_refused_and_file_unchanged`, `test_checksum_tamper_detected_and_preserved`.

**D3** Every older schema loads via migration and fixture; the shipped `1 -> 2` migration
marks existing saves' tutorial complete.
- *Why:* a schema bump must not strand old saves.
- *Check:* `save::tests::test_v1_migration_inserts_completed_tutorial`,
  `test_migration_chain_0_to_1_fixture`, `test_migration_chain_ordering`,
  `test_v0_without_migration_is_typed_error`.

**D4** One writer at a time; an older save never clobbers a newer one; a corrupt primary
falls back to the backup and is preserved.
- *Why:* concurrent or regressed writes lose progress.
- *Check:* `save::tests::test_save_counter_clobber_guard`,
  `test_lock_acquire_twice_and_release_on_drop`, `test_corrupt_primary_valid_backup_recovers`.

## E. Offline correctness

**E1** Offline adds Echo and never subtracts it; 8 h online and 8 h offline from the same
state produce the same hash.
- *Why:* offline must be indistinguishable from an open tab.
- *Check:* `offline::tests::test_offline_equals_online_8h`,
  `test_offline_168h_report_aggregates`.

**E2** The credited time never exceeds the hull-derived cap.
- *Why:* the cap is the contract; over-crediting breaks balance.
- *Check:* `offline::tests::test_offline_caps_by_endurance_hull`,
  `test_offline_30_day_capped_to_base`, `test_offline_zero_and_clock_skew`.

**E3** The game makes no network calls or telemetry.
- *Why:* it is a local, offline-capable game.
- *Check:* `cargo tree -p hadal-core` has no network crate;
  `rg -n 'reqwest|ureq|hyper|TcpStream|std::net' crates/` returns no matches.

## F. Descent safety

**F1** Descent never removes a permanent thing (Fathoms, hulls, achievements, logs, lifetime
stats).
- *Why:* prestige must only ever add capability.
- *Check:* `engine::tests::test_descent_reset_scope`, `test_descent_gain_and_stats`,
  `test_descent_gain_with_tidal_and_fractal`.

**F2** The exact effect (gain, reset list, threshold) is shown before the player pays.
- *Why:* an irreversible reset must be fully previewed.
- *Check:* `engine::tests::test_view_descent_thresholds`; the DESCENT confirmation rendered in
  `app::tests::end_to_end_session` (asserts the `CONFIRM DESCENT` text) and the `descent_*`
  goldens.

## G. Unlock discipline

**G1** A generator, upgrade, tab or feature appears only when its unlock condition is met.
- *Why:* unfolding is earned, not dumped.
- *Check:* `engine::tests::test_locked_generators`, `test_view_unlock_and_affordability`,
  `tables::tests::test_unlock_met`, `test_unlocks_reference_valid_thresholds`.

## H. Pacing and performance

**H1** All pacing bands in `docs/BALANCE.md` are met by the `active`, `idle` and `offline`
profiles.
- *Check:* `cargo test -p hadal-core --release --test pacing`;
  `cargo run -p hadal-sim --release -- --profile {active,idle,offline} --hours … --assert`.

**H2** The test floor may only rise.
- *Check:* `tools/test-floor.txt`; the gate's floor check via `cargo xtask gate` /
  `cargo xtask gate-status`.

**H3** The stripped release binary is under 20 MB.
- *Check:* `cargo build --release -p hadal` then `ls -l target/release/hadal` (measured
  1.71 MB).

**H4** 168 h offline (6.05 M ticks) runs in under 2 s in release.
- *Check:* `engine::tests::test_release_168h_under_two_seconds`.

## I. Content IDs

**I1** Content IDs are stable strings; appending never renames an existing one.
- *Why:* a rename would corrupt old saves.
- *Check:* `ids::tests::test_generator_ids`, `test_upgrade_ids`, `test_hull_ids`,
  `test_achievement_ids`, `test_log_ids`, `test_unknown_id_rejected`.

**I2** Tutorial content is defined by one step table, its state is persisted in
`meta.tutorial`, and it never changes production or costs; every step has a predicate test.
- *Why:* onboarding must be data, not code branches, and must not perturb the economy.
- *Check:* `content::tutorial::tests::test_steps_table`,
  `engine::tests::test_tutorial_predicates`, `test_tutorial_full_progression`,
  `save::tests::test_tutorial_step_roundtrips`.

## J. Gate discipline

**J1** The gate is green before every commit and the receipt is anchored to the tree.
- *Why:* a stale receipt hides an unverified tree.
- *Check:* `cargo xtask gate` exits 0 only when all checks pass;
  `cargo xtask gate-status` recomputes the content digest and exits non-zero on a stale
  receipt.
