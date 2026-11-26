# HADAL — Design Document

Approved fiction, loop mechanics, content tables, and formulas. Source of truth for M1
content.

## Fiction

The last operator signed off fourteen years ago. The station log has one entry since:
*"It is still repeating."*

You are Operator 7. **KESTREL**, the station's degraded intelligence, wakes the
hydrophones. The trench emits a carrier that is not random: it has grammar, and it is
getting clearer as the station sinks. Each Descent releases ballast and lowers K-9 another
500 m. The logs of the previous operator, **Voss**, surface between runs. The carrier is
eventually revealed as the station's own future distress call folding backward through the
trench; the layers are attempts to answer it.

Tone: restrained, procedural, eerie. No mascots, no jokes, no lore dumps. Narrative arrives
as short log entries and one-line boot messages.

> `BALLAST RELEASED. K-9 SINKS THROUGH ITS OWN SHADOW.`
>
> `THE CARRIER IS NOT A DISTRESS CALL. IT IS A REHEARSAL.`

## Visual identity

### Palette

| Name | Hex |
|---|---|
| Abyss (background) | `#03080B` |
| Panel | `#07141A` |
| Phosphor teal (accent) | `#4DE1C1` |
| Cold steel | `#7FB7C8` |
| Sodium amber | `#FFB454` |
| Alert | `#FF6B6B` |
| Bone text | `#D8F3EC` |
| Dim | `#2A4A52` |

### Type

Monospace only. ALL-CAPS micro-labels, wide letter spacing, one dominant number per zone.
No emoji; glyphs are widely supported box/block/geometric characters with a full ASCII
fallback.

### Number display

Display-only rule; formulas are unchanged.

| Range | Display |
|---|---|
| `< 1e6` | grouped integer with thousands commas (`1`, `999`, `1,000`, `999,999`), no K suffix |
| `1e6` to `< 1e36` | three significant figures with trailing zeros and suffix `M/B/T/Qa/Qi/Sx/Sp/Oc/No/Dc` (`1.00M`, `1.24M`, `12.3M`, `123M`, `1.00Dc`); Dc covers the 1e33–1e36 group |
| `≥ 1e36` | scientific, three significant figures (`1.00e36`) |

Amounts floor, costs ceil, affordability compares exact `Decimal` values, so a shown amount
never exceeds a shown cost. Rates show two decimals below 1e6 (`18.60`). K is folded into
grouped integers; this refines the plan's earlier K listing.

### Form

Thin-line console frames, dotted separators, a radial sonar scope, a vertical depth gauge,
and a carrier spectrum strip.

### Hierarchy

Echo amount and rate are the largest text; the next goal (Descent gain) is always visible;
actions are one keystroke.

### Themes

`abyss` (default), `high-contrast`, `amber-crt`, `mono`. Colour is never the only carrier of
state; matched bands and alerts also carry text labels.

## Tutorial

Purpose: teach by doing, not a text wall. The tutorial is an interactive coach, not a
first-run briefing.

- **Chapters.** FIRST SIGNAL (steps 1–9) reaches the first Descent and the first hull; SECOND
  SIGNAL (steps 10–13) covers the second Descent, SYSTEM, the manual and offline.
- **Persistence.** Progress lives in `MetaState::tutorial` (`TutorialState`) and is saved with
  the game. A legacy save with no tutorial data defaults to complete; a new save starts at
  step 1.
- **Skip and replay.** Skip is always available. Replay from SYSTEM is allowed only before the
  first Descent (`descents == 0`); once a run has descended, start a new save instead.
- **Copy rules.** The action comes first, jargon is named only after it is introduced, and
  there are no lore dumps or mascots.
- **Neutrality.** The tutorial never changes production, costs or prestige; steps advance
  deterministically from what the player does.

| # | id | chapter | action / target | completes when |
|---|---|---|---|---|
| 1 | Pulse | FIRST SIGNAL | SPACE (pulse bar) | a manual pulse fires |
| 2 | FirstGenerator | FIRST SIGNAL | `h` (Hydrophone row) | ≥1 Hydrophone |
| 3 | SecondGenerator | FIRST SIGNAL | `h` again (Hydrophone row) | ≥2 Hydrophones |
| 4 | Drifter | FIRST SIGNAL | reach 10 Hydrophones, `d` (Drifter row) | ≥1 Drifter |
| 5 | Upgrade | FIRST SIGNAL | `u`, Enter (Signal Chain) | Impedance Match owned |
| 6 | Tune | FIRST SIGNAL | `f` after the carrier drifts (tune strip) | tuned to the carrier since the drift |
| 7 | Peak | FIRST SIGNAL | reach `1.00M` (Peak readout) | Peak ≥ 1,000,000 |
| 8 | Descend | FIRST SIGNAL | `2`, `p`, `y` (DESCENT) | ≥1 Descent |
| 9 | Hull | FIRST SIGNAL | `2`, select Auto-Pulse, Enter (hull row) | Auto-Pulse owned |
| 10 | SecondDescent | SECOND SIGNAL | reach `1.00M` again and descend | ≥2 Descents |
| 11 | System | SECOND SIGNAL | `4` (SYSTEM tab) | SYSTEM opened |
| 12 | Manual | SECOND SIGNAL | `?`, Esc (help key) | manual opened then closed |
| 13 | Offline | SECOND SIGNAL | Enter (acknowledge) | tutorial acknowledged |

## Layer table

| Layer | Verb | Currency | Reset wipes | Persists | New math |
|---|---|---|---|---|---|
| In-run | Pulse, tune, buy | Echo | — | — | Geometric cost curves, multiplicative upgrade chain |
| L1 | **DESCENT** | **Fathom** | Echo, generators, in-run upgrades, charge, carrier state, run tick counters | Fathoms, era/lifetime stats, hulls, achievements, logs | 3 Fathoms + 3 per decade, three sub-buckets per decade |
| L2 (M2) | **TRANSMIT** | **Cipher** | Fathoms, hulls, Echo, run, Chambers | Ciphers, Doctrines, achievements, logs, lifetime stats | sqrt of era Fathoms |
| L3 (M4) | **BREACH** | **Tide** | Ciphers, Doctrines, Fathoms, hulls, run | Tides, strata rules, archive | Tide-window-gated production |

## M1 generator table

Cost is `base_cost × ratio^owned`. Echo/s is `base_rate × tier_multiplier × global_multiplier`.

| Key | Generator | Base cost | Ratio | Base E/s | Unlock |
|---|---|---|---:|---:|---|
| `h/H` | Hydrophone | 10 | 1.15 | 0.5 | start |
| `d/D` | Drifter | 250 | 1.18 | 8 | 10 Hydrophones |
| `m/M` | Sonar Mast | 20,000 | 1.20 | 120 | 6 Drifters |
| `c/C` | Tidal Core | 2,500,000 | 1.22 | 2,500 | 5 Sonar Masts |

## Signal Chain (in-run upgrades)

One-time, reset by Descent. Effects are multiplicative unless noted.

| Upgrade | Cost | Effect | Unlock |
|---|---:|---|---|
| Impedance Match | 100 | all production ×2 | 1 Drifter |
| Sweep Profile | 750 | Hydrophones ×3 | 10 Hydrophones |
| Cascaded Gain | 8,000 | all production ×2 | 1 Sonar Mast |
| Stable Drift | 40,000 | Drifters ×3 | 6 Drifters |
| Coherent Aperture | 300,000 | all production ×2 | 5 Sonar Masts |
| Mast Phasing | 2,000,000 | Sonar Masts ×4 | 5 Sonar Masts |
| Core Resonance | 25,000,000 | Tidal Cores ×3 | 1 Tidal Core |
| Full Spectrum | 150,000,000 | all production ×3 | 25 Sonar Masts |

Impedance + Cascaded + Coherent + Full Spectrum multiply all output by 24 before tier
upgrades; Masts get ×4 and Cores ×3. This is the slope that carries a run past 1e6.

## Hull tree (permanent)

Cost in current Fathoms; the prerequisite must be owned; effects persist across descents.
Spending `fathoms` does not weaken the `era_fathoms` passive.

| Hull | Cost | Prereq | Effect |
|---|---:|---|---|
| Auto-Pulse | 1 | — | Fires a pulse whenever the capacitor is full |
| Ballast Recall | 2 | — | Start runs with 5 Hydrophones |
| Tuned Ballast | 3 | Ballast Recall | Auto-tune on every carrier change |
| Auto-Buy I | 5 | Auto-Pulse | Auto-buy Hydrophones and Drifters |
| Deep Memory | 8 | — | Global ×1.25 |
| Coiled Hull | 12 | Deep Memory | Pulse yield ×2 |
| Auto-Buy II | 20 | Auto-Buy I | Adds Sonar Masts |
| Ballast Governor | 25 | Ballast Recall | Enables an auto-descent target |
| Long Storage | 30 | — | Offline cap 24 h |
| Chain Automator | 40 | Auto-Buy I | Buys the cheapest affordable unowned upgrade once/s |
| Null Array | 50 | Deep Memory | Global ×2 |
| Auto-Buy III | 80 | Auto-Buy II | Adds Tidal Cores |
| Extended Ballast | 120 | Long Storage | Offline cap 72 h |
| Tidal Loop | 200 | Tuned Ballast | Fathom gain ×1.5 |
| Abyssal Reservoir | 350 | Extended Ballast | Offline cap 168 h |
| Deep Field | 600 | Null Array | Global ×3 |
| Fractal Hull | **50,000** | Tidal Loop | Fathom gain ×2 |

Fractal Hull was raised from the early 1,000 to **50,000** after simulation showed the top of
the tree was reached within hours; the measured `active` runs still leave exactly one unowned
hull (Fractal Hull) at 72 h, which is the intended long-horizon goal.

- Auto-buy selects, once per second, the affordable unlocked generator with the highest
  `base_rate × tier_multiplier / cost`; ties resolve by enum order. Per-generator toggles
  live in SYSTEM; default on.
- Auto-descent target defaults to 0 (off).

## Carrier rules

- Six bands, numbered 0–5, cycling `0→1→…→5→0` every 900 ticks (90 s). The header shows the
  band as `CARRIER nn` (band + 1).
- A new run starts with **carrier 0 and tuned 0 (matched)**, so a fresh run begins with the
  ×1.5 bonus.
- Matching (`tuned == carrier`) grants ×1.5 global production. A mismatch carries no
  penalty.
- A band is recorded in `bands_seen` (and the match counted in `bands_matched`) only when a
  tuning action — manual `f` or Tuned Ballast auto-tune — results in a match. A bare carrier
  change does not count.
- Tuned Ballast sets the tuned band to the carrier on every carrier change.

## Achievements and Archive logs

### Achievements

Each unlocked achievement gives +1% global production and persists across descents. Eleven
exist:

`first_contact` (manual pulse), `hydrophile` (1 Hydrophone), `chorus` (50 Hydrophones),
`drifter` (1 Drifter), `mast` (1 Sonar Mast), `core` (1 Tidal Core), `first_descent`,
`six_deep` (6 descents), `century` (100 lifetime Fathoms), `all_bands` (all six tuned),
`signal` (best peak ≥ 1e12).

### Archive logs

Ten logs persist, show a toast once, and live in ARCHIVE. They never gate mechanics.

`boot`, `first_pulse`, `first_hydro`, `carrier_lock`, `first_descent`, `depth_9000`,
`peak_1e9`, `fathoms_100`, `all_bands`, `auto_pulse`.

Narrative text lives in `content/story.rs`.

## Descent formula

Available when Peak Echo ≥ **1,000,000**. Gain uses Peak Echo's normalized base-10 parts
`(mantissa, exponent)`:

```
d = exponent - 6                     (integer; 0 at 1e6)
s = 0 if mantissa < 2.154434690031884
s = 1 if mantissa < 4.641588833612779
s = 2 otherwise
base = 3 + 3d + s
gain = max(3, floor(base × tidal(1.5) × fractal(2)))
```

`tidal` is ×1.5 when Tidal Loop is owned, `fractal` is ×2 when Fractal Hull is owned. The
integer `base` times the exactly-representable factors `1.5`/`2.0` is computed in `f64` and
floored; the result is deterministic.

### Worked examples

| Peak Echo | d | s | base | gain (no hulls) | +Tidal | +Tidal+Fractal |
|---|---|---|---|---|---|---|
| 1.00e6 | 0 | 0 | 3 | 3 | 4 | 9 |
| 2.16e6 | 0 | 1 | 4 | 4 | 6 | 12 |
| 4.642e6 | 0 | 2 | 5 | 5 | 7 | 15 |
| 1.00e7 | 1 | 0 | 6 | 6 | 9 | 18 |
| 1.00e8 | 2 | 0 | 9 | 9 | 13 | 27 |
| 1.00e9 | 3 | 0 | 12 | 12 | 18 | 36 |

On Descent: `fathoms += gain`, `era_fathoms += gain`, `lifetime_fathoms += gain`,
`stats.descents += 1`; the run resets (Echo 0, peak 0, generators to 5 Hydrophones if Ballast
Recall, upgrades empty, charge full, carrier band 0, tuned 0). The confirmation shows the
gain, the next-bucket threshold, and exactly what resets and persists.

Passives: `era = 1 + 0.05 × era_fathoms`; `achievements = 1 + 0.01 × achievement_count`; hull
global multipliers multiply; carrier ×1.5 when matched; all apply to production and pulse.

## Depth formula

`depth_m = 7,912 + 500 × descents`, stored in `MetaState::depth_m` and advanced by 500 on
every Descent.

## Statistics

| Field | Definition |
|---|---|
| `lifetime_echo` | Total Echo ever produced: passive production **plus** manual and auto pulse yields. Purchases do not reduce it. |
| `best_peak` | Highest single-run peak Echo ever reached. |
| `pulses_manual` | Manual pulses fired. |
| `pulses_auto` | Auto-Pulse fires. |
| `purchases` | Purchase **actions** performed; a Ten or Max buy counts once. |
| `descents` | Descents completed. |
| `total_ticks` | Ticks simulated across all sessions. |
| `bands_matched` | Times a tuning action resulted in a carrier match. |

## Ballast Governor bounds

The valid target set is exactly `0` (off) or `3..=1_000_000`. Out-of-range proposals are
rejected with a readable message and are never silently clamped.

## Offline contract

- Base cap **8 h**; hulls extend to 24 h → 72 h → **168 h**.
- Offline rate is 100% of automated production; no reduced-rate punishment.
- `elapsed = max(0, now_unix - saved_at_unix)`; `credited = min(elapsed, cap)`; ticks =
  whole credited seconds × 10.
- Negative clock jumps credit nothing; large jumps are capped.
- Simulation is the exact online tick with no manual actions: auto-pulse/tune/buy/upgrade/
  descend happen only if owned.
- A welcome-back summary appears for absences ≥ 60 s. The game saves immediately before
  accepting input.

## Session rhythm targets

25 s pulse / 90 s carrier beat → 2–8 min upgrade or generator goal → 15–35 min first
Descent → hours Transmit era → days Breach. No decision is mandatory while offline.

## Demo fixture

`--demo` builds a deterministic late-game state in memory (never saved or loaded) for UI
review:

- Fathoms 140, era Fathoms 120, lifetime Fathoms 300, depth 9,412 m.
- 15 hulls (all except Deep Field and Fractal Hull), 10 of 11 achievements (all except
  Signal), all 10 logs, every band seen, all four auto-buy toggles on.
- Stats: lifetime Echo 1.2e10, best peak 8.4e11, 210 manual + 900 auto pulses, 640
  purchases, 12 descents, 3,600,000 total ticks, 180 bands matched.
- Run: Echo 5.2e8, peak 9.1e8, generators `[46, 28, 14, 6]`, all 8 upgrades, full
  capacitor, carrier band 2, tuned band 2.
