#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

FONT_PATH = Path.home() / "Library/Fonts/PirataOne-Regular.ttf"
OUT_RS = Path("/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/pirata_clock_font.rs")
TARGET_MAX_HEIGHT = 200
CHARS = "0123456789:"
PADDING_X = 6
PADDING_Y = 6
SPACING = 3

if not FONT_PATH.exists():
    raise SystemExit(f"Font not found: {FONT_PATH}")


def max_glyph_height(size: int) -> int:
    font = ImageFont.truetype(str(FONT_PATH), size=size)
    ascent, descent = font.getmetrics()
    canvas_h = ascent + descent + 2 * PADDING_Y
    img = Image.new("L", (512, canvas_h), 0)
    draw = ImageDraw.Draw(img)
    baseline = PADDING_Y + ascent
    max_h = 0
    for ch in CHARS:
        bbox = draw.textbbox((0, baseline), ch, font=font, anchor="ls")
        h = bbox[3] - bbox[1]
        max_h = max(max_h, h)
    return max_h


def pick_size() -> int:
    lo, hi = 20, 600
    best = lo
    while lo <= hi:
        mid = (lo + hi) // 2
        h = max_glyph_height(mid)
        if h <= TARGET_MAX_HEIGHT:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    return best


size = pick_size()
font = ImageFont.truetype(str(FONT_PATH), size=size)
ascent, descent = font.getmetrics()
cell_h = ascent + descent + 2 * PADDING_Y
baseline = PADDING_Y + ascent

# Measure glyph widths/bboxes first.
probe = Image.new("L", (512, cell_h), 0)
draw = ImageDraw.Draw(probe)
measures: dict[str, tuple[int, int, int, int]] = {}
max_w = 0
max_h = 0
for ch in CHARS:
    bbox = draw.textbbox((0, baseline), ch, font=font, anchor="ls")
    l, t, r, b = bbox
    w = r - l
    h = b - t
    measures[ch] = (l, t, w, h)
    max_w = max(max_w, w)
    max_h = max(max_h, h)

cell_w = max_w + 2 * PADDING_X


def pack_bits(im: Image.Image) -> tuple[int, int, list[int]]:
    w, h = im.size
    px = im.load()
    bpr = (w + 7) // 8
    out = [0] * (bpr * h)
    for y in range(h):
        row = y * bpr
        for x in range(w):
            if px[x, y] > 0:
                out[row + (x // 8)] |= 1 << (x % 8)
    return w, h, out


glyph_data: dict[str, tuple[int, int, list[int]]] = {}
for ch in CHARS:
    l, _t, w, _h = measures[ch]
    canvas = Image.new("L", (cell_w, cell_h), 0)
    d = ImageDraw.Draw(canvas)
    x = (cell_w - w) // 2 - l
    d.text((x, baseline), ch, font=font, anchor="ls", fill=255)

    # Trim empty rows/columns to get proportional glyph widths.
    bbox = canvas.getbbox()
    if bbox is None:
        trimmed = canvas
    else:
        left = max(0, bbox[0] - 1)
        top = max(0, bbox[1])
        right = min(cell_w, bbox[2] + 1)
        bottom = min(cell_h, bbox[3])
        trimmed = canvas.crop((left, top, right, bottom))

    # Convert to binary.
    bw = trimmed.point(lambda p: 255 if p >= 128 else 0, mode="1").convert("1")
    glyph_data[ch] = pack_bits(bw)

# Build Rust source.
lines: list[str] = []
lines.append("use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};")
lines.append("")
lines.append("pub const PIRATA_TIME_SPACING: i32 = %d;" % SPACING)
lines.append("")
lines.append("pub struct BitmapGlyph {")
lines.append("    pub width: u16,")
lines.append("    pub height: u16,")
lines.append("    pub data: &'static [u8],")
lines.append("}")
lines.append("")

for ch in CHARS:
    key = "COLON" if ch == ":" else f"D{ch}"
    w, h, data = glyph_data[ch]
    arr = ",".join(str(b) for b in data)
    lines.append(f"const PIRATA_{key}_DATA: [u8; {len(data)}] = [{arr}];")
    lines.append("")
    lines.append(f"pub const PIRATA_{key}: BitmapGlyph = BitmapGlyph {{")
    lines.append(f"    width: {w},")
    lines.append(f"    height: {h},")
    lines.append(f"    data: &PIRATA_{key}_DATA,")
    lines.append("};")
    lines.append("")

lines.append("pub fn glyph_for(ch: char) -> Option<&'static BitmapGlyph> {")
lines.append("    match ch {")
for ch in CHARS:
    key = "COLON" if ch == ":" else f"D{ch}"
    lines.append(f"        '{ch}' => Some(&PIRATA_{key}),")
lines.append("        _ => None,")
lines.append("    }")
lines.append("}")
lines.append("")
lines.append("pub fn draw_time_centered<T>(display: &mut T, text: &str, center: Point) where T: DrawTarget<Color = BinaryColor> {")
lines.append("    let mut total_w = 0i32;")
lines.append("    let mut max_h = 0i32;")
lines.append("    let mut glyphs: [Option<&BitmapGlyph>; 5] = [None, None, None, None, None];")
lines.append("    let mut n = 0usize;")
lines.append("    for ch in text.chars() {")
lines.append("        if let Some(g) = glyph_for(ch) {")
lines.append("            if n < glyphs.len() {")
lines.append("                glyphs[n] = Some(g);")
lines.append("                n += 1;")
lines.append("                total_w += g.width as i32;")
lines.append("                max_h = max_h.max(g.height as i32);")
lines.append("            }")
lines.append("        }")
lines.append("    }")
lines.append("    if n == 0 { return; }")
lines.append("    total_w += (n as i32 - 1) * PIRATA_TIME_SPACING;")
lines.append("    let mut x = center.x - total_w / 2;")
lines.append("    let top = center.y - max_h / 2;")
lines.append("    for i in 0..n {")
lines.append("        let g = glyphs[i].unwrap();")
lines.append("        draw_glyph(display, g, Point::new(x, top + (max_h - g.height as i32) / 2));")
lines.append("        x += g.width as i32 + PIRATA_TIME_SPACING;")
lines.append("    }")
lines.append("}")
lines.append("")
lines.append("fn draw_glyph<T>(display: &mut T, glyph: &BitmapGlyph, top_left: Point) where T: DrawTarget<Color = BinaryColor> {")
lines.append("    let bpr = (glyph.width as usize + 7) / 8;")
lines.append("    for y in 0..glyph.height as i32 {")
lines.append("        let row = y as usize * bpr;")
lines.append("        for x in 0..glyph.width as i32 {")
lines.append("            let b = glyph.data[row + (x as usize / 8)];")
lines.append("            if (b & (1 << (x as usize % 8))) != 0 {")
lines.append("                let _ = display.draw_iter(core::iter::once(Pixel(Point::new(top_left.x + x, top_left.y + y), BinaryColor::On)));")
lines.append("            }")
lines.append("        }")
lines.append("    }")
lines.append("}")

OUT_RS.write_text("\n".join(lines) + "\n", encoding="ascii")
print(f"Generated {OUT_RS}")
print(f"PirataOne size={size}, max_glyph_height={max_h}, cell_w={cell_w}, cell_h={cell_h}")
