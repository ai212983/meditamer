#!/usr/bin/env python3
"""Run the ambient-home mountain-snow progression experiment (Phases 1-2).

Validates the endpoint PNGs, compiles the priority-flood arrival order,
renders all 101 integer snow percentages in grayscale and stable one-bit
form, and writes the review sheet, blink viewer, and diagnostics.

All outputs are experimental derived data under .scratch/ (gitignored);
no runtime format is committed and firmware behaviour is unchanged.

Usage from the repository root:
    python3 tools/mountain_snow/run_experiment.py
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np
from PIL import Image

from mountain_snow import diagnostics as diag
from mountain_snow import order as ordmod
from mountain_snow import render as rend
from mountain_snow import sheet

ASSET_DIR_REL = "assets/experiments/ambient-home-mountain-snow"
ROCK_NAME = "mountain-rock.png"
SNOW_NAME = "mountain-snow-100.png"
GUIDE_NAME = "snow-arrival-guide.png"


def repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True, capture_output=True, text=True)
    return Path(out.stdout.strip())


def save_png(path: Path, array: np.ndarray, mode: str) -> None:
    Image.fromarray(array, mode=mode).save(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default=".scratch/ambient-home-mountain-snow",
                    help="output directory (repo-relative, gitignored)")
    ap.add_argument("--skip-frames", action="store_true",
                    help="reuse existing frames, only rebuild sheets/diagnostics")
    args = ap.parse_args()

    root = repo_root()
    out = root / args.out
    (out / "frames_gray").mkdir(parents=True, exist_ok=True)
    (out / "frames_1bit").mkdir(parents=True, exist_ok=True)
    t0 = time.time()

    asset = root / ASSET_DIR_REL
    rock = rend.read_rgba(asset / ROCK_NAME)
    snow = rend.read_rgba(asset / SNOW_NAME)
    guide = rend.read_rgba(asset / GUIDE_NAME)
    bluenose = rend.load_blue_noise(root)

    print("== Phase 1: endpoint validation ==")
    checks = ordmod.validate(rock, snow, guide)
    failures = 0
    for name, ok, detail in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}: {detail}")
        failures += not ok

    lum = ordmod.guide_luminance(guide)
    elig = ordmod.eligibility(rock, snow)
    count = int(elig.sum())
    print(f"  eligible mountain pixels: {count}")

    print("== Phase 2: order-field compiler ==")
    seed_mask, seeds = ordmod.find_seeds(elig, lum)
    print(f"  seeds: {len(seeds)} basins")
    for i, s in enumerate(seeds):
        print(f"    seed {i}: size={s.size} "
              f"centroid=({s.centroid_yx[0]:.1f},{s.centroid_yx[1]:.1f}) "
              f"bbox={s.bbox_yxxy}")
    barrier, seq, orphans = ordmod.priority_flood(elig, lum, seed_mask)
    print(f"  orphan speck seeds: {len(orphans)} "
          f"({sum(o['size'] for o in orphans)}px total)")
    for o in orphans:
        print(f"    orphan: size={o['size']} seed={o['seed_yx']} "
              f"guide={o['guide_value']}")
    rank = ordmod.equalize_rank(barrier, elig)
    arrival = ordmod.build_order(elig, rank, bluenose, seq)
    cuts = ordmod.cutoffs(count)
    half_band = max(1, count // 200)
    print(f"  flood done in {time.time() - t0:.1f}s; "
          f"transition half-band={half_band}px")

    pos = np.empty(600 * 600, dtype=np.int64)
    pos[arrival] = np.arange(count, dtype=np.int64)
    order_index = np.where(elig, pos.reshape(600, 600), -1)

    print("== host rendering: 101 frames x gray/1-bit ==")
    frames_gray: dict[int, np.ndarray] = {}
    frames_1bit: dict[int, np.ndarray] = {}
    for s in range(101):
        gp = out / f"frames_gray/snow-{s:03d}.png"
        bp = out / f"frames_1bit/snow-{s:03d}.png"
        if args.skip_frames and gp.exists() and bp.exists():
            frames_gray[s] = np.array(Image.open(gp).convert("L"))
            frames_1bit[s] = np.array(Image.open(bp).convert("L"))
            continue
        frame = rend.blend(rock, snow, order_index, count, s, half_band)
        if s == 0:
            assert frame.tobytes() == rock.tobytes(), "0% must match rock"
        if s == 100:
            assert frame.tobytes() == snow.tobytes(), "100% must match snow"
        g = rend.to_gray(frame)
        bit = rend.to_1bit(frame, bluenose)
        save_png(gp, g, "L")
        save_png(bp, bit, "L")
        if s in sheet.REVIEW_STEPS:
            frames_gray[s], frames_1bit[s] = g, bit
    # Determinism: same percentage renders byte-identical twice.
    for s in (0, 7, 50, 93, 100):
        first = rend.to_1bit(
            rend.blend(rock, snow, order_index, count, s, half_band), bluenose)
        second = rend.to_1bit(
            rend.blend(rock, snow, order_index, count, s, half_band), bluenose)
        assert first.tobytes() == second.tobytes(), \
            f"non-deterministic render at {s}%"
    print("  endpoint exactness + determinism checks passed")

    print("== diagnostics ==")
    comps = diag.components_along_order(arrival, 600, 600, cuts)
    sizes_at_review = {}
    for s in sheet.REVIEW_STEPS:
        snow_now = elig & (pos.reshape(600, 600) < cuts[s])
        sizes = sorted((len(g) for g in ordmod.connected_groups(snow_now)),
                       reverse=True)
        sizes_at_review[s] = sizes[:8]
    print("  fragment sizes at review steps: " +
          ", ".join(f"{s}%:{sizes_at_review[s][:4]}"
                    for s in sheet.REVIEW_STEPS))
    bounds = diag.step_bounds(arrival, 600, cuts)
    reverts = diag.revert_pixels(arrival, cuts)
    deltas = [b - a for a, b in zip(cuts[:-1], cuts[1:])]
    report = {
        "eligible_pixels": count,
        "seeds": [ {"size": s.size,
                    "centroid_yx": list(s.centroid_yx),
                    "bbox_yxxy": list(s.bbox_yxxy)} for s in seeds ],
        "transition_half_band_px": half_band,
        "orphan_seeds": orphans,
        "revert_pixels": reverts,
        "step_new_pixels": {"min": min(deltas), "max": max(deltas)},
        "components": [{"s": s, "snow_area": cuts[s], "components": c,
                        "new_bounds_yxxy": bounds[s - 1] if s else None,
                        "top_sizes": sizes_at_review.get(s)}
                       for s, c in enumerate(comps)],
    }
    (out / "diagnostics.json").write_text(json.dumps(report, indent=1) + "\n")
    print(f"  revert pixels: {reverts}")
    print(f"  per-step new pixels: min={min(deltas)} max={max(deltas)}")
    print(f"  components at review steps: " +
          ", ".join(f"{s}%:{comps[s]}" for s in sheet.REVIEW_STEPS))

    print("== review outputs ==")
    for s in sheet.REVIEW_STEPS:
        if s not in frames_gray:
            frames_gray[s] = np.array(
                Image.open(out / f"frames_gray/snow-{s:03d}.png").convert("L"))
            frames_1bit[s] = np.array(
                Image.open(out / f"frames_1bit/snow-{s:03d}.png").convert("L"))
    sheet.contact_sheet(frames_gray, frames_1bit).save(out / "review-sheet.png")
    sheet.edge_sheet(rock, snow).save(out / "edge-sheet.png")
    sheet.write_text(out / "viewer.html", sheet.viewer_html())
    print(f"  wrote {out}/review-sheet.png, edge-sheet.png, viewer.html")
    print(f"  total {time.time() - t0:.1f}s")
    (out / "checks.json").write_text(
        json.dumps([{"name": n, "ok": o, "detail": d}
                    for n, o, d in checks], indent=1) + "\n")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
