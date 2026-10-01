"""Ambient sky/sun SD packer (host-only).

Clips the sun sprite to its painted bounds, packs the full-frame sky and
the clipped sun as MSB-first 1-bit planes, and writes a review sheet so
the sky/sun composition can be checked visually before anything is flashed.

Layout of SKY.BIN: a 64-byte header followed by sky ink bits
(600x600/8), sun ink bits (stride*sun_h), and sun mask bits
(stride*sun_h), where stride = ceil(sun_w/8) bytes per row.

Header (all integers little-endian):
  magic[8] = b"AMBSKY01", sky_w u16, sky_h u16, sun_w u16, sun_h u16,
  anchor_x f32, anchor_y f32 (mask centroid in crop pixels),
  sky_len u32, sun_len u32 (ink bytes == mask bytes), payload_crc u32
  (IEEE CRC32 over the payload), then zero reserved bytes to 64.
"""

import struct
import zlib

MAGIC = b"AMBSKY01"
HEADER_LEN = 64
SKY_W = 600
SKY_H = 600
SKY_LEN = SKY_W * SKY_H // 8

# Sun path from AmbientHomeConfig::DEFAULT, resolved against the 600x600
# surface (products/meditamer ambient_view/model.rs). The preview draws
# the same one-pixel arc guide as the firmware composer.
SURFACE = 600.0
ARC_START = (-0.08 * SURFACE, 378.0)
CONTROL_1 = (0.175 * SURFACE, 15.0)
CONTROL_2 = (0.825 * SURFACE, 15.0)
ARC_END = (1.08 * SURFACE, 378.0)

REVIEW_FRACTIONS = (0.25, 0.375, 0.5, 0.625, 0.75)


def point_on_curve(t):
    """Cubic Bezier point; mirrors model::point_on_curve (t clamped)."""
    t = min(1.0, max(0.0, t))
    mt = 1.0 - t
    a, b, c, d = mt**3, 3 * mt * mt * t, 3 * mt * t * t, t**3
    consts = (ARC_START, CONTROL_1, CONTROL_2, ARC_END)
    return (
        a * consts[0][0] + b * consts[1][0] + c * consts[2][0] + d * consts[3][0],
        a * consts[0][1] + b * consts[1][1] + c * consts[2][1] + d * consts[3][1],
    )


def pack_bits(ink_rows):
    """Pack rows of 0/1 ints to MSB-first bytes; rows share one stride."""
    width = len(ink_rows[0])
    stride = (width + 7) // 8
    out = bytearray()
    for row in ink_rows:
        assert len(row) == width
        for i in range(stride):
            byte = 0
            for bit in range(8):
                x = i * 8 + bit
                if x < width and row[x]:
                    byte |= 0x80 >> bit
            out.append(byte)
    return bytes(out)


def unpack_bits(blob, width, height):
    """Inverse of pack_bits: 1 = ink."""
    import numpy as np

    stride = (width + 7) // 8
    assert len(blob) == stride * height
    out = np.zeros((height, width), dtype=bool)
    for y in range(height):
        for i in range(stride):
            byte = blob[y * stride + i]
            for bit in range(8):
                x = i * 8 + bit
                if x < width and byte & (0x80 >> bit):
                    out[y, x] = True
    return out


def clip_sun(gray, alpha):
    """Clip the sun to its nonzero-alpha bbox.

    Returns (ink, mask, bbox, anchor) where ink/mask are 2D bool arrays,
    bbox is (x0, y0, w, h), and anchor is the opaque-mask centroid in
    crop coordinates. Raises ValueError on empty or absurd geometry.
    """
    import numpy as np

    assert gray.shape == alpha.shape
    ys, xs = np.where(alpha > 0)
    if len(xs) == 0:
        raise ValueError("sun has no painted pixels")
    x0, x1 = int(xs.min()), int(xs.max())
    y0, y1 = int(ys.min()), int(ys.max())
    w, h = x1 - x0 + 1, y1 - y0 + 1
    if not (32 <= w <= 300 and 32 <= h <= 300):
        raise ValueError(f"sun bbox {w}x{h} outside 32..300px")
    crop_gray = gray[y0 : y1 + 1, x0 : x1 + 1]
    crop_alpha = alpha[y0 : y1 + 1, x0 : x1 + 1]
    mask = crop_alpha > 127
    if not mask.any():
        raise ValueError("sun mask empty after threshold")
    ink = (crop_gray < 128) & mask
    cy, cx = np.where(mask)
    anchor = (float(cx.mean()), float(cy.mean()))
    return ink, mask, (x0, y0, w, h), anchor


def encode_pack(sky_ink, sun_ink, sun_mask, anchor):
    """Build the SKY.BIN file bytes from bool arrays."""
    import numpy as np

    assert sky_ink.shape == (SKY_H, SKY_W)
    assert sun_ink.shape == sun_mask.shape
    sun_h, sun_w = sun_ink.shape
    if not (1 <= sun_w <= 256 and 1 <= sun_h <= 256):
        raise ValueError(f"sun {sun_w}x{sun_h} exceeds 256px header range")
    sky_blob = pack_bits(sky_ink.astype(int).tolist())
    sun_blob = pack_bits(sun_ink.astype(int).tolist())
    mask_blob = pack_bits(sun_mask.astype(int).tolist())
    assert len(sky_blob) == SKY_LEN
    assert len(sun_blob) == len(mask_blob)
    payload = sky_blob + sun_blob + mask_blob
    header = bytearray(HEADER_LEN)
    header[0:8] = MAGIC
    struct.pack_into(
        "<HHHHffIII",
        header,
        8,
        SKY_W,
        SKY_H,
        sun_w,
        sun_h,
        float(anchor[0]),
        float(anchor[1]),
        len(sky_blob),
        len(sun_blob),
        zlib.crc32(payload) & 0xFFFFFFFF,
    )
    return bytes(header) + payload


def decode_pack(data):
    """Validate and split a SKY.BIN file; returns a dict of views."""
    if len(data) < HEADER_LEN:
        raise ValueError("file shorter than header")
    header = data[:HEADER_LEN]
    if bytes(header[0:8]) != MAGIC:
        raise ValueError("bad magic")
    sky_w, sky_h, sun_w, sun_h, ax, ay, sky_len, sun_len, want_crc = struct.unpack(
        "<HHHHffIII", header[8:36]
    )
    if any(b != 0 for b in header[36:]):
        raise ValueError("reserved bytes nonzero")
    if (sky_w, sky_h) != (SKY_W, SKY_H):
        raise ValueError(f"sky {sky_w}x{sky_h} != 600x600")
    if not (1 <= sun_w <= 256 and 1 <= sun_h <= 256):
        raise ValueError(f"sun {sun_w}x{sun_h} out of range")
    stride = (sun_w + 7) // 8
    if sky_len != SKY_LEN or sun_len != stride * sun_h:
        raise ValueError("length fields mismatch geometry")
    payload = data[HEADER_LEN:]
    if len(payload) != sky_len + 2 * sun_len:
        raise ValueError("file size mismatch")
    if zlib.crc32(payload) & 0xFFFFFFFF != want_crc:
        raise ValueError("crc mismatch")
    return {
        "sun_w": sun_w,
        "sun_h": sun_h,
        "anchor": (ax, ay),
        "sky": payload[:sky_len],
        "sun_ink": payload[sky_len : sky_len + sun_len],
        "sun_mask": payload[sky_len + sun_len :],
    }
