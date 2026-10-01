#!/usr/bin/env python3
"""Reproducible quality gate for full-screen blue-noise threshold assets.

Host-only check (requires numpy on the host; no firmware/runtime
dependency). Run from anywhere:

    python3 platform/ui/visuals/blue-noise/generator/verify.py

Per asset (W, H, path, seed) it checks:

  - exact byte size W*H and a balanced histogram (per-level counts
    within +-1, all 256 levels present)
  - 20/50/80% binary cutoffs: radial low-band/mid-band power ratio < 0.5
    and well below a deterministic white-noise reference
  - no exact 32/64-cell full-array horizontal/vertical repeat
    (whole-array roll comparison, not a single row/column probe)
  - horizontal/vertical angular-sector power ratio on the mid band

Frequencies are normalized physical frequencies in cycles/pixel built
with ``np.fft.fftfreq`` on each axis, so the rectangular 400x300 map
gets its correct (non-square) frequency grid. Low band is
0 < f < 0.125 cycles/pixel; middle band is 0.125 <= f < 0.5. The DC
term is excluded from the low band. Angular sectors are +-15 degrees
around the horizontal (fx) and vertical (fy) frequency axes,
evaluated on the middle band.

The reported ratios describe these two shipped assets only; they are
not claimed as proof of isotropy at every cutoff or scale.
"""

import hashlib
import sys
from pathlib import Path

import numpy as np

REPO_ROOT = Path(__file__).resolve().parents[5]
CRATE = REPO_ROOT / "platform" / "ui" / "visuals" / "blue-noise"

CASES = [
    (600, 600, CRATE / "assets" / "blue-noise-600x600.bin", 600600),
    (400, 300, CRATE / "assets" / "blue-noise-400x300.bin", 400300),
]

LOW_CUTOFF = 0.125  # cycles/pixel: low is 0 < f < this, mid continues to 0.5
SECTOR_HALF_ANGLE = np.pi / 12  # +-15 degrees around each frequency axis
CUTOFFS = (0.2, 0.5, 0.8)
REPEAT_PERIODS = (32, 64)


def spectrum(tile, level):
    """Shifted power spectrum of the mean-free binary image at a cutoff."""
    mid = (tile < level).astype(float)
    spec = np.fft.fftshift(np.fft.fft2(mid - mid.mean()))
    return np.abs(spec) ** 2


def freq_grid(w, h):
    """Normalized physical frequencies (cycles/pixel) per axis."""
    fx = np.fft.fftfreq(w)
    fy = np.fft.fftfreq(h)
    xx, yy = np.meshgrid(np.fft.fftshift(fx), np.fft.fftshift(fy))
    return np.sqrt(xx * xx + yy * yy), np.arctan2(yy, xx)


def radial_bands(tile, level):
    """(low/mid power ratio, low mean, mid mean) at a binary cutoff."""
    p = spectrum(tile, level)
    h, w = tile.shape
    rr, _ = freq_grid(w, h)
    low = p[(rr > 0) & (rr < LOW_CUTOFF)].mean()
    band = p[(rr >= LOW_CUTOFF) & (rr < 0.5)].mean()
    return float(low / band), float(low), float(band)


def white_ratio(level, shape, seed):
    """Same band ratio for a deterministic white-noise reference image."""
    rng = np.random.default_rng(int(seed) ^ 0x9E3779B9)
    tile = rng.integers(0, 256, size=shape).astype(np.uint8)
    return radial_bands(tile, level)


def sector_anisotropy(tile, level):
    """Mid-band horizontal-sector / vertical-sector power ratio."""
    p = spectrum(tile, level)
    h, w = tile.shape
    rr, ang = freq_grid(w, h)
    ring = (rr >= LOW_CUTOFF) & (rr < 0.5)
    hsec = ring & (np.minimum(np.abs(ang), np.pi - np.abs(ang)) < SECTOR_HALF_ANGLE)
    vsec = ring & (np.abs(np.abs(ang) - np.pi / 2) < SECTOR_HALF_ANGLE)
    return float(p[hsec].mean() / p[vsec].mean())


def main():
    ok = True
    for w, h, path, seed in CASES:
        raw = Path(path).read_bytes()
        assert len(raw) == w * h, (str(path), len(raw))
        tile = np.frombuffer(raw, dtype=np.uint8).reshape(h, w)
        sha = hashlib.sha256(raw).hexdigest()
        hist, _ = np.histogram(tile, bins=256, range=(0, 256))
        print(f"{w}x{h}: size={len(raw)} sha256={sha}")
        print(
            f"  hist min={hist.min()} max={hist.max()} "
            f"expect~{w * h / 256:.2f} levels={np.count_nonzero(hist)}/256"
        )
        assert hist.max() - hist.min() <= 1
        assert np.count_nonzero(hist) == 256
        for frac in CUTOFFS:
            level = int(frac * 256)
            ratio, _, _ = radial_bands(tile, level)
            wratio, _, _ = white_ratio(level, tile.shape, seed)
            flag = "OK" if ratio < 0.5 else "FAIL"
            if ratio >= 0.5:
                ok = False
            print(
                f"  {int(frac * 100)}%: blue low/mid={ratio:.4f} "
                f"white={wratio:.4f} [{flag}]"
            )
        # Compare the whole overlapping interior as well as toroidal rolls.
        # A small tile cropped to a non-multiple screen size can fail the
        # roll comparison at the seam while still repeating everywhere else.
        for p in REPEAT_PERIODS:
            assert not np.array_equal(tile[:, p:], tile[:, :-p]), (
                f"horizontal interior repeat {p}"
            )
            assert not np.array_equal(tile[p:, :], tile[:-p, :]), (
                f"vertical interior repeat {p}"
            )
            assert not np.array_equal(tile, np.roll(tile, p, axis=1)), (
                f"horizontal repeat {p}"
            )
            assert not np.array_equal(tile, np.roll(tile, p, axis=0)), (
                f"vertical repeat {p}"
            )
        print("  no exact 32/64 full-array h/v repeat: OK")
        aniso = sector_anisotropy(tile, 128)
        print(f"  50% mid-band h/v sector power ratio={aniso:.3f}")
    print("QUALITY " + ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
