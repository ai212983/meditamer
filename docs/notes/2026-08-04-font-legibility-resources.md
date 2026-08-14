# Font legibility on 1-bit / 3-bit displays — resource list

Curated external references for improving text legibility on low-bit-depth displays,
annotated for compatibility with our LVGL font pipeline.

## Our current state

- `LV_COLOR_DEPTH 8` (`config/lvgl/lv_conf.h:6`) — LVGL renders 8-bit gray.
- `src/firmware/ui/lvgl/dither.rs` quantizes down to the panel's 3-bit (8 gray levels).
- Built-in fonts in use: Montserrat 14/18/20/24/32.
- `assets/fonts/IBMPlexSans-Variable.ttf` is vendored but unused.
  (`assets/fonts/IBMPlexSans-SemiBold.ttf` is 14 bytes — an LFS pointer or a broken
  download, not a usable font.)

Two regimes, wanting opposite fonts:

| Regime | Recommended | Rationale |
| --- | --- | --- |
| 3-bit grayscale | outline font at `--bpp 4`, semibold weight | anti-aliasing survives quantization to 8 levels |
| 1-bit mono / fast refresh | bitmap font at `--bpp 1`, native pixel size | no AA available; only a pixel-grid-drawn font stays sharp |

The trap: a 4bpp outline font thresholded to 1-bit is the worst of both.

## 1. Rendering theory — why glyphs fall apart at low bit depth

- [FreeType: On Slight Hinting, Proper Text Rendering, Stem Darkening and LCD Filters](https://freetype.org/freetype2/docs/hinting/text-rendering-general.html)
  — the best single write-up on this problem. Linear-space blending plus gamma
  correction *thins* glyphs; **stem darkening** (emboldening proportional to pixel
  size) is the counter-measure. With only 8 gray levels, thin stems land in the wrong
  quantization bucket and disappear.
- [FreeType driver properties reference](http://freetype.org/freetype2/docs/reference/ft2-properties.html)
  — the `no-stem-darkening` / `darkening-parameters` knobs, relevant if we ever render
  glyphs offline through FreeType before feeding LVGL.
- [LVGL font docs — bpp semantics](https://lvgl.io/docs/open/9.4/details/main-modules/font.html)
  — 1/2/4/8 bpp. 1bpp has no AA and "will not look good in most cases"; 4bpp is the
  size/quality sweet spot. Because our dither pass already quantizes to 3-bit, 4bpp is
  not wasted — the AA coverage survives as real gray levels.

## 2. Outline typefaces designed for legibility

These feed `lv_font_conv` directly (TTF/WOFF).

- [Atkinson Hyperlegible](https://www.brailleinstitute.org/freefont/)
  ([source repo](https://github.com/googlefonts/atkinson-hyperlegible)) — Braille
  Institute, SIL OFL. Built around *letterform distinction* (unambiguous I/l/1, O/0),
  which is exactly the failure mode when AA collapses to 8 levels. Best single
  candidate for small UI text.
- [Luciole](https://www.luciole-vision.com/) — CC-BY, designed with low-vision readers
  at CNRS / Orange Labs. Same rationale, more generous spacing.
- Georgia / Bookerly / Literata / Source Serif / Noto Serif — the e-reader canon; large
  x-height, thickened strokes. See [comparison writeup](https://simonh.uk/2025/11/02/best-fonts-for-ereading-part-1/)
  and [ebook-fonts](https://github.com/nicoverbruggen/ebook-fonts) (fonts pre-modified
  with tuned line height for e-ink devices — useful as a metrics reference).
- General e-ink guidance: prefer a **medium/semibold** weight over regular. Hairline
  and ultra-bold both degrade on e-paper.

## 3. Bitmap / pixel fonts — the 1-bit answer

At 1bpp there is no anti-aliasing to help, so the only path to sharpness is a font
*drawn* on the pixel grid. Most ship BDF; see §4 for the conversion hop.

**Project constraint: no monospace.** The UI wants a proportional face, which rules out
the usual bitmap-font recommendations (Terminus, Spleen, Cozette are all monospaced).
That leaves a thin field:

- [Pixel Operator](https://notabug.org/HarvettFox96/ttf-pixeloperator) — CC0, genuinely
  **proportional** (plus a bold; the separate Mono variant is not for us). Distributed
  as TTF, so it feeds `lv_font_conv --bpp 1` directly with no mkttf hop. Designed at
  8px/16px cells and crisp at exactly those sizes.
- **X11 Adobe Helvetica BDF** (`helvR12/14/18/24` and bold variants) — proportional,
  hand-tuned, MIT/X11 licence, redistributable. This is the Helvetica/Arial lineage the
  e-ink legibility study in §5 rated most legible on e-paper. Conveniently packaged in
  [toitlang/pkg-font-x11-adobe](https://github.com/toitlang/pkg-font-x11-adobe); needs
  the BDF → TTF hop.
- [pixelfonts.org](https://pixelfonts.org/) — browse/preview BDF and PCF fonts at actual
  pixel size before committing to a conversion.
- [Ark Pixel](https://github.com/TakWolf/ark-pixel-font) /
  [Fusion Pixel](https://github.com/TakWolf/fusion-pixel-font) — OFL, 8/10/12px,
  pan-CJK. Only relevant if we ever need non-Latin.
- [teryror/pixel-fonts](https://github.com/teryror/pixel-fonts) — small collection
  explicitly targeting low resolutions and limited-color displays.
- [Bitmap fonts make computers feel like computers again](https://korigamik.dev/blog/bitmap_fonts/)
  — orientation piece on the tradeoffs (pixel-perfect at exactly one size, only that size).

Caveat that pushes against this whole route: good proportional pixel fonts top out
around 16px, while our UI runs 18/20/24/32. Scaling a 16px pixel font up to 24px
reintroduces the raggedness we are trying to remove.

## 4. Tooling

- [lv_font_conv](https://github.com/lvgl/lv_font_conv) — TTF/WOFF in, LVGL C array out.
  Flags that matter:
  - `--bpp {1,2,4}`, `--size`, `--range` / `--symbols` (subset to keep flash down)
  - `--no-compress` — compressed fonts render ~30% slower; on our refresh budget we
    likely want compression off
  - ignore `--lcd` / `--lcd-v`: RGB subpixel rendering is meaningless on a grayscale
    e-paper panel
- [mkttf](https://github.com/Tblue/mkttf) — the missing link for BDF. `lv_font_conv`
  will not read BDF/PCF; mkttf uses potrace to trace outlines *and* embeds the original
  bitmap, producing a TTF that `lv_font_conv` accepts.
  [LVGL's BDF doc](https://lvgl.io/docs/open/main-modules/fonts/bdf_fonts) documents the
  two-step recipe.
- [Online font converter](https://lvgl.io/tools/fontconverter) — fine for one-off
  experiments; for this repo, prefer the CLI in a build script so fonts are reproducible.

Worked 1bpp invocation, from LVGL's docs:

```bash
lv_font_conv --bpp 1 --size 12 --no-compress --font TerminusMedium-001.000.ttf --range 0x20-0x7e --format lvgl -o terminus_1bpp_12px.c
```

## 5. Research / evidence

- [Developing a Typeface for Low Resolution E-Ink Displays](https://www.researchgate.net/publication/324671274_Developing_a_Typeface_for_Low_Resolution_E-Ink_Displays)
  — on-point academic study; found Verdana and Arial most legible on e-ink, Times New
  Roman and Franklin least.
- [EPFL/LSP typography publications (Hersch et al.)](https://lspwww.epfl.ch/publications/typography/lptgf.html)
  — grayscale-vs-bilevel work: perceptually-tuned grayscale characters outperform
  bilevel for search tasks, and rate superior at 8–10pt. The strongest argument for
  keeping 4bpp plus the dither path rather than dropping to 1-bit mono for text.

## Current UI state (2026-08-04)

Both pages use LVGL's built-in **Montserrat** only, at four sizes:

| Screen | Element | Font |
| --- | --- | --- |
| Home | "Meditamer" title | montserrat_24 (`home.rs:31`) |
| Home | "Ready" status | montserrat_18 (`home.rs:42`) |
| Home | arrows hint | montserrat_18 (`home.rs:53`) |
| Home | "TOP TEST" button | montserrat_18 (`home.rs:93`) |
| Gesture test | title | montserrat_24 (`gesture_test.rs:38`) |
| Gesture test | instructions | montserrat_18 (`gesture_test.rs:53`) |
| Gesture test | result panel | montserrat_20 (`gesture_test.rs:76`) |
| Both (carousel) | `<` / `>` buttons | montserrat_32 (`carousel.rs:73`) |
| Both (carousel) | page indicator | montserrat_18 (`carousel.rs:30`) |

`LV_FONT_MONTSERRAT_14` is enabled in `lv_conf.h:46` but never referenced — it is
LVGL's implicit `LV_FONT_DEFAULT` (the macro is not set), so it costs flash while
serving only as a fallback no widget currently hits.

All built-in Montserrat fonts are generated at **4bpp** — from the vendored
`lv_font_montserrat_18.c` header: `--no-compress --no-prefilter --bpp 4 --size 18`.

**The mismatch:** LVGL anti-aliases each glyph into 16 coverage levels, then
`dither.rs:77` discards all of it with a hard 50% threshold
(`fn dithered_black(luminance: u8) -> bool { luminance < 128 }`) — despite the module
name, there is no dithering. We pay 4bpp of flash per glyph and get 1bpp output.
Montserrat compounds it: a geometric sans with uniform thin stems, so at 18px a
vertical stem often sits near half-coverage and thresholds inconsistently across the
same glyph.

## Suggested next steps

In order, cheapest first. Steps 1 and 2 are independent.

1. **Tune the threshold** in `dither.rs:77` — one line, reversible. Raising the cut to
   ~150–170 thickens every glyph uniformly (ad-hoc stem darkening, cf. §1). Establishes
   whether legibility is threshold-bound before any asset work.
2. **Replace Montserrat with a sturdier proportional outline face** at semibold —
   Atkinson Hyperlegible, or the already-vendored IBM Plex Sans. Both carry more stem
   weight and more distinct letterforms than Montserrat.
3. **Pixel font** (Pixel Operator at 16px) only if 1 and 2 both fall short, and only for
   the smaller text sizes.

Real dithering (Bayer / error diffusion) in `dithered_black` is a fourth option, but for
small text it tends to read as noise — better suited to images than glyphs.
