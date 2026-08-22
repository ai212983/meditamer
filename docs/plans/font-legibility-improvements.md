# Font Legibility Improvements

- Status: Proposed
- Last-reviewed: 2026-08-18
- Background: [`reference/font-legibility.md`](../reference/font-legibility.md) —
  resource list, rendering theory, and the current-state audit these steps are based on.

## Suggested next steps

In order, cheapest first. Steps 1 and 2 are independent.

1. **Tune the threshold** in `dither.rs:77` — one line, reversible. Raising the cut to
   ~150–170 thickens every glyph uniformly (ad-hoc stem darkening). Establishes
   whether legibility is threshold-bound before any asset work.
2. **Replace Montserrat with a sturdier proportional outline face** at semibold —
   Atkinson Hyperlegible, or the already-vendored IBM Plex Sans. Both carry more stem
   weight and more distinct letterforms than Montserrat.
3. **Pixel font** (Pixel Operator at 16px) only if 1 and 2 both fall short, and only for
   the smaller text sizes.

Real dithering (Bayer / error diffusion) in `dithered_black` is a fourth option, but for
small text it tends to read as noise — better suited to images than glyphs.
