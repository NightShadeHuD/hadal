# HADAL — Balance Document

Pacing bands (the M1 acceptance contract), measured simulation results, the simulator's
check-in policy, tunable constants, and the exact commands.

All results below were measured on 2026-11-26 with the release simulator at the current
constants.

## Pacing bands (M1 acceptance contract)

The deterministic `active` policy pulses when the capacitor is full, tunes on every carrier
change, buys all affordable upgrades and hulls (cheapest first), buys the highest-ROI
generator each second, and descends when available (subject to the 300 s check-in below).

| Scenario | Required outcome |
|---|---|
| First Descent, fresh active | 15–35 min (900–2100 s) |
| First Descent, fresh idle (no manual actions) | 30–90 min (1800–5400 s) |
| First 30 min active | gap between purchases/unlocks ≤ 180 s; no stuck state |
| 1 h active | ≥2 descents, ≥6 era Fathoms, ≥1 hull |
| 24 h check-ins | ≥8 descents, ≥30 era Fathoms, all 4 generator types used, ≥6 hulls, depth ≥9,400 m, all finite |
| 72 h check-ins | ≥20 descents, ≥100 era Fathoms, depth ≥10,000 m, ≥12 hulls, ≥1 unowned goal remains |
| 8 h offline from fresh | credits exactly 8 h, no NaN, summary reports cap, no credit double-applied |
| 72 h offline from a midgame fixture with Abyssal Reservoir | credits 72 h, ≥5 descents, no NaN, purchases/descents aggregated |
| Online/offline equivalence | 8 h ticks with same automation → identical state hash |
| Clock skew | negative gap = 0; 30-day gap capped; no panic |
| Dead zones/soft-locks | none in first 30 min; every content item has a reachable path in 72 h |

Only constants in `content/tables.rs` may move during tuning. Formulas, reset scope, and
layer semantics are fixed.

## Current sim results

### Active — `--profile active --hours 1,24,72 --assert` (18 passed, 0 failed)

| Band | Measured | Required |
|---|---|---|
| active first descent | **900 s (15.0 min)** | 900–2100 s |
| first 30 min gap | 21 s | ≤ 180 s |
| 1 h: descents | 4 | ≥ 2 |
| 1 h: era Fathoms | 14 | ≥ 6 |
| 1 h: hulls | 4 | ≥ 1 |
| 24 h: descents | 279 | ≥ 8 |
| 24 h: era Fathoms | 7,366 | ≥ 30 |
| 24 h: generators used | 4 | 4 |
| 24 h: hulls | 16 | ≥ 6 |
| 24 h: depth | 147,412 m | ≥ 9,400 m |
| 72 h: descents | 857 | ≥ 20 |
| 72 h: era Fathoms | 26,580 | ≥ 100 |
| 72 h: depth | 436,412 m | ≥ 10,000 m |
| 72 h: hulls | 16 | ≥ 12 |
| 72 h: unowned goal remains | 1 | ≥ 1 |
| clock skew: negative gap → 0 credited | 0 s | 0 |
| clock skew: 30-day gap capped | credited 28,800 s, cap 28,800, capped 2,563,200, hit | capped at 8 h |
| online/offline 8 h equivalence | `93c0fd69ce201f1a` = `93c0fd69ce201f1a` | match |

Active metric snapshots:

| Elapsed | Echo | Rate | Peak | Descents | Era | Hulls | Depth | Gens |
|---|---|---|---|---|---|---|---|---|
| 1 h | 1.63e5 | 2.92e4 | 2.99e5 | 4 | 14 | 4 | 9,912 m | 4 |
| 24 h | 7.97e11 | 6.29e10 | 2.05e12 | 279 | 7,366 | 16 | 147,412 m | 4 |
| 72 h | 3.73e12 | 2.46e11 | 6.71e12 | 857 | 26,580 | 16 | 436,412 m | 4 |

The first active descent lands exactly on the lower band edge: **15.0 min (900 s)**.

### Idle — `--profile idle --hours 1 --assert` (4 passed, 0 failed)

| Band | Measured | Required |
|---|---|---|
| idle first-descent availability | 2,026 s | 1,800–5,400 s |
| clock skew: negative gap → 0 credited | 0 s | 0 |
| clock skew: 30-day gap capped | capped at 8 h | capped |
| online/offline 8 h equivalence | match | match |

The idle policy descends at the first 300 s check-in at or after availability, i.e. 2,100 s.

### Offline — `--profile offline --hours 8,72 --assert` (7 passed, 0 failed)

| Band | Measured | Required |
|---|---|---|
| 8 h offline: credits | 28,800 s | 28,800 s |
| 8 h offline: reports cap, no double credit | cap 28,800, credited 28,800, capped 0, ticks 288,000 | exactly 8 h |
| 72 h offline: credits | 259,200 s | 259,200 s |
| 72 h offline: descents | 12,184 | ≥ 5 |
| clock skew: negative gap → 0 credited | 0 s | 0 |
| clock skew: 30-day gap capped | capped at 8 h | capped |
| online/offline 8 h equivalence | match | match |

Offline reports:

| Run | Credited | Cap | Ticks | Echo Δ | Purchases | Descents | Fathoms |
|---|---|---|---|---|---|---|---|
| 8 h (fresh) | 28,800 s | 28,800 s | 288,000 | 3.12e3 | 0 | 0 | 0 |
| 72 h (midgame fixture) | 259,200 s | 604,800 s (168 h) | 2,592,000 | 0 | 289,161 | 12,184 | 123,562 |

The 8 h run starts from a fresh state (no automation hulls), so it yields passive Echo only.
The 72 h run starts from the midgame fixture (all automation hulls, Abyssal Reservoir) and
therefore runs the full automated loop.

## The 300 s simulator check-in policy

`DESCENT_CHECKIN_SECS = 300`. In the simulator, descents happen only on five-minute
check-in boundaries, not every tick.

Why:

- A human player cannot descend on every tick.
- Gating descents to a fixed cadence stops the era-Fathom feedback from running away, which
  would otherwise exhaust all content early.
- It lets a run build past 1e6 so the Tidal Core (2.5M) and higher Descent buckets are
  reachable — this is what satisfies the "all 4 generator types used by 24 h" band.
- It places the first descent at the first check-in at or after availability.

This is a **simulator policy for long AFK-style runs, not a game rule**: an online player may
descend the instant it is available.

## Idle interpretation

The idle policy performs the same purchases, hull buys and descents as active, but **never
pulses and never tunes** — pure passive production, the AFK player. It records both when a
Descent *becomes* available (2,026 s) and when the policy actually descends (2,100 s). The
30–90 min idle band is intentionally much wider than the active band because passive
production is far slower than a tuned, pulsing run.

## Final constants changed

- **Fractal Hull: 1,000 → 50,000 Fathoms.** At 1,000 the top of the hull tree was reached
  within the first hours, leaving no long-horizon goal. Raising it to 50,000 keeps Fractal
  Hull as the single unowned goal at 72 h (the "≥1 unowned goal remains" band passes with
  exactly 1) and gives the final Fathom-gain multiplier a meaningful cost.
- The rest of the tables are unchanged from the plan's M1 constants (costs, ratios, base
  rates, unlock thresholds, cap hours, multipliers, bucket boundaries and formula).

## Tunable constants vs frozen semantics

### Tunable (in `content/tables.rs`)

Generator base costs, ratios and base rates; upgrade costs and multipliers; hull costs and
prerequisites; achievement and log thresholds; carrier cycle (900 ticks / 90 s); pulse
capacitor (250 ticks / 25 s); pulse yield (rate × 10); offline cap base (8 h) and hull
extensions (24, 72, 168 h); Descent threshold (≥ 1,000,000); station draw (0.1 Echo/s); era
multiplier (0.05); achievement multiplier (0.01); Tidal (×1.5) and Fractal (×2) gain
multipliers.

### Frozen (no tuning may change)

Descent gain formula (`base = 3 + 3d + s`, `gain = max(3, floor(base × tidal × fractal))`);
bucket boundaries (2.154434690031884, 4.641588833612779); reset scope; layer semantics;
offline contract (automation only, 100 %, cap from hulls, save-once); tick order (10 steps,
10 Hz); Decimal determinism; achievement bonus; carrier match multiplier (×1.5).

## Exact commands

```
cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert
cargo run -p hadal-sim --release -- --profile idle --hours 1 --assert
cargo run -p hadal-sim --release -- --profile offline --hours 8,72 --assert
cargo test -p hadal-core --release --test pacing -- --nocapture
```

Profiles: `active` (pulse when full, tune on carrier change, buy all affordable, descend on
check-in), `idle` (no pulse, no tune), `offline` (one `simulate_offline` from a fixture).
`--assert` checks the bands for the profile(s) run and exits non-zero on failure. `--hours`
sets the simulated lengths; `--hours 0` exits with an empty result (used for artifact smoke).
