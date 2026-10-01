"""Committed unit tests for the mountain-snow renderer and diagnostics.

Synthetic inputs only; no dependency on the authored PNG assets.
Run from tools/mountain_snow:  python3 -m unittest discover -s tests -t .
"""

import os
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mountain_snow import diagnostics as diag
from mountain_snow import render as rend


def rgba(gray, alpha=255):
    img = np.zeros((10, 10, 4), dtype=np.uint8)
    img[..., :3] = gray
    img[..., 3] = alpha
    return img


class BlendTests(unittest.TestCase):
    def setUp(self):
        self.rock = rgba(60)
        self.snow = rgba(220)
        self.elig = np.zeros((10, 10), dtype=bool)
        self.elig[2:8, 2:8] = True
        n = int(self.elig.sum())
        self.index = np.full((10, 10), -1, dtype=np.int64)
        self.index[self.elig] = np.arange(n)
        self.count = n

    def test_endpoints_exact(self):
        self.assertEqual(
            rend.blend(self.rock, self.snow, self.index, self.count, 0, 1).tobytes(),
            self.rock.tobytes())
        self.assertEqual(
            rend.blend(self.rock, self.snow, self.index, self.count, 100, 1).tobytes(),
            self.snow.tobytes())

    def test_midpoint_mixes_inside_only(self):
        out = rend.blend(self.rock, self.snow, self.index, self.count, 50, 1)
        self.assertTrue((out[~self.elig] == self.rock[~self.elig]).all())
        inside = out[self.elig].astype(np.int32)[..., 0]
        self.assertTrue(bool(((inside > 60) & (inside < 220)).any()))

    def test_monotone_weights(self):
        pos = np.arange(-1, 12)
        prev = None
        for cutoff in range(0, 13):
            w = rend.snow_weight(pos, cutoff, 2)
            self.assertTrue(bool(((w >= 0) & (w <= 1)).all()))
            self.assertEqual(w[0], 0.0)  # non-eligible stays rock
            if prev is not None:
                self.assertTrue(bool((w[1:] >= prev[1:]).all()))
            prev = w


class ToneTests(unittest.TestCase):
    def test_transparent_is_paper(self):
        g = rend.to_gray(rgba(0, alpha=0))
        self.assertTrue((g == 255).all())
        b = rend.to_1bit(rgba(0, alpha=0),
                         np.zeros((10, 10), dtype=np.uint8))
        self.assertTrue((b == 255).all())

    def test_onebit_deterministic_and_bounded(self):
        frame = rgba(128)
        noise = ((np.arange(100).reshape(10, 10) * 53) % 256).astype(np.uint8)
        a = rend.to_1bit(frame, noise)
        b = rend.to_1bit(frame, noise)
        self.assertEqual(a.tobytes(), b.tobytes())
        self.assertTrue(set(np.unique(a).tolist()) <= {0, 255})

    def test_blue_noise_hash_enforced(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "platform" / "ui" / "visuals" / "blue-noise" /
             "assets").mkdir(parents=True)
            target = (Path(tmp) / "platform" / "ui" / "visuals" / "blue-noise" /
                      "assets" / "blue-noise-600x600.bin")
            target.write_bytes(b"\x00" * (600 * 600))
            with self.assertRaises(ValueError):
                rend.load_blue_noise(Path(tmp))


class DiagnosticsTests(unittest.TestCase):
    def test_single_block_single_component(self):
        # Row-major arrival over one 4x4 block: always connected.
        order = np.array([y * 10 + x for y in range(4) for x in range(4)])
        comps = diag.components_along_order(order, 10, 10, [0, 8, 16])
        self.assertEqual(comps, [0, 1, 1])

    def test_two_blocks_merge(self):
        left = [y * 10 + x for y in range(4) for x in range(2)]
        right = [y * 10 + x for y in range(4) for x in range(4, 6)]
        bridge = [y * 10 + 2 for y in range(4)] + [y * 10 + 3 for y in range(4)]
        order = np.array(left + right + bridge)
        comps = diag.components_along_order(order, 10, 10,
                                            [0, 8, 16, 24])
        self.assertEqual(comps, [0, 1, 2, 1])

    def test_no_reverts_on_growing_cutoffs(self):
        order = np.arange(20)
        self.assertEqual(diag.revert_pixels(order, [0, 5, 10, 20]), 0)

    def test_reverts_detected_on_shrinking_cutoffs(self):
        order = np.arange(20)
        self.assertGreater(diag.revert_pixels(order, [0, 10, 5, 20]), 0)

    def test_step_bounds_shape(self):
        order = np.array([5, 15, 25, 35])
        bounds = diag.step_bounds(order, 10, [0, 2, 4])
        self.assertEqual(bounds, [[0, 5, 1, 5], [2, 5, 3, 5]])


if __name__ == "__main__":
    unittest.main()
