#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
MANIFEST_PATH="$ROOT_DIR/tools/touch_replay/Cargo.toml"
FIXTURES_DIR="$ROOT_DIR/tools/touch_replay/fixtures"

run_case() {
  local trace_file="$1"
  local expected_file="$2"
  RUSTFLAGS='' cargo +stable-aarch64-apple-darwin run \
    --target aarch64-apple-darwin \
    --manifest-path "$MANIFEST_PATH" -- \
    "$FIXTURES_DIR/$trace_file" \
    --expect "$FIXTURES_DIR/$expected_file"
}

run_case tap_trace.csv tap_expected.txt
run_case swipe_right_trace.csv swipe_right_expected.txt
run_case multitouch_cancel_trace.csv multitouch_cancel_expected.txt
