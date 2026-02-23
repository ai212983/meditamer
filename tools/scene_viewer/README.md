# scene_viewer

`scene_viewer` loads `.scenebundle` assets and emulates the on-device stylized compositor:

- tone: `albedo * light * ao`
- optional depth-derived re-lighting for sun direction testing
- depth fog blend toward paper white
- edge darkening
- stroke texture modulation
- paper grain modulation
- tone curve LUT
- quantization + dithering for `mono1`, `gray3`, `gray4`, or `gray8`

## Render
```bash
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path tools/scene_viewer/Cargo.toml \
  --target aarch64-apple-darwin -- \
  render --bundle /path/to/scene.scenebundle --out /path/to/render.png \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 180 --sun-azimuth-deg 35 --sun-elevation-deg 18 \
  --save-debug /path/to/debug
```

## Ghosting emulation
```bash
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path tools/scene_viewer/Cargo.toml \
  --target aarch64-apple-darwin -- \
  render --bundle /path/to/scene.scenebundle --out /path/to/render.png \
  --ghost-from /path/to/previous.png --ghost-alpha 32
```

## Inspect bundle
```bash
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path tools/scene_viewer/Cargo.toml \
  --target aarch64-apple-darwin -- \
  inspect --bundle /path/to/scene.scenebundle
```

## Complex suite (same scene, multiple conditions)
Run:
```bash
tools/scene_viewer/scripts/render_complex_suite.sh
```

Outputs are written to:
- `tools/scene_viewer/out/complex_suite/maps`
- `tools/scene_viewer/out/complex_suite/bundle`
- `tools/scene_viewer/out/complex_suite/renders`
- `tools/scene_viewer/out/complex_suite/debug`

## Buddha 3D model suite
Run:
```bash
tools/scene_viewer/scripts/render_buddha_3d_scene.sh
```

This uses the Stanford Happy Buddha reconstruction and writes outputs to:
- `tools/scene_viewer/out/buddha_3d/maps`
- `tools/scene_viewer/out/buddha_3d/bundle`
- `tools/scene_viewer/out/buddha_3d/renders`
- `tools/scene_viewer/out/buddha_3d/debug`

## Blender MCP Buddha suite (reference + emulation)
Run:
```bash
tools/scene_viewer/scripts/render_buddha_blender_scene.sh
```

This does two things in one output folder:
- Creates a high-quality Blender reference scene and renders `master/daylight/evening` images.
- Builds a `.scenebundle` from geometry maps and renders emulated sumi-e outputs, including a morning→evening cycle.

Outputs are written to:
- `tools/scene_viewer/out/buddha_blender/renders`
- `tools/scene_viewer/out/buddha_blender/bundle`
- `tools/scene_viewer/out/buddha_blender/debug`
