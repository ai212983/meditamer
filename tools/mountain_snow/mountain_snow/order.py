"""Order-field compiler for the mountain-snow progression experiment.

Reads the three authored PNGs, validates them, derives the mountain
eligibility region, finds summit/ridge seeds in the arrival guide, runs a
connected priority flood (minimax barrier), histogram-equalizes the barrier
into a 0..65535 rank, and emits a total arrival order broken by the
repository's stable blue-noise map.

Only stdlib + NumPy + Pillow; host-only, no firmware dependency.
"""

from __future__ import annotations

import collections
from dataclasses import dataclass

import numpy as np

SIZE = (600, 600)
ALPHA_THRESH = 8
SEED_LEVEL = 8
MIN_SEED_AREA = 50
RANK_MAX = 65_535
NEIGHBOURS_8 = (
    (-1, -1), (-1, 0), (-1, 1),
    (0, -1), (0, 1),
    (1, -1), (1, 0), (1, 1),
)


@dataclass
class SeedInfo:
    size: int
    centroid_yx: tuple[float, float]
    bbox_yxxy: tuple[int, int, int, int]


def guide_luminance(guide: np.ndarray) -> np.ndarray:
    """Straight (unmultiplied) gray value of the arrival guide.

    The guide is authored as unmultiplied gray + coverage alpha: summit
    seeds are transparent black (arrive first) while late foothill wash is
    semi-transparent light gray (arrives last). Compositing over black or
    white would invert one of those intents, so the RGB channel is read
    directly. Raises if the guide is not gray.
    """
    if guide.ndim != 3 or guide.shape[2] < 3:
        raise ValueError(f"guide must be an RGB(A) image, got shape {guide.shape}")
    rgb = guide[..., :3].astype(np.int32)
    if not bool(((rgb[..., 0] == rgb[..., 1]) & (rgb[..., 1] == rgb[..., 2])).all()):
        raise ValueError("arrival guide is not grayscale (R/G/B channels differ)")
    return rgb[..., 0]


def eligibility(rock: np.ndarray, snow: np.ndarray, thresh: int = ALPHA_THRESH) -> np.ndarray:
    """Mountain eligibility region: union of endpoint brush coverage.

    A pixel mask derived from endpoint alpha, never a filled polygon, so
    soft brush coverage at the foothills is preserved.
    """
    return (rock[..., 3].astype(np.int32) > thresh) | (
        snow[..., 3].astype(np.int32) > thresh
    )


def _dilate(mask: np.ndarray) -> np.ndarray:
    """Chebyshev-1 dilation of a boolean mask (no wrap at borders)."""
    padded = np.pad(mask, 1)
    out = padded[1:-1, 1:-1].copy()
    height, width = mask.shape
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            out |= padded[1 + dy:1 + dy + height, 1 + dx:1 + dx + width]
    return out


def connected_groups(mask: np.ndarray) -> list[list[tuple[int, int]]]:
    """8-connected cell groups of a boolean mask (deterministic order)."""
    label = np.full(mask.shape, -1, dtype=np.int32)
    groups: list[list[tuple[int, int]]] = []
    comp = 0
    height, width = mask.shape
    for y in range(height):
        for x in range(width):
            if not mask[y, x] or label[y, x] >= 0:
                continue
            stack = [(y, x)]
            label[y, x] = comp
            cells: list[tuple[int, int]] = []
            while stack:
                cy, cx = stack.pop()
                cells.append((cy, cx))
                for dy, dx in NEIGHBOURS_8:
                    ny, nx = cy + dy, cx + dx
                    if 0 <= ny < height and 0 <= nx < width:
                        if mask[ny, nx] and label[ny, nx] < 0:
                            label[ny, nx] = comp
                            stack.append((ny, nx))
            groups.append(cells)
            comp += 1
    return groups


def validate(rock: np.ndarray, snow: np.ndarray, guide: np.ndarray) -> list[tuple[str, bool, str]]:
    """Phase-1 endpoint checks. Returns (name, ok, detail) rows.

    The silhouette gate compares outer boundaries, not solid cores: the
    snow repaint legitimately densifies faint rock wash into solid snow
    in the foothills, so core-vs-core comparison would false-positive.
    """
    rows: list[tuple[str, bool, str]] = []
    for name, img in (("rock", rock), ("snow", snow), ("guide", guide)):
        ok = img.shape == (SIZE[1], SIZE[0], 4)
        rows.append((f"{name}.size-mode", ok, f"shape={img.shape}"))
    for name, img in (("rock", rock), ("snow", snow)):
        sky_alpha = int(img[:300, ..., 3].max())
        rows.append((f"{name}.sky-transparent", sky_alpha <= ALPHA_THRESH,
                     f"top-half max alpha={sky_alpha}"))
    a = rock[..., 3].astype(np.int32)
    b = snow[..., 3].astype(np.int32)
    outward = (b > ALPHA_THRESH) & (a == 0)
    fringe = _dilate(a > 0) & outward
    growth = outward & ~fringe
    rows.append(("endpoints.silhouette-no-growth", not bool(growth.any()),
                 f"snow paint beyond rock silhouette: {int(growth.sum())}px "
                 f"({int(fringe.sum())}px edge fringe within 1px)"))
    inward = (a > 128) & (b == 0)
    groups = connected_groups(inward)
    biggest = max((len(g) for g in groups), default=0)
    rows.append(("endpoints.silhouette-no-holes", biggest <= 8,
                 f"rock-solid holes in snow: {int(inward.sum())}px "
                 f"in {len(groups)} groups, largest {biggest}px"))
    try:
        lum = guide_luminance(guide)
        elig = eligibility(rock, snow)
        rows.append(("guide.grayscale", True, "R==G==B everywhere"))
        rows.append(("guide.range-inside",
                     bool(lum[elig].min() <= SEED_LEVEL and lum[elig].max() >= 200),
                     f"min={int(lum[elig].min())} max={int(lum[elig].max())}"))
    except ValueError as exc:
        rows.append(("guide.grayscale", False, str(exc)))
    return rows


def find_seeds(
    elig: np.ndarray,
    lum: np.ndarray,
    seed_level: int = SEED_LEVEL,
    min_area: int = MIN_SEED_AREA,
) -> tuple[np.ndarray, list[SeedInfo]]:
    """Seed mask + seed inventory from dark guide basins (8-connected).

    Small dark specks (eraser holes, wash texture) fall below ``min_area``
    and are excluded; only authored summit/ridge basins seed the flood.
    Deterministic: row-major scan, fixed neighbour order.
    """
    dark = elig & (lum <= seed_level)
    mask = np.zeros(dark.shape, dtype=bool)
    seeds: list[SeedInfo] = []
    for cells in connected_groups(dark):
        if len(cells) >= min_area:
            for cy, cx in cells:
                mask[cy, cx] = True
            ys = [c[0] for c in cells]
            xs = [c[1] for c in cells]
            seeds.append(SeedInfo(
                size=len(cells),
                centroid_yx=(sum(ys) / len(ys), sum(xs) / len(xs)),
                bbox_yxxy=(min(ys), min(xs), max(ys), max(xs)),
            ))
    if not seeds:
        raise ValueError(
            f"no seed basin >= {min_area}px at guide level {seed_level}")
    return mask, seeds


def priority_flood(
    elig: np.ndarray, lum: np.ndarray, seeds: np.ndarray
) -> tuple[np.ndarray, np.ndarray, list[dict]]:
    """Minimax-barrier priority flood from the seeds (8-connected).

    barrier[p] = min over seed paths of max guide value along the path, so
    a pixel can only arrive through an already reached neighbour. Guide
    values are 0..255 integers, so a 256-bucket queue replaces the heap:
    popped barriers are nondecreasing and each pixel finalizes once.

    Detached faint wash specks unreachable from any guide basin are
    seeded at their darkest guide pixel so every eligible pixel still
    gets an arrival value; each such orphan is reported (size, guide
    value, max endpoint alpha) for the visual gate.
    Returns (barrier int16 with -1 outside eligibility, finalize
    sequence, orphan-seed reports).
    """
    height, width = elig.shape
    barrier = np.full((height, width), 300, dtype=np.int16)
    finalized = np.zeros((height, width), dtype=bool)
    seq = np.full((height, width), -1, dtype=np.int32)
    buckets: list[collections.deque[tuple[int, int]]] = [
        collections.deque() for _ in range(256)
    ]

    cur = 0
    order = 0
    remaining = int(elig.sum())
    done = 0
    orphans: list[dict] = []

    def push(y: int, x: int, value: int) -> None:
        nonlocal cur
        if value < int(barrier[y, x]):
            barrier[y, x] = np.int16(value)
            buckets[value].append((y, x))
            if value < cur:
                cur = value

    for y in range(height):
        for x in range(width):
            if seeds[y, x]:
                push(y, x, int(lum[y, x]))

    def drain() -> None:
        nonlocal cur, order, done
        while done < remaining:
            while cur < 256 and not buckets[cur]:
                cur += 1
            if cur >= 256:
                return
            y, x = buckets[cur].popleft()
            if finalized[y, x] or not elig[y, x]:
                continue
            finalized[y, x] = True
            seq[y, x] = order
            order += 1
            done += 1
            bv = int(barrier[y, x])
            for dy, dx in NEIGHBOURS_8:
                ny, nx = y + dy, x + dx
                if 0 <= ny < height and 0 <= nx < width:
                    if elig[ny, nx] and not finalized[ny, nx]:
                        cand = int(lum[ny, nx])
                        if cand < bv:
                            cand = bv
                        push(ny, nx, cand)

    drain()
    if done < remaining:
        stranded = elig & ~finalized
        for cells in connected_groups(stranded):
            darkest = min(cells, key=lambda c: (int(lum[c]), c[0], c[1]))
            y, x = darkest
            push(y, x, int(lum[y, x]))
            orphans.append({
                "size": len(cells),
                "seed_yx": [y, x],
                "guide_value": int(lum[y, x]),
            })
            drain()
    if done < remaining:
        raise ValueError("flood stalled after orphan seeding")
    barrier[~elig] = -1
    return barrier, seq, orphans


def equalize_rank(barrier: np.ndarray, elig: np.ndarray) -> np.ndarray:
    """Histogram-equalize barrier values to 0..65535 over eligible pixels.

    Equal one-percent thresholds then add nearly equal eligible area even
    when the guide has broad flat tones; pixels sharing a barrier level
    form a rank plateau broken later by blue-noise sub-order.
    """
    vals = barrier[elig].astype(np.int32)
    hist = np.bincount(vals, minlength=256)
    cdf = np.cumsum(hist).astype(np.float64) / vals.size
    span = 1.0 - cdf[vals.min()]
    if span <= 0.0:  # single barrier level: everything arrives together
        lut = np.full(256, RANK_MAX, dtype=np.int32)
        lut[vals.min()] = 0
    else:
        lut = np.minimum(
            np.rint((cdf - cdf[vals.min()]) / span * RANK_MAX).astype(np.int64),
            RANK_MAX).astype(np.int32)
    rank = np.zeros(barrier.shape, dtype=np.uint16)
    rank[elig] = lut[vals]
    return rank


def build_order(
    elig: np.ndarray, rank: np.ndarray, bluenose: np.ndarray, seq: np.ndarray
) -> np.ndarray:
    """Total arrival order: flat pixel indices sorted by (rank, noise, seq).

    Deterministic: fixed map origin, unique finalize sequence as final
    tiebreak, stable mergesort-equivalent key ordering via lexsort.
    """
    ys, xs = np.where(elig)
    keys = (seq[elig].astype(np.int64),
            bluenose[ys, xs].astype(np.int64),
            rank[elig].astype(np.int64))
    order_pos = np.lexsort(keys)
    flat = (ys.astype(np.int64) * elig.shape[1] + xs.astype(np.int64))
    return flat[order_pos]


def cutoffs(count: int) -> list[int]:
    """Eligible-pixel counts for snow fractions 0..100 inclusive."""
    return [round(s * count / 100) for s in range(101)]
