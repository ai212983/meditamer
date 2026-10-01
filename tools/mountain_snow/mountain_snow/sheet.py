"""Review outputs: contact sheet, edge sheet, and blink viewer (HTML)."""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

REVIEW_STEPS = [0, 10, 25, 50, 75, 90, 100]
EDGE_CROPS = {
    "summit": (200, 300, 400, 430),
    "foothill-left": (0, 470, 200, 600),
    "foothill-right": (400, 470, 600, 600),
}
EDGE_BACKGROUNDS = {"white": (255, 255, 255), "gray": (204, 204, 204)}


def contact_sheet(frames_gray: dict[int, np.ndarray],
                  frames_1bit: dict[int, np.ndarray]) -> Image.Image:
    """Side-by-side gray | 1-bit rows for the review percentages."""
    label_w = 64
    cell = 600
    sheet = Image.new("L", (label_w + cell * 2, 28 + cell * len(REVIEW_STEPS)), 255)
    draw = ImageDraw.Draw(sheet)
    draw.text((label_w + cell // 2 - 60, 6), "grayscale", fill=0)
    draw.text((label_w + cell + cell // 2 - 40, 6), "one-bit", fill=0)
    for i, s in enumerate(REVIEW_STEPS):
        y = 28 + i * cell
        draw.text((8, y + cell // 2), f"{s}%", fill=0)
        sheet.paste(Image.fromarray(frames_gray[s]), (label_w, y))
        sheet.paste(Image.fromarray(frames_1bit[s]), (label_w + cell, y))
    return sheet


def _over(rgba: np.ndarray, bg: tuple[int, int, int]) -> Image.Image:
    rgb = rgba[..., :3].astype(np.float64)
    alpha = rgba[..., 3:4].astype(np.float64) / 255.0
    comp = rgb * alpha + np.array(bg, dtype=np.float64) * (1.0 - alpha)
    return Image.fromarray(np.clip(np.rint(comp), 0, 255).astype(np.uint8))


def edge_sheet(rock: np.ndarray, snow: np.ndarray) -> Image.Image:
    """Endpoint edge crops at 2x over white and light gray.

    Supports the Phase-1 gate: blink the endpoints at native scale and
    review the transparent brush edge over both backgrounds. Layout is
    one row per background, one column per crop; each cell stacks the
    rock crop above the snow crop.
    """
    scale = 2
    pad = 8
    label_w = 56
    cells = []
    for bg in EDGE_BACKGROUNDS.values():
        row = []
        for box in EDGE_CROPS.values():
            x0, y0, x1, y1 = box
            pair = [_over(img[y0:y1, x0:x1], bg).resize(
                ((x1 - x0) * scale, (y1 - y0) * scale), Image.NEAREST)
                for img in (rock, snow)]
            row.append(pair)
        cells.append(row)
    cell_w = max(p[0].size[0] for row in cells for p in row)
    cell_h = max(p[0].size[1] + 4 + p[1].size[1] for row in cells for p in row)
    cols = len(EDGE_CROPS)
    sheet = Image.new(
        "RGB",
        (label_w + cols * (cell_w + pad) + pad,
         24 + len(cells) * (cell_h + pad) + pad),
        (255, 255, 255))
    draw = ImageDraw.Draw(sheet)
    for cx, name in enumerate(EDGE_CROPS):
        draw.text((label_w + cx * (cell_w + pad) + pad + 120, 6),
                  name, fill=(0, 0, 0))
    y = 24
    for (bg_name, _), row in zip(EDGE_BACKGROUNDS.items(), cells):
        draw.text((pad, y + 4), bg_name, fill=(0, 0, 0))
        for cx, pair in enumerate(row):
            x = label_w + cx * (cell_w + pad) + pad
            sheet.paste(pair[0], (x, y))
            sheet.paste(pair[1], (x, y + pair[0].size[1] + 4))
        y += cell_h + pad
    return sheet


VIEWER_HTML = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Mountain-snow progression viewer</title>
<style>
body {{ background: #888; font-family: sans-serif; text-align: center; }}
#stage {{ background: #fff; display: inline-block; margin: 12px; }}
img {{ width: 600px; height: 600px; image-rendering: pixelated; display: none; }}
img.on {{ display: block; }}
</style>
</head>
<body>
<h2>Mountain snow <span id="pct">50</span>% (<span id="mode">grayscale</span>)</h2>
<div>
<input id="slider" type="range" min="0" max="100" value="50" style="width:600px">
</div>
<div>
<button id="play">&#9654; play</button>
<button id="toggle">toggle gray / 1-bit</button>
<label><input id="blink" type="checkbox"> blink adjacent (500ms)</label>
</div>
<div id="stage">
{imgs}
</div>
<script>
const slider = document.getElementById('slider');
const pct = document.getElementById('pct');
const modeEl = document.getElementById('mode');
let mode = 'gray', timer = null, blinkOn = false;
function show() {{
  const s = slider.value;
  pct.textContent = s;
  modeEl.textContent = mode === 'gray' ? 'grayscale' : 'one-bit';
  document.querySelectorAll('#stage img').forEach(im => {{
    im.classList.toggle('on', im.dataset.s === s && im.dataset.m === mode);
  }});
}}
slider.addEventListener('input', show);
document.getElementById('toggle').addEventListener('click', () => {{
  mode = mode === 'gray' ? 'bit' : 'gray'; show();
}});
document.getElementById('blink').addEventListener('change', e => {{
  blinkOn = e.target.checked;
}});
document.getElementById('play').addEventListener('click', e => {{
  if (timer) {{ clearInterval(timer); timer = null; e.target.textContent = '\\u25b6 play'; return; }}
  e.target.textContent = '\\u23f8 pause';
  timer = setInterval(() => {{
    let s = parseInt(slider.value, 10);
    if (blinkOn) {{
      slider.value = (s % 2 === 0) ? Math.min(100, s + 1) : Math.max(0, s - 1);
    }} else {{
      slider.value = s >= 100 ? 0 : s + 1;
    }}
    show();
  }}, 500);
}});
show();
</script>
</body>
</html>
"""


def viewer_html() -> str:
    imgs = []
    for s in range(101):
        imgs.append(
            f'<img data-s="{s}" data-m="gray" src="frames_gray/snow-{s:03d}.png" alt="">')
        imgs.append(
            f'<img data-s="{s}" data-m="bit" src="frames_1bit/snow-{s:03d}.png" alt="">')
    return VIEWER_HTML.format(imgs="\n".join(imgs))


def write_text(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8")
