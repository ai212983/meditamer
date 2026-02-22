#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
importer="$repo_root/tools/touch_replay/import_touch_log.py"
manifest_path="$repo_root/tools/touch_replay/Cargo.toml"
fixtures_dir="$repo_root/tools/touch_replay/fixtures"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"

if [[ $# -lt 2 ]]; then
  echo "usage: scripts/make_touch_fixture.sh <capture.log> <fixture_name> [--keep-absolute-time]" >&2
  exit 2
fi

capture_path="$1"
fixture_name="$2"
shift 2

if [[ ! "$fixture_name" =~ ^[a-z0-9_]+$ ]]; then
  echo "fixture_name must match ^[a-z0-9_]+$" >&2
  exit 2
fi

if [[ ! -f "$capture_path" ]]; then
  echo "capture log not found: $capture_path" >&2
  exit 2
fi

if [[ "${1:-}" != "" && "${1:-}" != "--keep-absolute-time" ]]; then
  echo "unknown option: ${1:-}" >&2
  exit 2
fi

if [[ "${1:-}" == "--keep-absolute-time" ]]; then
  normalize_arg=("--keep-absolute-time")
else
  normalize_arg=()
fi

host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
if [[ -z "$host_target" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi

mkdir -p "$fixtures_dir"
trace_csv="$fixtures_dir/${fixture_name}_trace.csv"
expected_txt="$fixtures_dir/${fixture_name}_expected.txt"
tmp_device_expected="$(mktemp -t touch_device_expected.XXXXXX)"

if [[ ${#normalize_arg[@]} -gt 0 ]]; then
  "$importer" \
    "$capture_path" \
    "$trace_csv" \
    "${normalize_arg[@]}" \
    --events-output "$tmp_device_expected"
else
  "$importer" "$capture_path" "$trace_csv" --events-output "$tmp_device_expected"
fi

tmp_events="$(mktemp -t touch_replay_events.XXXXXX)"
trap 'rm -f "$tmp_events" "$tmp_device_expected"' EXIT

if [[ -s "$tmp_device_expected" ]]; then
  cp "$tmp_device_expected" "$expected_txt"
  echo "used decoded touch_event stream for expected kinds -> $expected_txt" >&2
else
  echo "no touch_event rows found; deriving expected kinds from replay output" >&2

  (
    cd /tmp
    RUSTFLAGS='' RUSTUP_TOOLCHAIN="$toolchain" cargo run \
      --locked \
      --target "$host_target" \
      --manifest-path "$manifest_path" -- \
      "$trace_csv" \
      >"$tmp_events"
  )

  awk -F, '/^event,/{print $3}' "$tmp_events" >"$expected_txt"
fi

if [[ ! -s "$expected_txt" ]]; then
  echo "warning: expected event file is empty: $expected_txt" >&2
else
  echo "wrote expected kinds -> $expected_txt" >&2
fi

echo "trace fixture:    $trace_csv"
echo "expected fixture: $expected_txt"
echo "review and edit $expected_txt if this capture includes noisy segments"
