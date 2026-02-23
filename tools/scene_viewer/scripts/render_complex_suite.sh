#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT_DIR="$ROOT_DIR/tools/scene_viewer/out/complex_suite"
MAP_DIR="$OUT_DIR/maps"
BUNDLE_DIR="$OUT_DIR/bundle"
RENDER_DIR="$OUT_DIR/renders"
DEBUG_DIR="$OUT_DIR/debug"
TMP_DIR="$OUT_DIR/tmp"

mkdir -p "$MAP_DIR" "$BUNDLE_DIR" "$RENDER_DIR" "$DEBUG_DIR" "$TMP_DIR"

WIDTH=600
HEIGHT=600
SEED=4242
BASE_SCENE="$TMP_DIR/shanshui_atkinson_seed_${SEED}.png"

# 1) Generate a complex base composition from the existing shanshui generator.
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/shanshui_preview/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  --out "$TMP_DIR" --count 1 --seed "$SEED" --width "$WIDTH" --height "$HEIGHT" --mode atkinson

# 2) Build map set used by scene_maker.
magick "$BASE_SCENE" \
  \( -size ${WIDTH}x${HEIGHT} plasma:fractal -colorspace Gray -blur 0x4 -level 30%,88% \) -compose Multiply -composite \
  \( -size ${WIDTH}x${HEIGHT} gradient:'#fcfcfc-#8f8f8f' -rotate 90 \) -compose Multiply -composite \
  -sigmoidal-contrast 5,56% "$MAP_DIR/albedo.png"

magick "$MAP_DIR/albedo.png" \
  \( -size ${WIDTH}x${HEIGHT} gradient:'#202020-#f8f8f8' \) -compose Screen -composite \
  -blur 0x7 -auto-level "$MAP_DIR/depth.png"

magick "$MAP_DIR/depth.png" -negate -auto-level -brightness-contrast 10x30 "$MAP_DIR/ao.png"

magick "$MAP_DIR/albedo.png" \
  \( "$MAP_DIR/depth.png" -edge 1 -blur 0x1 \) -compose Overlay -composite \
  -edge 1 -auto-level "$MAP_DIR/edge.png"

magick -size ${WIDTH}x${HEIGHT} radial-gradient:'#ffffff-#8a8a8a' \
  -colorspace Gray -sigmoidal-contrast 3,52% "$MAP_DIR/light.png"

magick -size ${WIDTH}x${HEIGHT} gradient:'#c8c8c8-#ffffff' -colorspace Gray "$MAP_DIR/mask.png"
magick -size ${WIDTH}x${HEIGHT} plasma:fractal -colorspace Gray -blur 0x0.8 "$MAP_DIR/stroke.png"

BUNDLE_PATH="$BUNDLE_DIR/complex_scene.scenebundle"

# 3) Pack maps into bundle.
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_maker/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  build --input "$MAP_DIR" --out "$BUNDLE_PATH" --strip-height 24 --compression rle

# 4) Render same scenery under different conditions (sun/fog/ink settings).
CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE_PATH" --out "$RENDER_DIR/scene_dawn_clear.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 192 --sun-azimuth-deg 35 --sun-elevation-deg 18 \
  --fog-strength 42 --edge-strength 164 --stroke-strength 64 --paper-strength 40 \
  --save-debug "$DEBUG_DIR/dawn_clear"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE_PATH" --out "$RENDER_DIR/scene_noon_clear.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 112 --sun-azimuth-deg 190 --sun-elevation-deg 72 \
  --fog-strength 24 --edge-strength 146 --stroke-strength 50 --paper-strength 30 \
  --save-debug "$DEBUG_DIR/noon_clear"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE_PATH" --out "$RENDER_DIR/scene_dusk_mist.png" \
  --preset sumi-e --mode gray3 --dither bayer4 \
  --sun-strength 220 --sun-azimuth-deg 302 --sun-elevation-deg 14 \
  --fog-strength 132 --edge-strength 176 --stroke-strength 72 --paper-strength 50 \
  --save-debug "$DEBUG_DIR/dusk_mist"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE_PATH" --out "$RENDER_DIR/scene_storm_fog.png" \
  --preset sumi-e --mode gray4 --dither bayer4 \
  --sun-strength 74 --sun-azimuth-deg 248 --sun-elevation-deg 27 \
  --fog-strength 190 --edge-strength 188 --stroke-strength 78 --paper-strength 60 \
  --save-debug "$DEBUG_DIR/storm_fog"

CARGO_ENCODED_RUSTFLAGS='' cargo +stable run \
  --manifest-path "$ROOT_DIR/tools/scene_viewer/Cargo.toml" \
  --target aarch64-apple-darwin -- \
  render --bundle "$BUNDLE_PATH" --out "$RENDER_DIR/scene_fast_mono1.png" \
  --preset sumi-e --mode mono1 --dither bayer4 \
  --sun-strength 180 --sun-azimuth-deg 330 --sun-elevation-deg 20 \
  --fog-strength 90 --edge-strength 190 --stroke-strength 66 --paper-strength 42 \
  --save-debug "$DEBUG_DIR/fast_mono1"

cat > "$OUT_DIR/manifest.txt" <<MANIFEST
scene bundle: $BUNDLE_PATH
maps: $MAP_DIR
renders:
  scene_dawn_clear.png
  scene_noon_clear.png
  scene_dusk_mist.png
  scene_storm_fog.png
  scene_fast_mono1.png
MANIFEST

echo "Complex suite generated in: $OUT_DIR"
