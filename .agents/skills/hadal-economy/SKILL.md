# HADAL Economy Skill

**Purpose:** Change economy values (generators, upgrades, hulls, achievements, logs,
constants) without breaking determinism, pacing, or save compatibility.

**When to use:** Tuning generator costs, adjusting upgrade effects, adding or changing hulls,
modifying achievement/log thresholds, running balance simulations.

## Workflow

1. **Read `docs/DESIGN.md`** for the approved content tables and formulas.
2. **Read `docs/BALANCE.md`** for the current pacing bands and measured sim results.
3. **Edit only `crates/hadal-core/src/content/tables.rs`.** Constants and `f64` literals live
   here and nowhere else; they convert to `Decimal` at point of use. ID variants live in
   `crates/hadal-core/src/content/ids.rs` and new ones may only be appended.
4. **Keep determinism:** no `f64` fields in state; only `+ - * / sqrt` and integer ops in
   simulation; the sole `f64` multiply is `descent_gain`'s `base as f64 × 1.5/2.0`, which is
   exact in binary.
5. **Run the balance simulations:**
   ```
   cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert
   cargo run -p hadal-sim --release -- --profile idle --hours 1 --assert
   cargo run -p hadal-sim --release -- --profile offline --hours 8,72 --assert
   ```
6. **Check bands** against `docs/BALANCE.md`. If a band fails, adjust constants and re-sim.
   Do not change formulas, reset scope, or layer semantics.
7. **Run the core and pacing tests:**
   ```
   cargo test -p hadal-core --lib
   cargo test -p hadal-core --release --test pacing
   ```
8. **Update `docs/BALANCE.md`** with the new measured results (paste the PASS table and the
   anchor date).
9. **Run `cargo xtask gate`** before committing.

## Rules

- Only `content/tables.rs` moves during tuning; formulas are frozen.
- IDs are stable strings; append, never rename (a rename needs a migration).
- The final hull costs end at **Fractal Hull = 50,000** Fathoms.
- No dead upgrades; every unlock has a test.

## Checklist

- [ ] Only `content/tables.rs` changed for economy values
- [ ] Stable string IDs preserved
- [ ] No `f64` state fields, no transcendental functions in simulation
- [ ] Pacing bands met for `active`, `idle`, `offline`
- [ ] `cargo test -p hadal-core --lib` and the release pacing test pass
- [ ] `docs/BALANCE.md` updated with anchored results
- [ ] `cargo xtask gate` passes

## Commands

```
cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert
cargo run -p hadal-sim --release -- --profile idle --hours 1 --assert
cargo run -p hadal-sim --release -- --profile offline --hours 8,72 --assert
cargo test -p hadal-core --lib
cargo test -p hadal-core --release --test pacing
cargo xtask gate
```
