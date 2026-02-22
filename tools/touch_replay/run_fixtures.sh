#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
MANIFEST_PATH="$ROOT_DIR/tools/touch_replay/Cargo.toml"
FIXTURES_DIR="$ROOT_DIR/tools/touch_replay/fixtures"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"

if [[ "${1:-}" != "" && "${1:-}" != --* ]]; then
  host_target="$1"
  shift
else
  host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
fi
if [[ -z "$host_target" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi

run_case() {
  local trace_file="$1"
  local expected_file="$2"
  RUSTUP_TOOLCHAIN="$toolchain" cargo run \
    --locked \
    --target "$host_target" \
    --manifest-path "$MANIFEST_PATH" -- \
    "$FIXTURES_DIR/$trace_file" \
    --expect "$FIXTURES_DIR/$expected_file"
}

(
  cd /tmp
  run_case tap_trace.csv tap_expected.txt
  run_case diagonal_drag_no_swipe_trace.csv diagonal_drag_no_swipe_expected.txt
  run_case swipe_left_trace.csv swipe_left_expected.txt
  run_case swipe_right_trace.csv swipe_right_expected.txt
  run_case swipe_up_trace.csv swipe_up_expected.txt
  run_case swipe_down_trace.csv swipe_down_expected.txt
  run_case multitouch_cancel_trace.csv multitouch_cancel_expected.txt
)
