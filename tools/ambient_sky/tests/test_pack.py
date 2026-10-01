"""Synthetic-input unit tests for the ambient sky/sun packer.

No asset dependency: all fixtures are built in-memory.
"""

import struct
import sys
import unittest
import zlib
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ambient_sky import pack as P


def checker(w, h, v=True):
    rows = []
    for y in range(h):
        rows.append([v and (x + y) % 2 == 0 for x in range(w)])
    return np.array(rows, dtype=bool)


class CurveTest(unittest.TestCase):
    def test_endpoints_match_control_points(self):
        self.assertEqual(P.point_on_curve(0.0), P.ARC_START)
        self.assertEqual(P.point_on_curve(1.0), P.ARC_END)

    def test_midpoint_matches_firmware_math(self):
        # Cubic at t=0.5 with the raised DEFAULT arch on 600x600.
        x, y = P.point_on_curve(0.5)
        self.assertAlmostEqual(x, 300.0)
        self.assertAlmostEqual(y, 105.75)

    def test_clamps(self):
        self.assertEqual(P.point_on_curve(-1.0), P.point_on_curve(0.0))
        self.assertEqual(P.point_on_curve(2.0), P.point_on_curve(1.0))


class BitPackTest(unittest.TestCase):
    def test_round_trip_with_padding(self):
        rows = checker(183, 5)  # 183px rows need a padded stride of 23.
        blob = P.pack_bits(rows.tolist())
        self.assertEqual(len(blob), 23 * 5)
        self.assertTrue((P.unpack_bits(blob, 183, 5) == rows).all())

    def test_msb_first(self):
        rows = [[True] + [False] * 7]
        self.assertEqual(P.pack_bits(rows), b"\x80")


class ClipTest(unittest.TestCase):
    def test_clips_to_nonzero_alpha(self):
        gray = np.full((600, 600), 200, dtype=np.uint8)
        alpha = np.zeros((600, 600), dtype=np.uint8)
        alpha[100:180, 50:130] = 255
        gray[100:180, 50:130] = 10
        ink, mask, bbox, anchor = P.clip_sun(gray, alpha)
        self.assertEqual(bbox, (50, 100, 80, 80))
        self.assertTrue(mask.all())
        self.assertTrue(ink.all())
        self.assertAlmostEqual(anchor[0], 39.5)
        self.assertAlmostEqual(anchor[1], 39.5)

    def test_empty_raises(self):
        with self.assertRaises(ValueError):
            P.clip_sun(np.zeros((10, 10), dtype=np.uint8), np.zeros((10, 10), dtype=np.uint8))

    def test_absurd_size_raises(self):
        alpha = np.full((600, 600), 255, dtype=np.uint8)
        with self.assertRaises(ValueError):
            P.clip_sun(np.zeros((600, 600), dtype=np.uint8), alpha)


class CodecTest(unittest.TestCase):
    def test_encode_decode_round_trip(self):
        sky = checker(600, 600)
        sun = checker(100, 60)
        blob = P.encode_pack(sky, sun, np.ones((60, 100), dtype=bool), (50.0, 30.0))
        got = P.decode_pack(blob)
        self.assertEqual((got["sun_w"], got["sun_h"]), (100, 60))
        self.assertAlmostEqual(got["anchor"][0], 50.0, places=4)
        self.assertTrue((P.unpack_bits(got["sky"], 600, 600) == sky).all())
        self.assertTrue((P.unpack_bits(got["sun_ink"], 100, 60) == sun).all())

    def test_crc_detects_corruption(self):
        sky = np.zeros((600, 600), dtype=bool)
        sun = np.zeros((40, 40), dtype=bool)
        blob = bytearray(P.encode_pack(sky, sun, sun, (20.0, 20.0)))
        blob[P.HEADER_LEN + 100] ^= 0xFF
        with self.assertRaises(ValueError):
            P.decode_pack(bytes(blob))

    def test_bad_magic_rejected(self):
        sky = np.zeros((600, 600), dtype=bool)
        sun = np.zeros((40, 40), dtype=bool)
        blob = bytearray(P.encode_pack(sky, sun, sun, (20.0, 20.0)))
        blob[0] ^= 0xFF
        with self.assertRaises(ValueError):
            P.decode_pack(bytes(blob))


if __name__ == "__main__":
    unittest.main()
