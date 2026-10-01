# blue-noise

Shared full-screen blue-noise threshold maps for ordered dithering, one
independent map per supported screen size. Replaces the 32x32 tiled array:
these maps are synthesized at their exact native grids, so a frame never
repeats, crops, or rescales a small tile.

Method: R. A. Ulichney, "The Void-and-Cluster Method for Generating Dither
Arrays," Proceedings of SPIE vol. 1913, pp. 332-343 (1993)
(`https://cv.ulichney.com/papers/1993-void-cluster.pdf`). Self-contained
three-phase pipeline, one independent run per grid size: seed a ~10%
random pattern and swap-converge it (repeatedly move a 1 from the
tightest cluster into the largest void) until removing the 1 from the
tightest cluster creates the largest void at the same cell; that
homogeneous pattern is the prototype, kept and never mutated afterwards.
Phase I strips densest clusters (ranks n-1 down to 0), phase II refills
largest voids from a fresh prototype clone (ranks n up from the black
count), phase III orders the complementary white minority (ranks half
through N-1). Every rank is assigned exactly once, and the stored byte
is `rank * 256 / N` (integer floor, so per-level counts differ by at
most one).

## Assets (canonical, reusable)

| File | Grid | Bytes | Seed | Histogram |
| --- | --- | --- | --- | --- |
| `assets/blue-noise-600x600.bin` | 600x600 | 360000 | 600600 | every level 1406-1407x |
| `assets/blue-noise-400x300.bin` | 400x300 | 120000 | 400300 | every level 468-469x |

Raw row-major `u8` thresholds, byte = `rank * 256 / N`. Checks:

- 600x600 `sha256:275bcbefb4a58837a2d43ef00c8ba0c6daaf61b34db6c9ed72b1b239457e3d03`
- 400x300 `sha256:7b5f0a939c4bff1421de3435d4584095334aff84d81ed6d485e60807655d9566`

Sample at integer screen coordinates. GPU consumers should treat bytes as
scalar data, fetch nearest texels without sRGB conversion or mipmaps, and
apply `(byte + 0.5) / 256` explicitly. Filtering or resizing changes the
threshold distribution.

Quality (`generator/verify.py`, NumPy host-only check): frequencies are
normalized cycles/pixel per axis (`np.fft.fftfreq`), low band
0 < f < 0.125 versus middle band 0.125 <= f < 0.5, DC excluded. Measured
low/mid power ratios against a deterministic white-noise reference:

| Asset | 20% blue (white) | 50% blue (white) | 80% blue (white) | Mid-band h/v sectors |
| --- | --- | --- | --- | --- |
| 600x600 | 0.0338 (0.9984) | 0.0217 (1.0091) | 0.0344 (1.0077) | 1.000 |
| 400x300 | 0.0331 (0.9817) | 0.0215 (1.0125) | 0.0350 (1.0162) | 0.989 |

No exact 32/64-cell full-array horizontal/vertical repeat (whole-array
roll comparison). Angular sectors are +-15 degrees around the horizontal
and vertical frequency axes on the middle band. These ratios describe
the two shipped assets at these cutoffs only; they are not claimed as
proof of isotropy at every cutoff or scale.

## Generation

Host-only C++ generator (stdlib only), tuned for full-screen grids: the
filtered field is an incremental toroidal cache (finite wrapped Gaussian,
sigma 1.5, support radius 7, i.e. a 15x15 stencil; both shipped assets
were generated at this radius) updated on each dot toggle, with lazy
versioned max/min heaps over ON/OFF cells and seeded per-cell tie-breaks.
A per-pixel full-grid recompute would take hours at 600x600; this runs
in seconds (400x300 ~3 s, 600x600 ~10 s, single-threaded `-O3` host build).

Reproducible quality check from the repository root (NumPy required on the
host only, never a firmware/runtime dependency):

```sh
python3 platform/ui/visuals/blue-noise/generator/verify.py
```

From this crate directory:

```sh
c++ -O3 -std=c++17 -o generator/generate generator/generate.cpp
./generator/generate --width 600 --height 600 --seed 600600 \
    --output assets/blue-noise-600x600.bin
./generator/generate --width 400 --height 300 --seed 400300 \
    --output assets/blue-noise-400x300.bin
./generator/generate --self-check   # 32x32 warmup: impulse symmetry,
                                    # cache-vs-full-recompute, permutation,
                                    # seed sensitivity, stable output
```

`--progress <file> --progress-every <n>` writes an atomic one-line status
ledger (tmp file + rename). Output bytes are written atomically the same
way. Homogenization failure aborts nonzero instead of leaving a partial map.

## Rust API

`no_std`, no heap, no unsafe. Bytes are linked read-only via `include_bytes!`
and only borrowed (`BlueNoiseMap` is `Copy` over a `&'static [u8]`).

- `BlueNoiseMap { width, height, data }` with `threshold(x, y) -> f32`
  (`(byte + 0.5) / 256`, toroidal wrap, negatives and extremes valid) and
  `value(x, y) -> u8`.
- `map_for_size(w, h)` matches exact dimensions among enabled maps only.
- `default_map()` is the largest enabled map (600x600, else 400x300, else
  `None`).
- Features (default none; enable only what the app shows):
  `map-600x600`, `map-400x300`.

Embedded cost is read-only data (360000 + 120000 bytes when both maps are
enabled); no firmware DRAM or flash placement is claimed without an ELF
measurement.
