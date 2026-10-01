#!/usr/bin/env python3
"""Build the mountain-snow device pack (SD format v1) and check parity.

Reads the three authored PNGs plus the repo blue-noise map, recompiles
the arrival order with the experiment modules, and writes the streaming
pack the firmware composer consumes: gray endpoint rows, barrier rows,
packed eligibility rows, and noise rows for the measured mountain-bounds
band, with a 64-byte header and IEEE CRC32 (see
platform/ui/visuals/mountain-snow/src/pack.rs).

Then renders every percentage through the *device* model (barrier-level
cut from a 256-bin histogram + noise band) and diffs it against the host
frames, reporting the parity gap the prototype accepts by not storing
the full order field.

Usage from the repository root:
    python3 tools/mountain_snow/build_pack.py [--band 64]
Outputs (gitignored): .scratch/mountain-snow-pack/
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np
from PIL import Image

from mountain_snow import order as ordmod
from mountain_snow import render as rend

MAGIC = b"AMBMNT01"
HEADER_LEN = 64
WIDTH = 600
BAND_DEFAULT = 64

assert zlib.crc32(b"123456789") == 0xCBF43926


def repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True, capture_output=True, text=True)
    return Path(out.stdout.strip())


def pack_u8(rows: np.ndarray) -> bytes:
    return rows.astype(np.uint8).tobytes()


def pack_elig_bits(elig: np.ndarray) -> bytes:
    flat = elig.astype(np.uint8)
    assert flat.shape[1] % 8 == 0
    out = np.zeros((flat.shape[0], flat.shape[1] // 8), dtype=np.uint8)
    for bit in range(8):
        out |= (flat[:, bit::8] << (7 - bit)).astype(np.uint8)
    return out.tobytes()


def device_render(gray_rock, gray_snow, barrier, elig, noise,
                  hist, target, band):
    """Device model: histogram cut + noise band, MSB-first 1-bit output."""
    cum, level, frac = 0, 255, 256
    for lv in range(256):
        pop = int(hist[lv])
        if pop and target < cum + pop:
            level, frac = lv, (target - cum) * 256 // pop
            break
        cum += pop
    snow = np.zeros_like(barrier, bool)
    snow |= (barrier < level) & elig
    at = (barrier == level) & elig
    if band <= 0:
        snow |= at & (noise.astype(np.int32) < frac)
        gray = np.where(snow, gray_snow, gray_rock)
    else:
        d = frac - noise.astype(np.int32)
        full_snow = at & (d >= band)
        full_rock = at & (d <= -band)
        snow |= full_snow
        gray = np.where(snow, gray_snow, gray_rock).astype(np.float64)
        blend = at & ~full_snow & ~full_rock
        num = (d[blend] + band) / (2 * band)
        gray[blend] = gray_rock[blend] * (1 - num) + gray_snow[blend] * num
    # Host-exact integer dither: ink iff 256*gray <= 255*noise + 127.
    gray_u8 = np.clip(np.rint(gray), 0, 255).astype(np.int32)
    bit = ((gray_u8 * 256 <= noise * 255 + 127) & elig)
    return bit, (level, min(frac, 256))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--band", type=int, default=BAND_DEFAULT)
    ap.add_argument("--out", default=".scratch/mountain-snow-pack")
    args = ap.parse_args()
    t0 = time.time()
    root = repo_root()
    out = root / args.out
    out.mkdir(parents=True, exist_ok=True)

    asset = root / "assets/experiments/ambient-home-mountain-snow"
    rock = rend.read_rgba(asset / "mountain-rock.png")
    snow = rend.read_rgba(asset / "mountain-snow-100.png")
    guide = rend.read_rgba(asset / "snow-arrival-guide.png")
    bluenose = rend.load_blue_noise(root)
    lum = ordmod.guide_luminance(guide)
    elig = ordmod.eligibility(rock, snow)
    seeds, _ = ordmod.find_seeds(elig, lum)
    barrier, _, orphans = ordmod.priority_flood(elig, lum, seeds)

    rows = np.where(elig.any(axis=1))[0]
    r0, r1 = int(rows.min()), int(rows.max()) + 1
    nrows = r1 - r0
    print(f"mountain row band: [{r0}, {r1}) ({nrows} rows)")

    gray_rock = rend.to_gray(rock)[r0:r1]
    gray_snow = rend.to_gray(snow)[r0:r1]
    # Barrier outside eligibility is unstored padding; eligibility decides.
    bar = np.where(elig[r0:r1], barrier[r0:r1].astype(np.uint8), 0)
    el = elig[r0:r1]
    nz = bluenose[r0:r1]

    sections = [pack_u8(gray_rock), pack_u8(gray_snow), pack_u8(bar),
                pack_elig_bits(el), pack_u8(nz)]
    payload = b"".join(sections)
    header = bytearray(HEADER_LEN)
    header[0:8] = MAGIC
    struct.pack_into("<HHHH", header, 8, WIDTH, WIDTH, r0, nrows)
    struct.pack_into("<IIIII", header, 16, *(len(s) for s in sections))
    struct.pack_into("<I", header, 36, zlib.crc32(payload) & 0xFFFFFFFF)
    pack_bytes = bytes(header) + payload
    (out / "mountain-snow-v1.pack").write_bytes(pack_bytes)

    manifest = {
        "magic": MAGIC.decode(), "version": 1,
        "width": WIDTH, "height": WIDTH, "first_row": r0, "rows": nrows,
        "sections": ["rock_gray", "snow_gray", "barrier", "eligibility_bits", "noise"],
        "section_bytes": [len(s) for s in sections],
        "pack_bytes": len(pack_bytes),
        "pack_sha256": hashlib.sha256(pack_bytes).hexdigest(),
        "sources": {
            "rock": hashlib.sha256((asset / "mountain-rock.png").read_bytes()).hexdigest(),
            "snow": hashlib.sha256((asset / "mountain-snow-100.png").read_bytes()).hexdigest(),
            "guide": hashlib.sha256((asset / "snow-arrival-guide.png").read_bytes()).hexdigest(),
            "blue_noise": rend.BLUE_NOISE_SHA256,
        },
        "orphan_seeds": orphans,
        "band_noise_units": args.band,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n")
    print(f"pack bytes: {len(pack_bytes)} "
          f"({', '.join(str(len(s)) for s in sections)})")

    # Parity: device model vs host one-bit frames.
    hist = np.bincount(barrier[elig].astype(np.int64), minlength=256)
    count = int(elig.sum())
    frames = root / ".scratch/ambient-home-mountain-snow/frames_1bit"
    if not (frames / "snow-050.png").exists():
        print("host frames missing; run run_experiment.py first")
        return 2
    diffs = []
    grays = (gray_rock.astype(np.float64), gray_snow.astype(np.float64))
    for s in range(101):
        target = round(s * count / 100)
        if s in (0, 100):
            endpoint = np.clip(np.rint(grays[1 if s == 100 else 0]), 0, 255).astype(np.int32)
            bit = elig[r0:r1] & (endpoint * 256 <= nz.astype(np.int32) * 255 + 127)
        else:
            bit, _ = device_render(grays[0], grays[1], bar.astype(np.int32),
                                   el, nz.astype(np.int32),
                                   hist, target, args.band)
        full = np.full((600, 600), 255, np.uint8)
        full[r0:r1] = np.where(bit, 0, 255).astype(np.uint8)
        host = np.array(Image.open(frames / f"snow-{s:03d}.png").convert("L"))
        diffs.append(int((((full <= 128) != (host <= 128))).sum()))
    parity = {"max_diff_px": max(diffs), "mean_diff_px": sum(diffs) / len(diffs),
              "diffs": diffs}
    (out / "parity.json").write_text(json.dumps(parity, indent=1) + "\n")
    print(f"parity vs host 1-bit: max {max(diffs)}px "
          f"mean {sum(diffs) / len(diffs):.0f}px of {count} eligible")
    print(f"total {time.time() - t0:.1f}s")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
