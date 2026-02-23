#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
MODEL="$ROOT/tools/scene_maker/assets/buddha_happy/happy_recon/happy_vrip_res2.ply"
OUT="$ROOT/tools/scene_viewer/out/buddha_blender"
MAPS="$OUT/maps"
BUNDLE_DIR="$OUT/bundle"
RENDERS="$OUT/renders"
DEBUG="$OUT/debug"
CYCLE="$RENDERS/cycle"
mkdir -p "$MAPS" "$BUNDLE_DIR" "$RENDERS" "$DEBUG" "$CYCLE"

if [ ! -f "$MODEL" ]; then
  mkdir -p "$ROOT/tools/scene_maker/assets/buddha_happy"
  curl -L 'http://graphics.stanford.edu/pub/3Dscanrep/happy/happy_recon.tar.gz' -o "$ROOT/tools/scene_maker/assets/buddha_happy/happy_recon.tar.gz"
  tar -xzf "$ROOT/tools/scene_maker/assets/buddha_happy/happy_recon.tar.gz" -C "$ROOT/tools/scene_maker/assets/buddha_happy"
fi

# Build and render a high-quality reference scene directly in Blender via MCP.
python3 "$ROOT/tools/scene_maker/scripts/setup_buddha_scene_via_blender_mcp.py" \
  --mesh "$MODEL" \
  --out-dir "$OUT" \
  --width 600 --height 600 \
  --samples-master 768 --samples-variants 512

# Bake compact map passes for the on-device compositor emulation.
python3 "$ROOT/tools/scene_maker/scripts/bake_ply_scene.py" \
  --mesh "$MODEL" \
  --out-dir "$MAPS" \
  --width 600 --height 600 \
  --yaw-deg 160 --pitch-deg -8 --distance 2.1 --fov-deg 44 --seed 1337

BUNDLE="$BUNDLE_DIR/buddha_3d.scenebundle"
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_maker/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  build --input "$MAPS" --out "$BUNDLE" --strip-height 24 --compression rle

# Master geometry render with minimal post-processing.
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE" --out "$RENDERS/master_scene_geometry_minimal_emulated.png" \
  --mode gray8 --dither none --tone-curve linear \
  --edge-strength 0 --fog-strength 0 --stroke-strength 0 --paper-strength 0 --sun-strength 0 \
  --save-debug "$DEBUG/master_scene_geometry_minimal_emulated"

# Stylized checkpoints against the same static camera scene.
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE" --out "$RENDERS/sumie_morning_emulated.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 150 --sun-azimuth-deg 115 --sun-elevation-deg 32 \
  --fog-strength 58 --edge-strength 168 --stroke-strength 66 --paper-strength 46 \
  --save-debug "$DEBUG/sumie_morning_emulated"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE" --out "$RENDERS/sumie_evening_emulated.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 195 --sun-azimuth-deg 255 --sun-elevation-deg 10 \
  --fog-strength 126 --edge-strength 182 --stroke-strength 74 --paper-strength 58 \
  --save-debug "$DEBUG/sumie_evening_emulated"

python3 - <<'PY' > "$OUT/cycle_params.tsv"
start_az, end_az = 115.0, 255.0
start_el, end_el = 32.0, 10.0
start_fog, end_fog = 58.0, 126.0
for i in range(9):
    t = i / 8.0
    az = start_az + (end_az - start_az) * t
    el = start_el + (end_el - start_el) * t
    fog = start_fog + (end_fog - start_fog) * t
    print(f"{i}\t{az:.2f}\t{el:.2f}\t{fog:.0f}")
PY

while IFS=$'\t' read -r idx az el fog; do
  frame="$(printf 'frame_%02d.png' "$idx")"
  CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
    --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
    --target aarch64-apple-darwin -- \
    render --bundle "$BUNDLE" --out "$CYCLE/$frame" \
    --preset sumi-e --mode gray3 --dither bayer4 \
    --sun-strength 176 --sun-azimuth-deg "$az" --sun-elevation-deg "$el" \
    --fog-strength "$fog" --edge-strength 176 --stroke-strength 70 --paper-strength 52
done < "$OUT/cycle_params.tsv"

cat > "$OUT/manifest.txt" <<MANIFEST
model_source: Stanford 3D Scanning Repository - Happy Buddha
model_file: $MODEL
camera: yaw=160 pitch=-8 distance=2.1 fov=44 (emulator maps)
blender_outputs:
  - renders/master_scene_geometry_minimal.png
  - renders/daylight_reference.png
  - renders/evening_reference.png
emulated_outputs:
  - renders/master_scene_geometry_minimal_emulated.png
  - renders/sumie_morning_emulated.png
  - renders/sumie_evening_emulated.png
  - renders/cycle/frame_00.png ... frame_08.png
bundle: $BUNDLE
MANIFEST

echo "Blender+viewer Buddha suite generated in: $OUT"
