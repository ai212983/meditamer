"""Committed unit tests for the mountain-snow order-field compiler.

Synthetic inputs only; no dependency on the authored PNG assets.
Run from tools/mountain_snow:  python3 -m unittest discover -s tests -t .
"""

import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mountain_snow import order as ordmod


def blob(shape, top, left, size, value):
    img = np.zeros(shape, dtype=np.uint8)
    img[top:top + size, left:left + size] = value
    return img


class GuideTests(unittest.TestCase):
    def test_rejects_non_gray(self):
        guide = np.zeros((4, 4, 4), dtype=np.uint8)
        guide[..., 0] = 10
        with self.assertRaises(ValueError):
            ordmod.guide_luminance(guide)

    def test_reads_straight_channel(self):
        guide = np.zeros((4, 4, 4), dtype=np.uint8)
        guide[..., :3] = 211
        guide[..., 3] = 64  # semi-transparent light wash stays late
        self.assertTrue((ordmod.guide_luminance(guide) == 211).all())


class EligibilityTests(unittest.TestCase):
    def test_union_of_coverage(self):
        rock = np.zeros((6, 6, 4), dtype=np.uint8)
        snow = np.zeros((6, 6, 4), dtype=np.uint8)
        rock[1:3, 1:3, 3] = 200
        snow[3:5, 3:5, 3] = 200
        elig = ordmod.eligibility(rock, snow)
        self.assertEqual(int(elig.sum()), 8)
        self.assertFalse(elig[0, 0])


class SeedTests(unittest.TestCase):
    def test_two_basins_plus_speck(self):
        elig = np.ones((30, 30), dtype=bool)
        lum = np.full((30, 30), 200, dtype=np.int32)
        lum[2:10, 2:10] = 3    # main summit basin, 64px
        lum[20:28, 20:28] = 5  # secondary basin, 64px
        lum[15, 15] = 0        # isolated speck, excluded
        mask, seeds = ordmod.find_seeds(elig, lum, seed_level=8, min_area=50)
        self.assertEqual(len(seeds), 2)
        self.assertFalse(mask[15, 15])
        self.assertTrue(mask[4, 4])

    def test_no_seeds_raises(self):
        elig = np.ones((8, 8), dtype=bool)
        lum = np.full((8, 8), 200, dtype=np.int32)
        with self.assertRaises(ValueError):
            ordmod.find_seeds(elig, lum)


class FloodTests(unittest.TestCase):
    def setUp(self):
        self.elig = np.zeros((20, 20), dtype=bool)
        self.elig[2:18, 2:18] = True
        # Bowl: dark summit corner, bright far corner, ridge across middle.
        yy, xx = np.mgrid[0:20, 0:20]
        self.lum = np.clip((xx + yy) * 8, 0, 255).astype(np.int32)
        self.lum[9:11, 2:18] = 250  # high ridge barrier mid-field
        self.seeds = np.zeros((20, 20), dtype=bool)
        self.seeds[2, 2] = True

    def test_full_coverage_and_seed_barrier(self):
        barrier, seq, orphans = ordmod.priority_flood(self.elig, self.lum, self.seeds)
        self.assertEqual(orphans, [])
        self.assertTrue((seq[self.elig] >= 0).all())
        self.assertEqual(barrier[2, 2], self.lum[2, 2])
        self.assertTrue((barrier[~self.elig] == -1).all())

    def test_barrier_nondecreasing_in_arrival_order(self):
        barrier, seq, orphans = ordmod.priority_flood(self.elig, self.lum, self.seeds)
        flat_barrier = barrier[self.elig]
        flat_seq = seq[self.elig]
        in_order = flat_barrier[np.argsort(flat_seq)]
        self.assertTrue(bool((np.diff(in_order.astype(np.int32)) >= 0).all()))

    def test_detached_speck_gets_reported_orphan_seed(self):
        elig = self.elig.copy()
        elig[0, 19] = True  # detached single pixel, far from the seed
        lum = self.lum.copy()
        barrier, seq, orphans = ordmod.priority_flood(elig, lum, self.seeds)
        self.assertEqual(len(orphans), 1)
        self.assertEqual(orphans[0]["size"], 1)
        self.assertTrue((seq[elig] >= 0).all())

    def test_every_pixel_reached_through_neighbour(self):
        barrier, seq, orphans = ordmod.priority_flood(self.elig, self.lum, self.seeds)
        ys, xs = np.where(self.elig & ~self.seeds)
        for y, x in zip(ys.tolist(), xs.tolist()):
            earlier = False
            for dy, dx in ordmod.NEIGHBOURS_8:
                ny, nx = y + dy, x + dx
                if 0 <= ny < 20 and 0 <= nx < 20 and self.elig[ny, nx]:
                    if seq[ny, nx] < seq[y, x]:
                        earlier = True
                        break
            self.assertTrue(earlier, f"pixel {(y, x)} has no earlier neighbour")


class RankTests(unittest.TestCase):
    def test_equalize_range_and_monotone(self):
        barrier = np.full((10, 10), -1, dtype=np.int16)
        elig = np.zeros((10, 10), dtype=bool)
        elig[1:9, 1:9] = True
        barrier[elig] = (np.arange(64) % 5 * 40).astype(np.int16)
        rank = ordmod.equalize_rank(barrier, elig)
        self.assertEqual(int(rank[elig].min()), 0)
        self.assertEqual(int(rank[elig].max()), 65535)
        # Same barrier -> same rank; higher barrier -> rank not lower.
        for _ in range(20):
            a = (np.random.randint(1, 9), np.random.randint(1, 9))
            b = (np.random.randint(1, 9), np.random.randint(1, 9))
            if barrier[a] <= barrier[b]:
                self.assertLessEqual(rank[a], rank[b])

    def test_build_order_is_total_eligible_order(self):
        elig = np.zeros((8, 8), dtype=bool)
        elig[1:7, 2:6] = True
        n = int(elig.sum())
        rank = np.zeros((8, 8), dtype=np.uint16)
        rank[elig] = np.arange(n, dtype=np.uint16) % 7
        noise = (np.arange(64, dtype=np.uint8).reshape(8, 8) * 37) % 251
        seq = np.full((8, 8), -1, dtype=np.int32)
        seq[elig] = np.arange(n)
        arrival = ordmod.build_order(elig, rank, noise, seq)
        self.assertEqual(len(arrival), n)
        self.assertEqual(len(set(arrival.tolist())), n)
        ranks_in_order = rank.flat[arrival]
        self.assertTrue(bool((np.diff(ranks_in_order.astype(np.int32)) >= 0).all()))

    def test_cutoffs_endpoints(self):
        cuts = ordmod.cutoffs(90416)
        self.assertEqual(len(cuts), 101)
        self.assertEqual(cuts[0], 0)
        self.assertEqual(cuts[100], 90416)
        self.assertTrue(all(b >= a for a, b in zip(cuts[:-1], cuts[1:])))


if __name__ == "__main__":
    unittest.main()
