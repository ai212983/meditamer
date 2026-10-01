# Ambient sky/sun packer (host tooling)

Host-only implementation of the SD asset side of the Ambient Home
sky/sun work. No firmware dependency, no committed runtime format.

## Run

From the repository root:

```sh
python3 tools/ambient_sky/run_pack.py
```

Requires host Python with Pillow and NumPy (the same host-only
dependencies the mountain-snow experiment uses).

The approved source is
`assets/experiments/ambient-home-sumi-e/sun-contour-two-pass-approved-source.png`.
The resized, non-dithered 600x600 version is stored alongside it. To
regenerate that version and the one-bit sprite with ImageMagick:

```sh
magick -size 600x600 xc:none \
  \( assets/experiments/ambient-home-sumi-e/sun-contour-two-pass-approved-source.png \
  -channel A -threshold 50% +channel -trim +repage -resize 150x150 \) \
  -gravity north -geometry +0+18 -compose over -composite \
  assets/experiments/ambient-home-sumi-e/sun-contour-two-pass-approved-600.png
magick assets/experiments/ambient-home-sumi-e/sun-contour-two-pass-approved-600.png \
  -colorspace Gray -channel R -ordered-dither o8x8,2 +channel \
  -channel A -threshold 50% +channel -type GrayscaleAlpha -depth 8 \
  -define png:color-type=4 assets/sun_05_dithered.png
```

Outputs go to `.scratch/ambient-home-sky-sun/` (gitignored derived
data, never committed):

| Output | Content |
| --- | --- |
| `AMBIENT/SKY.BIN` | Device pack: 64-byte header + sky/sun 1-bit planes |
| `AMBIENT/MANIFEST.txt` | Sources, sun clip bbox, anchor, sizes, SD target |
| `review.png` | Sky + arc guide + sun at five journey fractions |
| `review-midpoint.png` | Sky + arc guide + one sun at the journey midpoint |
| `sun-clip.png` | Clipped sun at 2x on mid-gray with anchor cross |

The review images mirror the firmware's sky/sun composition. The sun path math mirrors
`products/meditamer/.../ambient_view/model.rs` (same control points and
cubic); the midpoint is pinned by unit test.

Exit 0 is clean; exit 2 is a hard validation failure.

## Method

1. Load `assets/sky_01_dithered.png` (600x600 L) and
   `assets/sun_05_dithered.png` (600x600 LA); reject wrong size/mode.
2. Threshold sky gray at 128 to ink bits (the source is pure 1-bit
   dither, so this is lossless).
3. Clip the sun to its nonzero-alpha bbox (currently x=229 y=18 w=142
   h=150, recomputed every run, never hardcoded); ink = gray < 128
   inside alpha > 127; anchor = opaque-mask centroid in crop pixels.
4. Pack MSB-first 1-bit rows and write `SKY.BIN` atomically
   (`SKY.BIN.tmp` + rename) with header, payload CRC32, and manifest.
   The SD target is `/assets/AMBIENT/SKY.BIN`, so
   `hostctl upload --src DIR --dst /assets` keeps working.

## Tests

Synthetic-input unit tests (no asset dependency):

```sh
cd tools/ambient_sky && python3 -m unittest discover -s tests -t .
```

They cover curve endpoints/midpoint/clamping, bit packing order and
row padding, sun clipping/anchor, and pack round-trip with CRC and
magic rejection.
