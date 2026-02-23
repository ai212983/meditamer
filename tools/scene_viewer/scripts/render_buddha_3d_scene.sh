#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
MODEL_DIR="$ROOT/tools/scene_maker/assets/buddha_happy"
MODEL_RECON_DIR="$MODEL_DIR/happy_recon"
MODEL="$MODEL_RECON_DIR/happy_vrip_res2.ply"

OUT="$ROOT/tools/scene_viewer/out/buddha_3d"
MAPS="$OUT/maps"
BUNDLE_DIR="$OUT/bundle"
RENDERS="$OUT/renders"
DEBUG="$OUT/debug"
mkdir -p "$MODEL_DIR" "$MAPS" "$BUNDLE_DIR" "$RENDERS" "$DEBUG"

if [ ! -f "$MODEL" ]; then
  curl -L 'http://graphics.stanford.edu/pub/3Dscanrep/happy/happy_recon.tar.gz' -o "$MODEL_DIR/happy_recon.tar.gz"
  tar -xzf "$MODEL_DIR/happy_recon.tar.gz" -C "$MODEL_DIR"
fi

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
  render --bundle "$BUNDLE" --out "$RENDERS/master_scene_geometry_minimal.png" \
  --mode gray8 --dither none --tone-curve linear \
  --edge-strength 0 --fog-strength 0 --stroke-strength 0 --paper-strength 0 --sun-strength 0 \
  --save-debug "$DEBUG/master_scene_geometry_minimal"

# Sumi-e variants from same 3D scene under different conditions.
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE" --out "$RENDERS/sumie_daylight.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 150 --sun-azimuth-deg 35 --sun-elevation-deg 25 \
  --fog-strength 54 --edge-strength 164 --stroke-strength 62 --paper-strength 44 \
  --save-debug "$DEBUG/sumie_daylight"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE" --out "$RENDERS/sumie_misty.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 214 --sun-azimuth-deg 298 --sun-elevation-deg 13 \
  --fog-strength 142 --edge-strength 178 --stroke-strength 72 --paper-strength 54 \
  --save-debug "$DEBUG/sumie_misty"

cat > "$OUT/manifest.txt" <<MANIFEST
model_source: Stanford 3D Scanning Repository - Happy Buddha
model_file: $MODEL
camera: yaw=160 pitch=-8 distance=2.1 fov=44
bundle: $BUNDLE
renders:
  master_scene_geometry_minimal.png
  sumie_daylight.png
  sumie_misty.png
MANIFEST

echo "3D Buddha scene generated in: $OUT"
