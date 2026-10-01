# Mountain-snow progression experiment (host tooling)

Host artwork and sequence tooling for the Ambient Home mountain.
The preview itself changes no device behaviour. Requires host Python with Pillow and NumPy (the same host-only
NumPy the blue-noise quality check already uses).

## Run

From the repository root:

```sh
python3 tools/mountain_snow/run_experiment.py
```

Outputs go to `.scratch/ambient-home-mountain-snow/` (gitignored derived
data, never committed):

| Output | Content |
| --- | --- |
| `frames_gray/snow-000..100.png` | 101 grayscale previews |
| `frames_1bit/snow-000..100.png` | 101 stable one-bit previews (repo blue-noise map) |
| `review-sheet.png` | 0/10/25/50/75/90/100% contact sheet, gray + one-bit |
| `edge-sheet.png` | endpoint edge crops at 2x over white and light gray |
| `viewer.html` | slider + play + blink viewer over all 101 steps |
| `diagnostics.json` | area, components, fragment sizes, bounds, reverts |
| `checks.json` | Phase-1 endpoint validation rows |

`--skip-frames` reuses existing frames and rebuilds only sheets and
diagnostics. Exit 0 is clean; exit 2 is a hard validation failure.

`build_pack.py` writes the v1 device pack the firmware composer
consumes (`.scratch/mountain-snow-pack/`, gitignored): gray endpoint
rows, barrier rows, packed eligibility, and noise rows for the measured
mountain band, with manifest and a parity report against the host
one-bit frames. Pack layout mirrors the sibling sky/sun pack family
(header, little-endian fields, IEEE CRC32, MSB-first planes); the
decoder is `platform/ui/visuals/mountain-snow/src/pack.rs`.

## Approved host progression method

1. Read the three 600x600 PNGs; validate size, RGBA mode, transparent
   sky, grayscale guide, and outer-silhouette registration (no snow
   paint outside the rock silhouette; rock-solid holes in snow reported
   with group sizes).
2. Eligibility = union of endpoint alpha coverage (a pixel mask, never a
   filled polygon). Guide read as straight unmultiplied gray: summit
   seeds are transparent black (arrive first), late foothill wash is
   light gray (arrives last).
3. Seeds = dark-guide basins ≥ 50px, inventoried with centroid and bbox.
   Detached faint wash specks unreachable from any basin get an orphan
   seed at their darkest guide pixel and are reported, never silently
   dropped.
4. Minimax priority flood (256-bucket queue over 0..255 guide values):
   each pixel arrives only through a reached neighbour. Barrier field
   histogram-equalized to rank 0..65535; ties broken by the repo
   `blue-noise-600x600.bin` map at fixed origin (SHA-pinned), then flood
   sequence. Threshold `s` takes the first `round(s·N/100)` pixels, so
   steps add 904–905px each, 0% is rock byte-for-byte, 100% is snow
   byte-for-byte, repeats are byte-identical.
5. Intermediate frames lerp registered RGBA across a ±0.5% smoothstep
   band, then composite over white (gray) or threshold with the
   blue-noise map (one-bit).

## Tests

Synthetic-input unit tests (no asset dependency):

```sh
cd tools/mountain_snow && python3 -m unittest discover -s tests -t .
```

They cover seed filtering, flood reachability/ordering, rank span,
endpoint exactness, dither determinism, union-find components, and
revert detection.

## Known findings (for the visual gate)

- 4 seed basins (main summit + ridge chain + right ridge) plus 4 orphan
  wash specks totalling 12px at ≤ 7% opacity.
- Transient fragments ≤ 246px at 10/25% (unmerged seed basins and
  noise-roughened front splinters); one dominant mass at every step;
  zero revert pixels; area strictly nondecreasing.
- No authored sky/sun composition exists yet, so edge review composites
  over white and light gray only.
