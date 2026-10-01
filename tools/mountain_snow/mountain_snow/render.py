"""Host renderer for the mountain-snow progression experiment.

For snow fraction ``s`` in 0..100: 0 and 100 return the rock / snow
endpoint byte-for-byte; intermediate values compare each eligible pixel's
ranked arrival position with the threshold for ``s``, mix registered rock
and snow samples across a narrow transition band, and dither the composed
luminance with the repository's stable blue-noise map.

Only stdlib + NumPy + Pillow; host-only, no firmware dependency.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

import numpy as np
from PIL import Image

BLUE_NOISE_SHA256 = (
    "275bcbefb4a58837a2d43ef00c8ba0c6daaf61b34db6c9ed72b1b239457e3d03"
)
BLUE_NOISE_REL = (
    "platform/ui/visuals/blue-noise/assets/blue-noise-600x600.bin"
)


def load_blue_noise(repo_root: Path) -> np.ndarray:
    """Load the canonical 600x600 blue-noise threshold map (row-major u8).

    Fails loudly on hash mismatch so previews can never silently use a
    regenerated or cropped map.
    """
    path = repo_root / BLUE_NOISE_REL
    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if digest != BLUE_NOISE_SHA256:
        raise ValueError(
            f"blue-noise map {path} sha256 {digest} != canonical "
            f"{BLUE_NOISE_SHA256}")
    if len(data) != 600 * 600:
        raise ValueError(f"blue-noise map {path} has {len(data)} bytes")
    return np.frombuffer(data, dtype=np.uint8).reshape(600, 600)


def read_rgba(path: Path) -> np.ndarray:
    with Image.open(path) as img:
        return np.array(img.convert("RGBA"))


def snow_weight(order_pos: np.ndarray, cutoff: int, half_band: int) -> np.ndarray:
    """Per-pixel snow blend weight in 0..1 (smoothstep across the band).

    Pixels fully arrived (``cutoff - pos >= half_band``) weigh 1, pixels
    fully unarrived weigh 0; the narrow band around the threshold mixes.
    Non-eligible pixels carry position -1 and always weigh 0.
    """
    d = (cutoff - order_pos.astype(np.float64)) / max(half_band, 1)
    w = np.clip((np.clip(d, -1.0, 1.0) + 1.0) / 2.0, 0.0, 1.0)
    w = w * w * (3.0 - 2.0 * w)
    w[order_pos < 0] = 0.0
    return w


def blend(
    rock: np.ndarray,
    snow: np.ndarray,
    order_index: np.ndarray,
    count: int,
    fraction: int,
    half_band: int,
) -> np.ndarray:
    """Compose the RGBA frame for integer snow percentage ``fraction``.

    Returns the rock endpoint array unchanged at 0 and the snow endpoint
    at 100 (callers may compare bytes directly). Alpha and luminance are
    interpolated together so an ink mark cannot appear through an
    unrelated opacity mask.
    """
    if fraction <= 0:
        return rock.copy()
    if fraction >= 100:
        return snow.copy()
    cutoff = round(fraction * count / 100)
    w = snow_weight(order_index, cutoff, half_band)[..., None]
    out = rock.astype(np.float64) * (1.0 - w) + snow.astype(np.float64) * w
    return np.clip(np.rint(out), 0, 255).astype(np.uint8)


def to_gray(rgba: np.ndarray) -> np.ndarray:
    """Grayscale preview: luminance composited over white paper."""
    rgb = rgba[..., :3].astype(np.float64)
    alpha = rgba[..., 3:4].astype(np.float64) / 255.0
    lum = 0.299 * rgb[..., 0] + 0.587 * rgb[..., 1] + 0.114 * rgb[..., 2]
    return np.clip(np.rint(lum * alpha[..., 0] + 255.0 * (1.0 - alpha[..., 0])),
                   0, 255).astype(np.uint8)


def to_1bit(rgba: np.ndarray, bluenose: np.ndarray) -> np.ndarray:
    """Stable one-bit preview using the repository blue-noise thresholds.

    The threshold field is fixed per canvas coordinate, so repeated
    renders of one percentage are byte-identical and the only change
    between steps is the material crossover itself.
    """
    gray = to_gray(rgba).astype(np.float64) / 255.0
    thresh = (bluenose.astype(np.float64) + 0.5) / 256.0
    return np.where(gray > thresh, 255, 0).astype(np.uint8)
