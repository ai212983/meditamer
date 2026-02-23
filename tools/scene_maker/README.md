# scene_maker

`scene_maker` packs pre-baked grayscale map passes into one strip-major `.scenebundle` file for fast streaming/compositing.

## 3D mesh baking (PLY)
For mesh-driven inputs (instead of photos), use:
```bash
python3 tools/scene_maker/scripts/bake_ply_scene.py \
  --mesh /path/to/model.ply \
  --out-dir /path/to/maps \
  --width 600 --height 600
```

This generates the required map set (`albedo/light/ao/depth/edge/mask/stroke`) from 3D geometry.

## Blender MCP scene setup (quality reference)
To build a high-quality Buddha scene directly in a running Blender instance (with Blender MCP addon enabled):
```bash
python3 tools/scene_maker/scripts/setup_buddha_scene_via_blender_mcp.py
```

Outputs:
- `tools/scene_viewer/out/buddha_blender/renders/master_scene_geometry_minimal.png`
- `tools/scene_viewer/out/buddha_blender/renders/daylight_reference.png`
- `tools/scene_viewer/out/buddha_blender/renders/evening_reference.png`
- `tools/scene_viewer/out/buddha_blender/blender/buddha_scene.blend`

This script configures a static-camera 3D Buddha scene with ground, sky, fog, and shadows, plus morning-to-evening keyframes for daylight direction.

## Supported input maps
Place PNG files in an input directory (default `tools/scene_maker/input`):

- `albedo.png` (required)
- `light.png` (required)
- `ao.png` (optional, defaults to white)
- `depth.png` (optional, defaults to 0)
- `edge.png` (optional, auto-derived if missing)
- `mask.png` (optional, defaults to white)
- `stroke.png` (optional, defaults to neutral 128)

## Build bundle
```bash
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path tools/scene_maker/Cargo.toml \
  --target aarch64-apple-darwin -- \
  build --input /path/to/maps --out /path/to/scene.scenebundle
```

Important: this repo has ESP32 global rustflags in `.cargo/config.toml`; the `CARGO_ENCODED_RUSTFLAGS=''` prefix is needed for host desktop tools.

## Inspect bundle
```bash
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path tools/scene_maker/Cargo.toml \
  --target aarch64-apple-darwin -- \
  inspect --bundle /path/to/scene.scenebundle
```

## Bundle format
See `tools/scene_maker/docs/scene_bundle_format.md`.
