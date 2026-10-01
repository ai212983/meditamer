"""Committed unit tests for the mountain-snow review sheets.

Run from tools/mountain_snow:  python3 -m unittest discover -s tests -t .
"""

import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mountain_snow import sheet


def endpoint(offset):
    img = np.zeros((600, 600, 4), dtype=np.uint8)
    img[330:, 100:500, :3] = offset
    img[330:, 100:500, 3] = 200
    return img


class SheetTests(unittest.TestCase):
    def test_edge_sheet_has_no_padding_holes(self):
        out = np.array(sheet.edge_sheet(endpoint(60), endpoint(220)))
        self.assertEqual(out.shape[2], 3)
        # Every row is white paper or paint; a geometry bug pads black.
        self.assertFalse(bool((out == 0).all(axis=2).any()))
        # Both backgrounds and all three crops are present.
        self.assertGreater(out.shape[0], 600)
        self.assertGreater(out.shape[1], 3 * 400)

    def test_contact_sheet_layout(self):
        gray = {s: np.full((600, 600), s * 2, dtype=np.uint8)
                for s in sheet.REVIEW_STEPS}
        bit = {s: np.full((600, 600), 255 if s > 50 else 0, dtype=np.uint8)
               for s in sheet.REVIEW_STEPS}
        out = sheet.contact_sheet(gray, bit)
        self.assertEqual(out.size,
                         (64 + 600 * 2, 28 + 600 * len(sheet.REVIEW_STEPS)))

    def test_viewer_covers_all_steps(self):
        html = sheet.viewer_html()
        for s in range(101):
            self.assertIn(f"frames_gray/snow-{s:03d}.png", html)
            self.assertIn(f"frames_1bit/snow-{s:03d}.png", html)


if __name__ == "__main__":
    unittest.main()
