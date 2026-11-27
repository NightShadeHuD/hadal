# HADAL Art Style

Short, enforceable rules for the terminal art. The medium is a character grid,
not a framebuffer: structure first, pixels only where the grid is too coarse.

## Shade ramp

Mid-tones come from the CP437 shade ramp, not from extra colours:

| Level | Unicode | ASCII |
|---|---|---|
| light  | `░` | `:` |
| medium | `▒` | `+` |
| dense  | `▓` | `#` |

Shade characters are dim: the ramp carries tone, colour carries state. Never
use shade alone to mean "on" or "owned"; pair it with shape (see below).

## Half-block pixels

`▀` / `▄` double the vertical resolution inside one cell; `█` is two equal
halves. The Canvas renderer (`render/canvas.rs`) maps a pixel grid of
`width × 2·height` onto those glyphs. Use it for compact silhouettes and rings
whose shape matters at cell scale; use text cells for anything that should read
as a chart, list, or band.

## Per-cell priority

Later passes overwrite earlier ones, in this order:

1. Background strata (shade bands, keel, rail)
2. Structure (box-drawing links, ticks, traces)
3. Marker (the subject: K-9, station, current node)
4. Pulse (rings, pulses — clipped, never obscuring the marker centre)
5. Labels (depth, counts, legend — always last, cleared to bg first)

Never let a later layer silently erase a label: clear the label row to the
background, then write it.

## Shape + colour

Colour is never the sole encoding. Every state differs in shape or glyph as
well: owned `██` vs empty `░░`, recorded `██` vs locked `··`, current `▓▓` plus
a ring. This keeps the art legible in monochrome, ASCII, and reduced-colour
palettes.

## Motion

- No hardware blink and no wall clock. Animation is a pure function of
  `anim.frame_ms` (and the reduce-motion flag), so a frame is reproducible.
- Reduce motion freezes every animated element at a static pose (for example a
  ping ring at half radius).
- Rings are clipped to their panel and never larger than the structure that
  carries them.

## Minimum size

The layout floor is 80×24. At the floor, art must still read: no element wider
than its column budget, no labels overpainted by pixels, no partial panels.
Panels that need more room (hull schematic, signal map) appear only at wider
sizes and are absent — not squeezed — below their threshold.

## Palette ramps

Each theme defines semantic colours (`bg`, `panel`, `text`, `dim`, `accent`,
`amber`, `alert`). Build ramps from those roles, not ad-hoc RGB: strata and
chrome use `dim`, the subject uses `accent`, the current/newest state uses
`amber`, alerts use `alert`. Colour depth (truecolor / 256 / 16 / none) is
applied once at blit time; art must survive the coarsest quantisation.

## Techniques borrowed from ANSI scene art

- Shade dithering for mid-tones instead of half-tone blocks.
- `▀` / `▄` to double vertical resolution where a shape needs it.
- Box-drawing chrome only at panel edges and link paths, never as filler.

These are terminal techniques; HADAL has no GPU, sprite atlas, or framebuffer
pipeline and does not claim them.
