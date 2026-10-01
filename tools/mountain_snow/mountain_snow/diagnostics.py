"""Sequence diagnostics for the mountain-snow progression experiment.

All diagnostics derive from the single total arrival order, so snow sets
are nested prefixes by construction; the revert check verifies that
explicitly instead of assuming it.

Only stdlib + NumPy; host-only.
"""

from __future__ import annotations

import numpy as np

NEIGHBOURS_8 = (
    (-1, -1), (-1, 0), (-1, 1),
    (0, -1), (0, 1),
    (1, -1), (1, 0), (1, 1),
)


def _find(par: list[int], i: int) -> int:
    while par[i] != i:
        par[i] = par[par[i]]
        i = par[i]
    return i


def components_along_order(
    order: np.ndarray, width: int, height: int, cutoffs: list[int]
) -> list[int]:
    """8-connected snow-region component counts at each cutoff.

    Single union-find pass over pixels in arrival order: activating each
    pixel once and unioning with active neighbours yields the exact
    component count for every threshold in O(N) total.
    """
    index_of = np.full(width * height, -1, dtype=np.int64)
    index_of[order] = np.arange(order.size, dtype=np.int64)
    parent = [-1] * order.size
    active = np.zeros(order.size, dtype=bool)
    comps = 0
    out: list[int] = []
    ptr = 0
    for cutoff in cutoffs:
        while ptr < cutoff:
            flat = int(order[ptr])
            parent[ptr] = ptr
            active[ptr] = True
            comps += 1
            y, x = divmod(flat, width)
            for dy, dx in NEIGHBOURS_8:
                ny, nx = y + dy, x + dx
                if 0 <= ny < height and 0 <= nx < width:
                    q = index_of[ny * width + nx]
                    if q >= 0 and active[q]:
                        rq, rp = _find(parent, int(q)), _find(parent, ptr)
                        if rq != rp:
                            parent[rq] = rp
                            comps -= 1
            ptr += 1
        out.append(comps)
    return out


def step_bounds(
    order: np.ndarray, width: int, cutoffs: list[int]
) -> list[list[int] | None]:
    """Bounding box [y0, x0, y1, x1] of newly snow pixels per step."""
    out: list[list[int] | None] = []
    for a, b in zip(cutoffs[:-1], cutoffs[1:]):
        if b <= a:
            out.append(None)
            continue
        ys, xs = np.divmod(order[a:b], width)
        out.append([int(ys.min()), int(xs.min()), int(ys.max()), int(xs.max())])
    return out


def revert_pixels(order: np.ndarray, cutoffs: list[int]) -> int:
    """Pixels snow at some step but rock at a later step (expect 0)."""
    pos = np.empty(int(order.max()) + 1, dtype=np.int64)
    pos[order] = np.arange(order.size, dtype=np.int64)
    snow = pos < cutoffs[0]
    reverts = 0
    for cutoff in cutoffs[1:]:
        nxt = pos < cutoff
        reverts += int((snow & ~nxt).sum())
        snow = nxt
    return reverts
