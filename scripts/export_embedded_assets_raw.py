#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SRC_DIR = REPO_ROOT / "src"
DEFAULT_OUT = REPO_ROOT / "assets" / "raw"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export embedded font/blue-noise assets into raw files."
    )
    parser.add_argument(
        "--out",
        default=str(DEFAULT_OUT),
        help=f"Output root (default: {DEFAULT_OUT})",
    )
    return parser.parse_args()


def write_bytes(path: Path, data: bytes) -> dict[str, object]:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return {
        "path": str(path.relative_to(REPO_ROOT)),
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def parse_u8_array_blob(blob: str) -> bytes:
    values: list[int] = []
    for token in re.findall(r"\d+", blob):
        value = int(token, 10)
        if value < 0 or value > 255:
            raise ValueError(f"u8 out of range: {value}")
        values.append(value)
    return bytes(values)


def export_blue_noise(out_root: Path) -> dict[str, object]:
    source_rs = (SRC_DIR / "suminagashi_blue_noise.rs").read_text(encoding="utf-8")
    array_match = re.search(
        r"pub const BLUE_NOISE_32X32:\s*\[u8;[^\]]+\]\s*=\s*\[(.*?)\];",
        source_rs,
        re.DOTALL,
    )
    if not array_match:
        raise RuntimeError("failed to parse BLUE_NOISE_32X32 array")
    blue_noise_32 = parse_u8_array_blob(array_match.group(1))
    if len(blue_noise_32) != 32 * 32:
        raise RuntimeError(f"unexpected BLUE_NOISE_32X32 length: {len(blue_noise_32)}")

    out = {"entries": []}
    out["entries"].append(
        {
            **write_bytes(
                out_root / "blue_noise" / "blue_noise_32x32_u8.raw", blue_noise_32
            ),
            "width": 32,
            "height": 32,
            "format": "u8-threshold",
        }
    )

    blue_noise_600_src = SRC_DIR / "suminagashi_blue_noise_600.bin"
    blue_noise_600 = blue_noise_600_src.read_bytes()
    if len(blue_noise_600) != 600 * 600:
        raise RuntimeError(f"unexpected blue_noise_600 size: {len(blue_noise_600)}")
    out["entries"].append(
        {
            **write_bytes(
                out_root / "blue_noise" / "blue_noise_600x600_u8.raw", blue_noise_600
            ),
            "width": 600,
            "height": 600,
            "format": "u8-threshold",
        }
    )
    return out


def export_pirata_font(out_root: Path) -> dict[str, object]:
    source = (SRC_DIR / "pirata_clock_font.rs").read_text(encoding="utf-8")

    data_matches = re.finditer(
        r"const PIRATA_([A-Z0-9_]+)_DATA:\s*\[u8;\s*(\d+)\s*\]\s*=\s*\[(.*?)\];",
        source,
        re.DOTALL,
    )
    data_by_key: dict[str, bytes] = {}
    for match in data_matches:
        key = match.group(1)
        declared_len = int(match.group(2), 10)
        payload = parse_u8_array_blob(match.group(3))
        if len(payload) != declared_len:
            raise RuntimeError(
                f"PIRATA_{key}_DATA length mismatch: declared {declared_len} actual {len(payload)}"
            )
        data_by_key[key] = payload

    glyph_matches = re.finditer(
        r"pub const PIRATA_([A-Z0-9_]+):\s*BitmapGlyph\s*=\s*BitmapGlyph\s*\{\s*"
        r"width:\s*(\d+),\s*height:\s*(\d+),\s*data:\s*&PIRATA_([A-Z0-9_]+)_DATA,\s*\};",
        source,
        re.DOTALL,
    )

    entries: list[dict[str, object]] = []
    for match in glyph_matches:
        key = match.group(1)
        width = int(match.group(2), 10)
        height = int(match.group(3), 10)
        data_key = match.group(4)
        if key != data_key:
            raise RuntimeError(
                f"glyph/data key mismatch for PIRATA_{key}: data points to {data_key}"
            )
        payload = data_by_key.get(key)
        if payload is None:
            raise RuntimeError(f"missing payload for PIRATA_{key}")

        glyph_name = ":" if key == "COLON" else key.removeprefix("D")
        file_key = "colon" if glyph_name == ":" else f"digit_{glyph_name}"
        out_path = out_root / "fonts" / "pirata_clock" / f"{file_key}_mono1.raw"
        entry = {
            **write_bytes(out_path, payload),
            "glyph": glyph_name,
            "width": width,
            "height": height,
            "format": "mono1-row-major-lsb",
        }
        entries.append(entry)

    entries.sort(key=lambda item: str(item["glyph"]))
    if len(entries) != 11:
        raise RuntimeError(f"expected 11 pirata glyphs (0-9 and colon), got {len(entries)}")

    return {
        "entries": entries,
        "spacing_px": parse_pirata_spacing(source),
    }


def parse_pirata_spacing(source: str) -> int:
    match = re.search(r"pub const PIRATA_TIME_SPACING:\s*i32\s*=\s*(-?\d+);", source)
    if not match:
        raise RuntimeError("failed to parse PIRATA_TIME_SPACING")
    return int(match.group(1), 10)


def main() -> int:
    args = parse_args()
    out_root = Path(args.out).resolve()
    out_root.mkdir(parents=True, exist_ok=True)

    # Keep the output deterministic and remove stale generated files.
    if out_root.exists():
        for child in out_root.iterdir():
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()

    manifest = {
        "source": "embedded",
        "blue_noise": export_blue_noise(out_root),
        "fonts": {"pirata_clock": export_pirata_font(out_root)},
    }

    manifest_path = out_root / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"Generated {manifest_path.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
