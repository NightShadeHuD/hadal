# QA Rubric

The studio grades every handoff against this rubric. A handoff is a diff, a report, or a
snapshot set produced by one role pass (see `AGENTS.md` §14). The reviewer must be a
**different model** than the author and must record the report in the format below.

## Score (1-100)

| Dimension | Weight | What earns the points |
|---|---|---|
| Correctness | 30 | The change does what it claims, end to end, with no hidden scope gaps. |
| Tests | 20 | Every new behaviour has a test; edge cases and failure paths covered; no tautologies or source-text assertions. |
| Determinism | 15 | No wall clock, RNG, or environment in `tick()`; `Decimal` only; identical ops give the same state hash. |
| UX / polish | 15 | Copy is accurate and readable; colour is never the sole encoding; ASCII/reduce-motion honoured; layout meets minimums. |
| Performance | 10 | Meets the budgets in `AGENTS.md` §12; no avoidable allocation on hot paths; snapshot/perf proof supplied where relevant. |
| Maintainability | 10 | Boring, single-sourced, dead code removed, one convention, docs updated. |

Dimension score = weight × (awarded / max). The total is the sum of the six dimension scores,
rounded to the nearest integer, in `1..=100`.

## Pass/fail gates

All gates are binary. Any FAIL blocks a SHIP verdict.

| Gate | PASS condition |
|---|---|
| Build | `cargo build -p hadal` succeeds. |
| Format | `cargo fmt --all --check` is clean. |
| Clippy | `cargo clippy -p hadal --all-targets -- -D warnings` is clean. |
| Tests | `cargo test -p hadal` and `cargo test -p hadal-core --lib` are green. |
| Determinism | Determinism tests pass; no new `f64` state, no transcendental functions in simulation, no wall clock/RNG in core. |
| No placeholders | No `TODO`, `FIXME`, stubs, mocks in production paths, or narrowed tests. |
| Snapshot / perf proof | Any UI change updates its goldens; performance claims carry a measurement. |
| Independence | The reviewer's base model family differs from every author's base family. Same-base variants (e.g. two reasoning modes of one preset) are mechanical gate runs only and cannot yield a cross-model SHIP. |

## Reviewer report format

```
Artifact:        <branch / diff / files / report name>
Author model:    <model id of the author>
Reviewer model:  <model id of the reviewer>   (must differ from the author)
Reviewer base model/family: <base weights id + family, e.g. "gpt-oss-20b / OpenAI-OSS">   (must differ from every author's base family)
Score:           <1-100>
Gates:
  build:         PASS | FAIL
  format:        PASS | FAIL
  clippy:        PASS | FAIL
  tests:         PASS | FAIL
  determinism:   PASS | FAIL
  no-placeholders: PASS | FAIL
  snapshot/perf: PASS | FAIL
  independence:  PASS | FAIL
Blocking findings:
  - <file:line — issue — required fix>   (or "none")
Verdict:         SHIP | FIX
```

`SHIP` requires a score of at least 85 **and** every gate PASS. Otherwise the verdict is `FIX`,
and the blocking findings are addressed before the next pass.
