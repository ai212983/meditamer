"""Pack sky_01 + sun_05 into an SD staging dir and render a review sheet.

From the repository root:

  python3 tools/ambient_sky/run_pack.py [--out-dir DIR] [--assets DIR]

Outputs (gitignored derived data unless --out-dir points at a tracked dir):
  AMBIENT/SKY.BIN    device pack (64-byte header + 1-bit planes)
  AMBIENT/MANIFEST.txt  sources, clip bbox, anchor, sizes, crc, SD target
  review.png         sky + sun at five journey fractions
  review-midpoint.png  sky + sun at the journey midpoint
  sun-clip.png       clipped sun at 2x on mid-gray with anchor cross

Exit 0 is clean; exit 2 is a hard validation failure.
"""

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ambient_sky import pack as P

REPO_ROOT = Path(__file__).resolve().parent.parent.parent


def load_inputs(assets_dir):
    from PIL import Image
    import numpy as np

    sky_path = assets_dir / "sky_01_dithered.png"
    sun_path = assets_dir / "sun_05_dithered.png"
    sky_img = Image.open(sky_path)
    if sky_img.size != (600, 600) or sky_img.mode != "L":
        raise ValueError(f"{sky_path}: expected 600x600 L, got {sky_img.size} {sky_img.mode}")
    sky = np.array(sky_img)
    sun_img = Image.open(sun_path)
    if sun_img.size != (600, 600) or sun_img.mode != "LA":
        raise ValueError(f"{sun_path}: expected 600x600 LA, got {sun_img.size} {sun_img.mode}")
    sun = np.array(sun_img)
    return sky, sun[:, :, 0], sun[:, :, 1]


def paint_arc_guide(canvas):
    """Mirror the firmware's 48-segment, one-pixel ink guide."""
    def rounded(value):
        return int(value + 0.5) if value >= 0 else int(value - 0.5)

    previous = tuple(map(rounded, P.point_on_curve(0.0)))
    for step in range(1, 48):
        target = tuple(map(rounded, P.point_on_curve(step / 47.0)))
        x, y = previous
        dx, dy = abs(target[0] - x), -abs(target[1] - y)
        sx, sy = (1 if x < target[0] else -1), (1 if y < target[1] else -1)
        error = dx + dy
        while True:
            if 0 <= x < 600 and 0 <= y < 600:
                canvas[y, x] = 0
            if (x, y) == target:
                break
            twice = 2 * error
            if twice >= dy:
                error += dy
                x += sx
            if twice <= dx:
                error += dx
                y += sy
        previous = target


def render_review(sky_ink, sun_ink, sun_mask, anchor, fractions):
    """600x600 review: sky, arc guide, and sun at each fraction."""
    from PIL import Image
    import numpy as np

    canvas = np.full((600, 600), 255, dtype=np.uint8)
    canvas[sky_ink] = 0
    paint_arc_guide(canvas)
    # Sun at each review fraction, anchored by the mask centroid.
    sh, sw = sun_ink.shape
    for f in fractions:
        cx, cy = P.point_on_curve(f)
        x0 = int(round(cx - anchor[0]))
        y0 = int(round(cy - anchor[1]))
        for y in range(sh):
            for x in range(sw):
                if not sun_mask[y, x]:
                    continue
                xx, yy = x0 + x, y0 + y
                if 0 <= xx < 600 and 0 <= yy < 600:
                    canvas[yy, xx] = 0 if sun_ink[y, x] else 255
    return Image.fromarray(canvas, mode="L")


def render_clip(sun_ink, sun_mask, anchor):
    """Clipped sun at 2x on mid-gray with an anchor cross."""
    from PIL import Image
    import numpy as np

    h, w = sun_ink.shape
    big = np.full((h * 2, w * 2), 128, dtype=np.uint8)
    for y in range(h):
        for x in range(w):
            if sun_mask[y, x]:
                big[y * 2 : y * 2 + 2, x * 2 : x * 2 + 2] = 0 if sun_ink[y, x] else 255
    ax, ay = int(round(anchor[0] * 2)), int(round(anchor[1] * 2))
    big[max(0, ay - 6) : ay + 7, ax] = 64
    big[ay, max(0, ax - 6) : ax + 7] = 64
    return Image.fromarray(big, mode="L")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", default=str(REPO_ROOT / ".scratch/ambient-home-sky-sun"))
    ap.add_argument("--assets", default=str(REPO_ROOT / "assets"))
    args = ap.parse_args()
    try:
        sky, sun_gray, sun_alpha = load_inputs(Path(args.assets))
        sky_ink = sky < 128
        sun_ink, sun_mask, bbox, anchor = P.clip_sun(sun_gray, sun_alpha)
        blob = P.encode_pack(sky_ink, sun_ink, sun_mask, anchor)
        decoded = P.decode_pack(blob)  # round-trip before writing anything
        assert decoded["sky"] == blob[P.HEADER_LEN : P.HEADER_LEN + P.SKY_LEN]

        out = Path(args.out_dir) / "AMBIENT"
        out.mkdir(parents=True, exist_ok=True)
        dest, tmp = out / "SKY.BIN", out / "SKY.BIN.tmp"
        tmp.write_bytes(blob)
        tmp.rename(dest)
        x0, y0, w, h = bbox
        manifest = (
            "# ambient sky/sun pack manifest\n"
            "source_sky: sky_01_dithered.png\n"
            "source_sun: sun_05_dithered.png\n"
            f"sun_bbox: x={x0} y={y0} w={w} h={h}\n"
            f"sun_anchor: x={anchor[0]:.2f} y={anchor[1]:.2f}\n"
            f"sky_len: {len(decoded['sky'])}\n"
            f"sun_len: {len(decoded['sun_ink'])}\n"
            f"file_len: {len(blob)}\n"
            "sd_target: /assets/AMBIENT/SKY.BIN\n"
        )
        (out / "MANIFEST.txt").write_text(manifest)
        review = render_review(sky_ink, sun_ink, sun_mask, anchor, P.REVIEW_FRACTIONS)
        review.save(Path(args.out_dir) / "review.png")
        render_review(sky_ink, sun_ink, sun_mask, anchor, (0.5,)).save(
            Path(args.out_dir) / "review-midpoint.png"
        )
        render_clip(sun_ink, sun_mask, anchor).save(Path(args.out_dir) / "sun-clip.png")
        print(f"wrote {dest} ({len(blob)} bytes)")
        print(f"sun clip x={x0} y={y0} w={w} h={h} anchor=({anchor[0]:.1f},{anchor[1]:.1f})")
        print(f"review: {Path(args.out_dir) / 'review.png'}")
    except Exception as e:
        print(f"pack failed: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
