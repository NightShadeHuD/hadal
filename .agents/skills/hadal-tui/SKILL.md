# HADAL TUI Skill

**Purpose:** Change TUI layout, themes, input, animation or accessibility without breaking
the core boundary or golden snapshots.

**When to use:** Adding or changing a screen, layout, theme, animation, input binding,
accessibility option, or terminal-restore behaviour.

## Workflow

1. **Read `docs/DESIGN.md`** for the palette, themes, layout and accessibility rules.
2. **Read `docs/ARCHITECTURE.md`** for the `StateView`/`Action` boundary. The UI reads
   `StateView` accessors and mutates only through `Engine::apply()`; it holds no economy
   state.
3. **Edit under `crates/hadal-tui/src/`:**
   - Screens/overlays: `render/{sonar,descent,archive,system,help,coach,dive}.rs`
   - Tutorial copy/step table (in core): `crates/hadal-core/src/content/tutorial.rs`
   - Coach band, highlights, skip/replay confirm: `render/coach.rs`
   - Shared visual vocabulary, too-small screen: `render/mod.rs`
   - Click hit-testing: `render/hits.rs`
   - Derived names/requirements/progress/governor/objective/selection: `model.rs`
   - Keymap and command queue: `input.rs`
   - Themes, colour depth, contrast: `theme.rs`
   - Glyph sets: `glyphs.rs`
   - Control-char sanitisation: `sanitize.rs`
   - Terminal guard and panic hook: `terminal.rs`
   - Session loop and config: `app.rs`
   Never touch `hadal-core` for a presentation change.
4. **Update goldens** for affected screens at all sizes:
   ```
   INSTA_UPDATE=always cargo test -p hadal
   ```
   Inspect the resulting `.snap` diffs before committing.
5. **Verify accessibility:**
   - 80×24 renders; below that only the too-small screen (no panic).
   - Colour is never the sole encoding (matched bands, alerts and locks carry text labels).
   - Reduce motion drops animation, not information.
   - ASCII (`--ascii`) and mono work.
   - Full keyboard path; mouse additive.
   - Body text ≥ 4.5:1, numerals ≥ 3:1 (`theme::tests::all_themes_pass_wcag`).
6. **Run the TUI tests:**
   ```
   cargo test -p hadal
   ```
7. **Run `cargo xtask gate`** before committing.

## Golden set (34)

`boot_100x30`, `too_small_79x24`, `demo_banner_100x30`,
`coach_{step0_80x24,step0_100x30,waiting_hydro_80x24,tune_waiting_80x24,hull_descent_80x24,offline_80x24,ascii_80x24}`,
`skip_confirm_80x24`,
`sonar_{80x24_unicode,100x30_unicode,140x40}`,
`sonar_{80x24_ascii,100x30_ascii,80x24_mono}`, `sonar_completed_80x24`,
`descent_{80x24,100x30,140x40}`, `confirm_descent_80x24`,
`archive_{80x24,100x30,locked_100x30}`,
`system_{80x24,100x30,save_failure_80x24}`, `system_tutorial_80x24`,
`help_80x24`, `help_glossary_80x24`,
`offline_summary_80x24`, `import_80x24`, `reset_80x24`.

## Checklist

- [ ] Goldens regenerated and reviewed for affected screens
- [ ] Accessibility: 80×24, colour-not-sole-encoding, reduce motion, ASCII/mono
- [ ] Terminal restored on every exit path (normal and panic)
- [ ] No economy state in the TUI; all mutation via `Engine::apply()`
- [ ] Tutorial copy/order lives in `content/tutorial.rs`; replay stays allowed only before the first Descent
- [ ] Keymap and session tests pass
- [ ] `cargo xtask gate` passes

## Commands

```
INSTA_UPDATE=always cargo test -p hadal
cargo test -p hadal
cargo run -p hadal -- --data-dir target/qa-data
cargo run -p hadal -- --demo
cargo run -p hadal -- --ascii --theme mono
cargo xtask gate
```
