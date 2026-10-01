> Archived: 2026-09-07. Historical survey dated 2026-09-04, preserved below.
> Its Plex/Ark selection, sizing tables, implementation status, and conclusions
> are not the current plan. Known errors include calling a `404: Not Found`
> asset an LFS pointer and applying a half-em x-height across different faces.
> Methodology: [font legibility reference](../../reference/font-legibility.md).
> Current work: [font legibility plan](../../plans/font-legibility-improvements.md).
> Historical internal links retain their original locations and may be stale.

# Font Generator: Options and Measurements

As of: 2026-09-04

Which toolchain should generate our bitmap fonts. This is the engineering
survey behind that choice: what our own fonts actually contain, what the
candidate toolchains do with them, and measured comparisons against FreeType.

Companion to [`font-legibility.md`](font-legibility.md), which covers *why*
glyphs fall apart at low bit depth and which typefaces to prefer. This doc
covers *what builds them*.

The framing that matters: the output format is glue. Emitting
`lv_font_fmt_txt` tables, or Adafruit-GFX tables, or anything else, is a
few hundred lines of straightforward code. The hard parts are rasterization
fidelity, grid-fitting, kerning, and variable-axis access -- so the choice is
about which engine rasterizes and which shapes, not which converter writes
the file.

## Conclusion

Use **skrifa + harfrust + zeno**, with **freetype-rs as a dev-dependency test
oracle**.

| Role | Crate | Why |
| --- | --- | --- |
| Outlines, hinting, variable axes | `skrifa` 0.46 | Google's FreeType replacement; Chrome 145 dropped FreeType from Blink for it. Bytecode interpreter *and* a port of FreeType's autohinter. Renders at any variation location, so no instancing prestep. |
| GPOS kerning | `harfrust` 0.13 | HarfBuzz port that parses via `read-fonts`, shared with skrifa. Byte-identical kerning to HarfBuzz C. |
| Rasterization | `zeno` 0.3 | Purpose-built glyph-mask rasterizer using the same exact-area coverage approach as FreeType's smooth rasterizer. Measurably closest to FreeType of the options tested. |
| Correctness oracle (tests only) | `freetype-rs` | The reference implementation, available to diff against without putting a native library in `build.rs`. |

`skrifa` and `harfrust` share `read-fonts` 0.43 and `font-types` 0.12, so
there is one font parser in the tree. No native dependency in the build.

## What our own fonts contain

Measured directly from the sfnt table directories in `assets/fonts/`.

| Font | Kerning | Hinting bytecode | Variable |
| --- | --- | --- | --- |
| `IBMPlexSans-Variable.ttf` | GPOS only, no `kern` | `cvt`/`fpgm`/`prep`/`gasp` present | `wght` 100-700, `wdth` 75-100 |
| `ArkPixel-16px-Proportional-Latin.ttf` | GPOS only, no `kern` | none | no |
| `ArkPixel-12px-Proportional-Latin.ttf` | GPOS only, no `kern` | none | no |
| `IBMPlexSans-SemiBold.ttf` | -- | -- | **14 bytes; broken LFS pointer, not a font** |

Three consequences:

1. **Kerning is GPOS-only in everything we ship.** It lives in PairPos
   formats 1 *and* 2 under type-9 extension lookups. Any tool that only reads
   the legacy `kern` table finds nothing at all.
2. **IBM Plex is already hinted and we throw it away.** The `cvt`/`fpgm`/`prep`
   tables are there; the current pipeline ignores them.
3. **Ark Pixel has no bytecode**, so it needs an autohinter or native-size
   integer placement. skrifa's `Engine::AutoFallback` mirrors FreeType's own
   selection rule -- interpreter when `fpgm`/`prep` are non-empty, autohinter
   otherwise -- which lands correctly on both families with no configuration.

## The panels want different things

- **Inkplate Tempera**: eight physical gray levels (three-bit), stored as
  packed Gray4 -- see [`display-refresh.md`](display-refresh.md). This is the
  only target where glyph bit depth and rasterizer coverage accuracy matter.
- **Waveshare RLCD42**: 1 bpp in hardware
  (`boards/waveshare-rlcd42/src/panel.rs:3`). Bit depth is meaningless here;
  hinting is the entire story.

**Prerequisite for any bit-depth work.** The Inkplate flush path still hard
thresholds at `boards/inkplate-tempera/src/panel_blit.rs:84`
(`luminance < 128`). Until that carries gray, a 2- or 3-bpp font cannot be
observed on hardware.

## Current state

Four unrelated glyph-generation paths, none sharing code:

| Path | Output | bpp | Kerning |
| --- | --- | --- | --- |
| [`tools/lvgl_font_compiler`](../../tools/lvgl_font_compiler/src/lib.rs) | `lv_font_fmt_txt` as Rust source | 1, hardcoded | none (`kern_dsc: null`) |
| [`tools/bdf_font_extractor.py`](../../tools/bdf_font_extractor.py) | Adafruit-GFX tables for `targets/medinote-waveshare/src/crash_screen.rs` | 1 | none |
| `tools/flip_digit_compiler` | its own | 1 | none |
| Vendored LVGL Montserrat 14/18/20/24/32 | `lv_font_fmt_txt` C | 4 | yes (upstream) |

`lvgl_font_compiler` uses `fontdue` 0.9, which reads kerning from
`Tag::from_bytes(b"kern")` and nothing else, and has no variable-font axis
support. Against our fonts that means **zero kerning pairs** and **no access
to the semibold weight** that `font-legibility.md` recommends.

## Options considered

| | Rasterize + hint | Shape / kern | Var axes | Build dep |
| --- | --- | --- | --- | --- |
| FreeType + HarfBuzz (bindings) | FreeType | HarfBuzz | yes | native C libs |
| **skrifa + harfrust + zeno** | **skrifa** | **harfrust** | **yes, no instancing** | **none** |
| swash | swash | swash | yes | none |
| lv_font_conv as-is | FreeType (wasm) | opentype.js | no | Node |
| lv_font_conv + patched kerning | FreeType (wasm) | harfbuzzjs | no | Node + fork |
| Hand-drawn BDF | n/a -- drawn | by hand | n/a | otf2bdf / TakWolf tools |

Notes on the ones not chosen:

- **FreeType + HarfBuzz** is the reference implementation and the only option
  with the full hinting-target matrix, synthetic emboldening
  (`FT_Outline_Embolden`) and native BDF/PCF input -- the last of which would
  let one tool cover the crash screen's GohuFont too. Rejected only because it
  puts a native C library in `build.rs`. Retained as a dev-dependency oracle.
- **swash** is a single crate covering shaping, hinting, scaling and
  rasterizing -- by far the least glue. Not evaluated; its author has since
  moved to the Fontations team and its hinting was not compared. Worth
  revisiting if the three-crate assembly proves awkward.
- **lv_font_conv** is not the naive JS converter it appears to be: it bundles
  FreeType compiled to WASM and keeps opentype.js *only* to extract GPOS
  kerning. Rejected on measurement -- see below. Still useful as a
  cross-check and as the reference for how LVGL encodes kerning
  (`kern_classes` vs sorted `kern_pairs`, `kern_scale = 16`). It confirms
  `--bpp {1,2,3,4,8}` and refuses `--bpp 3` outright without compression
  ("LVGL supports \"--bpp 3\" with compression only"). Its own help warns that
  `--autohint-strong` "will break kerning".
- **Hand-drawn BDF** remains the highest-fidelity answer at 12-16px, and is
  the route `crash_screen.rs` arrived at over six hardware rounds before
  settling on GohuFont. Worth testing at the small end independently; it does
  not compete with a rasterizer pipeline for 18-32px, and BDF carries no
  kerning.

## Measurements

All against `assets/fonts/IBMPlexSans-Variable.ttf` instanced to a static
`wght=400` with `fontTools.varLib.instancer`, printable ASCII, unless noted.

### Kerning coverage

Non-zero pairs over printable ASCII at 18px:

| Tool | pairs | >= 0.5px | >= 1.0px |
| --- | --- | --- | --- |
| HarfBuzz C (via `uharfbuzz`) | 1232 | 309 | 92 |
| `rustybuzz` 0.20 | 1232 | 309 | 92 |
| `harfrust` 0.13 | 1232 | 309 | 92 |
| `lv_font_conv` | 20 | -- | -- |
| `fontdue` 0.9 | 0 | -- | -- |

The three HarfBuzz-lineage implementations agree exactly. `lv_font_conv`
recovers about 2% of the font's kerning, because opentype.js does not follow
the class-based PairPos format 2 subtables under type-9 extensions where most
of IBM Plex's kerning lives. Pointed at the *variable* font it emits
`kern_dsc = NULL` and silently renders the default instance; pre-instancing
restores a `kern_pairs` table, but still only 20 pairs.

Ark Pixel, for reference: 130 pairs at 16px, 150 at 12px, all a whole pixel
(it is drawn on the pixel grid).

Size cost of kerning is small: 21855 vs 21016 bytes of generated C for
18px/1bpp/full-ASCII with and without.

### Hinting

Per-glyph `(height, top)` at 18px, unhinted-AA-then-threshold (the current
pipeline) versus FreeType `TARGET_MONO`:

| glyph | unhinted | hinted |
| --- | --- | --- |
| `e` | (11, 10) | (9, 9) |
| `a` | (11, 10) | (9, 9) |
| `m` | (10, 10) | (9, 9) |
| `d` | (15, 14) | (13, 13) |
| `t` | (12, 12) | (12, 12) |
| `M` | (13, 13) | (13, 13) |

Unhinted, the round letters `e` and `a` overshoot `m`'s x-height by a row, so
the x-height is not a single line; `M` renders with one 1px stem and one 2px
stem. Hinted, both are corrected. This is a larger legibility effect than
kerning at our sizes.

`skrifa` with `Target::Mono` reproduced FreeType's hinted metrics on 7/7
sampled glyphs, and its hinted bounding box matched FreeType's on **every**
glyph at every size tested in the grayscale comparison below -- which is what
makes that comparison a rasterizer-only measurement.

### Rasterizer fidelity, grayscale

Identical skrifa-hinted outlines (`Target::Smooth`/`Normal`) through three
rasterizers, compared against FreeType `FT_LOAD_RENDER`:

| size | rasterizer | mean abs diff /255 | max | differ after 3-bit quant | differ after 1-bit |
| --- | --- | --- | --- | --- | --- |
| 12px | **zeno** | **2.01** | 20 | **10.86%** | 0.84% |
| 12px | ab_glyph | 4.76 | 28 | 18.38% | 0.84% |
| 12px | tiny-skia | 7.93 | 33 | 18.11% | 1.95% |
| 18px | **zeno** | **1.89** | 17 | **4.22%** | 0.62% |
| 18px | ab_glyph | 3.55 | 31 | 8.20% | 1.24% |
| 18px | tiny-skia | 5.26 | 25 | 13.79% | 2.48% |
| 32px | **zeno** | **1.22** | 22 | **3.56%** | 0.21% |
| 32px | ab_glyph | 2.25 | 39 | 6.38% | 0.66% |
| 32px | tiny-skia | 3.66 | 37 | 8.95% | 1.28% |

zeno is closest at every size on every metric, roughly half ab_glyph's error
and a third of tiny-skia's.

Inspecting the difference maps, tiny-skia's error is **structured**: solid
vertical bands along stem edges on `M` and `R`. That is the worst shape of
error available to us, since stem rendering is what legibility depends on.
ab_glyph's error is speckled around curves (`a`, `g`, `s`, `8`) with no stem
bias.

### Rasterizer fidelity, 1-bit

Against FreeType `FT_RENDER_MODE_MONO` (a true mono scan conversion with
dropout control), aligned on the glyph origin, over 94 printable-ASCII glyphs:

| size | arm | differing px | % of reference ink | glyphs identical |
| --- | --- | --- | --- | --- |
| 12px | unhinted + threshold (today) | 232 | 16.3% | 22/94 |
| 12px | `Target::Mono` + zeno | 90 | 6.3% | 41/94 |
| 12px | `Target::Mono` + tiny-skia | 113 | 7.9% | 46/94 |
| 12px | `Target::Mono` + ab_glyph | 92 | 6.5% | 41/94 |
| 16px | unhinted + threshold (today) | 219 | 8.5% | 20/94 |
| 16px | `Target::Mono` + zeno | 100 | 3.9% | 51/94 |
| 16px | `Target::Mono` + tiny-skia | 185 | 7.2% | 32/94 |
| 16px | `Target::Mono` + ab_glyph | 118 | 4.6% | 45/94 |
| 18px | unhinted + threshold (today) | 458 | 15.7% | 11/94 |
| 18px | `Target::Mono` + zeno | 134 | 4.6% | 36/94 |
| 18px | `Target::Mono` + tiny-skia | 380 | 13.0% | 20/94 |
| 18px | `Target::Mono` + ab_glyph | 167 | 5.7% | 36/94 |
| 24px | unhinted + threshold (today) | 608 | 10.7% | 9/94 |
| 24px | `Target::Mono` + zeno | 146 | 2.6% | 37/94 |
| 24px | `Target::Mono` + tiny-skia | 220 | 3.9% | 31/94 |
| 24px | `Target::Mono` + ab_glyph | 154 | 2.7% | 38/94 |

Hinting cuts the error by half to three-quarters at every size. Among
rasterizers the spread is much smaller than in grayscale -- expected, since
thresholding discards most of the coverage difference -- though tiny-skia's
stem bias still shows at 16 and 18px.

**On dropout control.** No arm reaches bit-identity with FreeType; the best is
51/94 glyphs at 16px. The residual is FreeType's dropout control plus the
difference between AA-then-threshold and true mono scan conversion. It costs a
few percent of ink, concentrated at small sizes. This is the one capability
the chosen stack does not have, and it is worth watching at the 12px caption
size -- it fails loudly (a glyph loses a stroke), so a full-range render diff
catches it.

## Viewing geometry

The quality metrics below are parameterised by physical viewing conditions, so
these are inputs, not trivia. Recorded here because they are not in
`docs/reference/hardware/`; they belong in a board doc if one is ever written.

| Panel | Pixels | Diagonal | DPI | Pixel pitch | Panel size |
| --- | --- | --- | --- | --- | --- |
| Inkplate Tempera | 600 x 600 | 3.8 in | 223.3 | 0.1137 mm | 68.2 x 68.2 mm |
| Waveshare RLCD42 | 300 x 400 | 4.2 in | 119.0 | 0.2134 mm | 64.0 x 85.3 mm |

### Two viewing modes

The devices are used two ways, and type must be sized for each:

| Mode | Use | Distance |
| --- | --- | --- |
| **Ambient** | occasional glance at the screen | 80 cm - 1 m |
| **Reading** | device being operated | 40 - 50 cm |

There is also a deliberate third tier: sizes chosen *below* the readable range
because the content matters more than its readability -- 14px on the Waveshare,
for example. Those are intentional and are treated as a declared deviation, not
a defect. See "Deviation tier" below.

### The fluent range

Legge & Bigelow (2011) define the **fluent range** -- the span of angular
x-height over which text reads at maximum speed -- as roughly **0.2 deg to
2 deg**. They measured newspapers at 0.23 deg and hardback books at 0.24 deg.

IBM Plex Sans x-height is exactly em/2 at the sizes we compile (measured from
our own hinted renders: 16px em gives 8px x-height, 24px gives 12px). Ark Pixel
differs and needs its own measurement before this table is applied to Medinote.

**em px needed for x-height to land in the fluent range:**

| Panel | Mode | Distance | fluent min 0.20 deg | book 0.24 deg | generous 0.30 deg |
| --- | --- | --- | --- | --- | --- |
| Inkplate | ambient | 80 cm | 49 px | 59 px | 74 px |
| Inkplate | ambient | 100 cm | 61 px | 74 px | 92 px |
| Inkplate | reading | 40 cm | 25 px | 29 px | 37 px |
| Inkplate | reading | 50 cm | 31 px | 37 px | 46 px |
| Waveshare | ambient | 80 cm | 26 px | 31 px | 39 px |
| Waveshare | ambient | 100 cm | 33 px | 39 px | 49 px |
| Waveshare | reading | 40 cm | 13 px | 16 px | 20 px |
| Waveshare | reading | 50 cm | 16 px | 20 px | 25 px |

### Where the shipping sizes land

Each size judged at the mode it belongs to, not at a single distance:

| Panel | Mode | Dist | em | angular | Verdict | What |
| --- | --- | --- | --- | --- | --- | --- |
| Inkplate | ambient | 100 cm | 128 | 0.417 deg | fluent | Ambient clock |
| Inkplate | ambient | 100 cm | 64 | 0.209 deg | fluent | Ambient environment |
| Inkplate | reading | 40 cm | 32 | 0.261 deg | fluent | Montserrat 32, carousel arrows |
| Inkplate | reading | 40 cm | 24 | 0.196 deg | marginal | Montserrat 24, titles |
| Inkplate | reading | 50 cm | 24 | 0.156 deg | **below fluent** | Montserrat 24, titles |
| Inkplate | reading | 40 cm | 18 | 0.147 deg | **below fluent** | Montserrat 18, body |
| Inkplate | reading | 40 cm | 14 | 0.114 deg | **below fluent** | Montserrat 14, catalogue/overlay |
| Waveshare | reading | 40 cm | 16 | 0.244 deg | fluent (book) | Ark Pixel 16, Home heading |
| Waveshare | reading | 50 cm | 16 | 0.196 deg | marginal | Ark Pixel 16, Home heading |
| Waveshare | reading | 40 cm | 14 | 0.214 deg | fluent | GohuFont 14, crash screen |
| Waveshare | reading | 40 cm | 12 | 0.183 deg | marginal | Ark Pixel 12, Home caption |
| Waveshare | reading | 50 cm | 12 | 0.147 deg | **below fluent** | Ark Pixel 12, Home caption |

**Ambient is correctly sized** on both counts -- 128px and 64px stay fluent out
to a metre. **Waveshare reading is close to right**: 16px is almost exactly
book-equivalent at 40 cm, and even the 14px crash screen sits inside the range.

**Inkplate reading mode is the outlier.** 18px and 14px are below fluent at any
operating distance and 24px holds only at 40 cm; body text there wants roughly
25px at 40 cm and 31px at 50 cm, about double what ships today. These look like
inherited Montserrat defaults rather than deliberate choices, so they should
either be resized or reclassified as declared deviations.

### Deviation tier

A size deliberately below 0.2 deg is a product decision, not a bug -- but it
changes the rendering policy, because there is no legibility margin left:

- topology (no severed strokes, no filled counters) becomes a hard gate;
- stem weight should be pushed to the top of the usable range;
- edge-quality work is wasted effort at that scale.

Deviations should carry a recorded reason and the angular size they land at, so
the build reports them rather than warning about them.

### What this implies for rendering

**Sizing by angle collapses the two modes into one rendering problem.** Ambient
and reading target the same angular x-height at different distances, so they
land at the same ratio of visual blur to letter size -- Watson's sigma =
2.33 arcmin works out to roughly 0.15-0.20 of the x-height in both. The only
tier that behaves differently is the deviation tier.

At reading distance one pixel subtends **0.98 arcmin** on the Inkplate and
**1.83 arcmin** on the Waveshare; at 80 cm, **0.49** and **0.92** arcmin, against
roughly 1 arcmin resolution for 20/20 vision. So Inkplate pixels are at or below
the resolving limit in ambient mode and right at it in reading mode, while
Waveshare pixels stay individually resolvable at reading distance.

Provisional consequence, to be confirmed with a CSF model rather than this rule
of thumb: edge fidelity earns least on the Inkplate in ambient mode, where
ragged edges are optically averaged away before reaching the retina, and most on
the Waveshare in reading mode. **Stroke weight and stroke integrity matter
everywhere** -- a stem that thins or drops out changes the letter at any scale.

## Inkplate panel facts established on hardware

Determined while building the specimen gallery (`targets/meditamer-inkplate/src/bin/gallery_probe.rs`);
several are undocumented in the datasheet and each one silently corrupts a
frame rather than raising an error.

- **Gray4 polarity: a higher value is lighter.** The white ground stores as
  level 7 (nibble 14) and full black text as 0. Confirmed by showing both
  polarities on the glass. Getting this backwards yields a photographic
  negative, not an error.
- **Gray4 packing**: two pixels per byte, **high nibble is the even (left) x**,
  300 bytes per row, and the eight physical levels occupy the **even** values
  of the 4-bit field (`level * 2`).
- **Binary packing is LSB-first** within the byte (`1 << (x % 8)`), 75 bytes
  per row -- the opposite of the Waveshare convention, so the two must not
  share packing code.
- **`InkplateHal::init_core()` is required before any display call.** The
  TPS65186 PMIC at 0x48 does not ACK until it has configured the expander pins
  and pulsed WAKEUP; without it every refresh fails `AcknowledgeCheckFailed`.
- **The panel's axes sit 90 degrees counter-clockwise** from a natural
  row-major raster, so frames are authored normally and rotated clockwise on
  the way into the panel buffer.
- **Touch reports in its own 1152x1152 space**, not panel pixels; samples need
  scaling before any pixel-denominated threshold.
- **The product app runs from `ota_0`** (0x380000, 3.67 MB). `factory`
  (0x60000) holds only the updater/recovery image.

Two host-side operational notes, both of which cost time:

- Driving DTR/RTS to reset the board leaves the serial command console
  unresponsive. Open the port with both lines deasserted and use
  `espflash reset` to restart the device.
- `STATE SET upload=on` is a no-op when upload is already on, so it does not
  re-trigger the Wi-Fi resume. Toggle `off` then `on`.

## Quality metrics

Measuring "closer to FreeType" cannot show an improvement *over* FreeType --
its best possible score is "identical". These are absolute metrics from the
vision-science literature, so any renderer including FreeType can score badly.

**1. Visual perimetric complexity.** Pelli, Burns, Farell & Moore-Page (2006,
*Vision Research* 46(28):4646-4674) found letter-identification efficiency to
be inversely proportional to perimetric complexity -- perimeter squared over
ink area -- across many scripts, sizes, contrasts and ages, "and nearly
independent of everything else". Watson (2012, NASA / *The Mathematica
Journal*) showed the naive definition breaks on a binary pixel grid, gave a
corrected algorithm, and defined *visual* perimetric complexity: blur by the
visual system's response first, then measure. Computable directly from our
bitmaps, against the ideal outline as reference.

*Caveat:* Pelli's result concerns complexity differences between typefaces.
Applying it to rendering differences within one typeface is an extrapolation
that Watson's blur-first variant makes defensible but does not replicate.

**2. S-CIELAB.** Zhang & Wandell (1997, *JSID*) filter both images through a
contrast-sensitivity model before differencing; Zhang, Silverstein, Farrell &
Wandell applied it to halftone texture visibility, which is our quantisation
problem. Our panels are grayscale so it reduces to a CSF-filtered luminance
difference. Reference implementation: `wandell/SCIELAB-1996`. Blocked behind
`panel_blit.rs` carrying gray.

**3. Critical print size.** Legge & Bigelow, applied above.

Applicability caveat: metric 1 needs the modelled blur to be small relative to
the letter -- sigma/x-height <= ~0.20 -- or it degenerates and every glyph
scores alike. Judged at each size's own viewing mode that holds nearly
everywhere; judged at the wrong distance it does not. An earlier revision of
this document measured reading-mode sizes at ambient distance and wrongly
concluded the metric was inapplicable.

**4. Stroke and counter topology.** A severed stroke or filled counter changes
which letter it is -- categorical, not perceptual. Keep as a pass/fail gate,
not a score.

A symmetry metric was tried and discarded: no literature backing, and it
measured bounding-box centring rather than asymmetry.

### Current standing

Scored at 16 and 24px, 1-bit, over printable ASCII
(`tools/font_probe/scorecard.py`):

| | FreeType | ours (skrifa + zeno) |
| --- | --- | --- |
| topology errors @16px | 1 | **8** |
| topology errors @24px | 0 | **1** |
| x-height / cap-height row consistency | 1 / 1 | 1 / 1 |
| odd-width stem runs @16px | 13 | 13 |
| confusability (higher better) | 0.67 | 0.66 |

At 16px our renders split `b`, `h`, `m`, `n` and `p` into two disconnected ink
components -- the stem detaches from the arch or bowl. The failure is
systematic across exactly the stem-plus-join glyphs, which points at the
threshold rule (we fill at coverage >= 50%; FreeType's mono rasterizer fills
on pixel-centre coverage plus dropout control) rather than at zeno. **Unconfirmed
-- diagnose before building on it.**

## Decisions still open

1. **Advance and kerning rounding.** `Target::Mono` grid-fits advances to
   integers; harfrust returns kerning in scaled font units; LVGL stores
   kerning in 1/16px (`kern_scale = 16`). If advances round to integers while
   kerning stays fractional, spacing drifts across a run. Recommend integer
   advances *and* integer-rounded kerning -- what a bitmap font wants --
   accepting that roughly two-thirds of Plex's 1232 pairs round away and the
   309 that survive are the ones that were visible anyway. This is the same
   hazard behind lv_font_conv's `--autohint-strong` warning.
2. **Inkplate reading-mode sizes.** Montserrat 14/18 are below the fluent
   range at any operating distance and 24 holds only at 40 cm. Either resize
   (roughly double) or reclassify as declared deviations. A larger effect than
   any rendering change, and a UI decision rather than a font-pipeline one.
   Waveshare reading and both ambient faces need no change.
3. **Bit depth per target.** Inkplate wants 2 or 4 bpp once
   `panel_blit.rs` carries gray; LVGL's uncompressed path supports 1/2/4/8 and
   bpp 3 only under compression, so 4 bpp quantized to the panel's 8 levels is
   the sensible choice over 3. Waveshare stays at 1 bpp permanently.
4. **Panel-response quantization.** Nothing off-the-shelf quantizes coverage
   against a *measured* panel response curve; every tool surveyed quantizes
   linearly. E-paper gray levels are not perceptually evenly spaced, so this
   remains ours to write regardless of toolchain.
5. **Whether to unify the BDF path.** The chosen stack cannot read BDF, so
   `bdf_font_extractor.py` and the crash screen's GohuFont stay separate.
   Only FreeType would unify them.
6. **Broken asset.** `assets/fonts/IBMPlexSans-SemiBold.ttf` is a 14-byte LFS
   pointer. With variable-axis support it is redundant -- `wght=600` off the
   variable face gives the same thing -- so the likely fix is deleting it.

## Reproducing

The harness lives in `tools/font_probe/` -- untracked, not part of any build
(`tools` is excluded from the workspace). See its `README.md` for the run
order. What each piece does:

- `scorecard.py` -- the absolute quality metrics (topology, x-height and
  cap-height row consistency, stem-width uniformity, confusability).
- `geometry.py` -- DPI, pixel pitch and angular x-height against the fluent
  range.
- `scale.py` -- the two-mode angular scale and the classification of shipping
  sizes; the source of the tables in "Viewing geometry".
- `joins.py` -- fill-rule comparison for the stem/arch join failure.
- `applicability.py` -- whether the complexity metric discriminates at a given
  size and viewing mode.
- `topo_img.py`, `onebit_img.py`, `compare.py` -- rendered comparisons.
- `dump.rs`, `dump1.rs`, `hr.rs` -- the skrifa/zeno/harfrust side.

Notes for rebuilding any of it:

- **Kerning**: shape each ASCII pair and subtract the two solo advances;
  compare `uharfbuzz`, `rustybuzz` and `harfrust` on the same file.
- **Hinting and rasterizers**: draw glyphs through `skrifa`'s `OutlinePen`
  into a shared command list, fill the same list with each rasterizer on an
  identical canvas, and diff against `freetype-py` renders aligned on the
  glyph origin (not the bitmap box -- FreeType trims empty rows and columns
  from mono bitmaps, so bounding boxes legitimately differ).
- **Instancing**: `fontTools.varLib.instancer.instantiateVariableFont`.

Versions measured: skrifa 0.46.2, harfrust 0.13.3, rustybuzz 0.20.1,
zeno 0.3.3, ab_glyph_rasterizer 0.1.10, tiny-skia 0.11, read-fonts 0.43.3,
FreeType 2.14.3, HarfBuzz 14.4.0.
