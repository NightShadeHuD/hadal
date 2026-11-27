# QA Ledger

Studio grading record (see `AGENTS.md` §14 and `docs/QA-RUBRIC.md`). Every handoff is
reviewed by a **different model** than its author and graded 1-100 against the rubric.

> **Reviewer-independence correction (2026-11-27).** The rows below record reviewer
> `qa-pass-1`, which ran on the **same Qwen base** as the `studio:high` authors (see
> "Infrastructure audit — model identity"). Their scores are **same-base mechanical gate
> checks**, not genuine cross-model taste reviews, and are retained unchanged for the record.
> The genuine different-model approval for the milestone is the CTO executive sign-off
> (deepseek-flash).

| Pass | Milestone / artifact scope | Author model(s) | Reviewer model | Score | Gates (build / fmt / clippy / tests / determinism / no-placeholders / snapshot-perf) | Verdict | Blocking findings |
|---|---|---|---|---|---|---|---|
| QA-1 | Foundation repair: pixel/graphics foundation (`render/{canvas,radar,wave,spark,big}.rs`), `Anim`/`AnimState` rate sampling, story/tutorial detail + HELP MISSION, studio governance docs (`AGENTS.md` §14, `docs/QA-RUBRIC.md`) | deepseek/deepseek-flash | qa-pass-1 (independent) | 92 | PASS / PASS / PASS / PASS (hadal 195, core 210) / PASS / PASS / PASS | SHIP | none (non-blocking: step-index display bug in HELP; tutorial ASCII checks only on `detail`; `_head` prefix nit; 11 `allow(dead_code)` scaffolding entries) |
| QA-2 | Full milestone: pixel foundation wired into SONAR graphical layout, story/tutorial depth + HELP MISSION/TUTORIAL REFERENCE, DESCENT trench + hull schematic, ARCHIVE SIGNAL MAP, coach band/scrim/target border/tab naming, tutorial input gate + nudge | mixed local / deepseek: foundation-repair, screen-art, story-depth, coach-ux / coach-local, tutorial-gate | qa-pass-1 (independent) | 85 | PASS / PASS / PASS / PASS (hadal 219, core 210) / PASS / PASS / PASS (35 goldens, SONAR 600-frame ~1.16 ms debug) | FIX | 1 — tutorial replay from SYSTEM leaves the player on SYSTEM with step 0 active where the coach points at `TARGET SONAR TAB` but `OpenSonar`/`Cancel` are gated, soft-locking the player (`crates/hadal-tui/src/input.rs:233-350`, `crates/hadal-tui/src/app.rs:938-945`) |
| QA-3 | Final full-diff review after the QA-2 fix pass + graphical boot redesign: `replay_tutorial`→SONAR reset, `OpenSonar` gate additions, ChainPanel target covers header+first row, state-driven `SIGNAL TUNED/LOCKED`, glyph trim, `hits.rs` LF, spark zero `·`/`.`, nudge expiry+redraw, new graphical boot (wordmark + radar + wave + staged beats, 800/1400/2000/2500/400/800 ms) | mixed local / deepseek (same role set, fix-pass authors) | qa-pass-1 (independent) | 93 | PASS / PASS / PASS / PASS (hadal 228, core 210) / PASS / PASS / PASS (36 goldens, SONAR 600-frame ~1.13 ms debug) | SHIP | none |
| Release-eng | Release engineering: floor raise (399→448), docs accuracy (AGENTS.md §7/§13, TESTING.md, README.md, CHANGELOG.md, QA-LEDGER.md), startup assertion split (debug 250 ms / release 150 ms), gate rerun with --release-pacing and privacy | release-eng (single) | qa-pass-1 (final audit) | — | PASS / PASS / PASS / PASS (core 210, hadal 229, sim 8, pacing 1 = 448 total) / PASS / PASS (37 goldens) / PASS (floor 448≥448) / PASS (--release-pacing) / PASS (privacy) | FIX | Final audit verdict: FIX (docs/claims); corrections applied; gate rerun (--release-pacing + privacy) green; CTO sign-off pending. |
| Hotfix | Boot D1: `App::tick` marks the frame dirty while boot is active so the three `BOOT_BEATS` reveal and radar/wave animate; regression test `boot_stages_redraw_and_reveal_beats` at 900/1500/2100 ms | tutorial-gate (local) | director (deepseek, different base family) | — | PASS / PASS / PASS / PASS (hadal 230, core 210) / PASS / PASS (37 goldens) / PASS (floor 449≥449) | SHIP | none; evidence `target/manual-playthrough/REPORT.md` D1 |
| Audit v0.2.1 → 0.2.2 | Post-release audit of v0.2.1: export path provisioning, `--demo` config isolation, overlay/banner placement, modal input (Y/N, q), reduce-motion contacts, offline-overlay boot defer, stale temp cleanup, replay copy; save `verify` stored-bytes checksum + 4 MiB cap; sim offline era metric; test determinism | audit-runtime, audit-persist, audit-core / fix-runtime, fix-persist (local studio) | director (deepseek-flash, different base family) | — | PASS / PASS / PASS / PASS (hadal 253, core 210, sim 9, pacing 1 = 473) / PASS / PASS (37 goldens) / PASS (floor 473≥473) | SHIP | 3 P1 + P2s fixed; evidence `target/audit-*/REPORT.md` |

## Infrastructure audit — model identity (2026-11-27)

Read-only audit of the local inference endpoint (`C:/models/models.ini`, `GET /v1/models` on
`127.0.0.1:8080`). No files, configs, or models were changed or loaded.

| Harness label / id | Base model GGUF | Context (`ctx-size`) | Family | Cross-model reviewer? |
|---|---|---|---|---|
| `studio` (`studio:high`, `studio:off`) | `Qwen3.6-35B-A3B-UD-Q4_K_M.gguf` | 131072 | Qwen | No — one preset, two reasoning modes |
| `reviewer-20b` | `gpt-oss-20b-MXFP4.gguf` | 65536 | OpenAI-OSS (gpt-oss) | **Yes — designated cross-family reviewer** |
| `studio-38` | `Qwen3.8-27B-UD-IQ3_S.gguf` | 98304 | Qwen | No — same family |

Evidence: `models.ini` defines exactly three sections `[studio]`, `[reviewer-20b]`,
`[studio-38]`; there is no `[studio:high]`/`[studio:off]` section. `GET /v1/models` lists the
presets as model ids with their full `--ctx-size`/`--model` args, confirming one `studio`
preset at 131072 ctx and `reviewer-20b` at 65536 ctx. A grep for `studio:high|studio:off` across
the repo and `C:/models/*.ini|*.bat` returns nothing — the labels are harness-side reasoning
modes over the same weights.

**Correction (reviewer independence).** The reviewer recorded as `qa-pass-1` in the QA-1, QA-2,
QA-3 and Release-eng rows ran on the **same Qwen base** as `studio:high`/`studio:off`. Those
scores are therefore **same-base mechanical gate checks**, not genuine cross-model taste
reviews. The historical scores are retained for the record but are not evidence of independent
cross-model review. The genuine different-model approval for this milestone is the **CTO
executive sign-off (deepseek-flash)**, which read the code/snapshots and the final gate
receipts.

## Model policy

Stated policy: **local execution only; deepseek may act as executive only.** `AGENTS.md` §14
records the policy (local execution; deepseek executive-only). Session provenance remains
outside the tree. The designated local reviewer is `reviewer-20b` (gpt-oss-20b, 65536 ctx);
`qa-pass-1` (studio:off) is same-base as the authors and its rows are labelled as mechanical
checks.

**Designated future reviewer:** `reviewer-20b` (gpt-oss-20b-MXFP4, 65536 ctx) — a different
base family from Qwen, already available locally. No new model install is needed. Harness
routing must point the reviewer role at `reviewer-20b`; that routing lives outside the repo and
was **not** changed here.

## Notes

- QA-1 measured counts at review time: `hadal` 195, `hadal-core --lib` 210.
- QA-2 measured counts at review time: `hadal` 219, `hadal-core --lib` 210, goldens 35.
- QA-3 measured counts at review time: `hadal` 228, `hadal-core --lib` 210, goldens 36;
  SONAR 600-frame measurement 1.13 ms/frame (debug, 100×30). Stable over 5 repeated
  `cargo test -p hadal` runs and `--test-threads` 1/2/4. One isolated 227/228 failure was
  observed once while a second cargo build held the package-cache lock; not reproducible
  (informational, not attributed to the code).
- QA-3 non-blocking carry-overs: the boot hint and first beat visually sit inside the radar
  dome while the code comment claimed the radar "never crosses the text" (`crates/hadal-tui/src/boot.rs:69`);
  the `Descend` coach `capitalized_after` prefix is a stale no-op; the gate has no mechanical
  (step × owner-tab) `Open*` regression test. The "no stage-0 golden" carry-over was resolved
  by the release pass (`boot_graphical_stage0_80x24` added).
- Release-eng (2026-11-27): floor raised 399→448. Measured counts: core 210, hadal 229, sim 8, pacing 1, total 448. Goldens: 37. Startup measured 18 ms (debug); assertion split 250 ms debug / 150 ms release. Gate: all steps PASS including --release-pacing and privacy. Receipt: `target/gate-receipt`. Model: llama.cpp/studio.
