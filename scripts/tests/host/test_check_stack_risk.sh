#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_rel="src/firmware/_stack_risk_test_fixture.rs"
fixture_path="$repo_root/$fixture_rel"

cleanup() {
  rm -f "$fixture_path"
}
trap cleanup EXIT

cat >"$fixture_path" <<'RUST'
pub fn stack_risk_fixture_violation() {
    let _buffer: [u8; 5000] = [0; 5000];
    let _ = _buffer.len();
}
RUST

if "$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null 2>&1; then
  echo "expected check_stack_risk.sh to fail for oversized local array" >&2
  exit 1
fi

STACK_RISK_LOCAL_ARRAY_MAX_BYTES=6000 "$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null

cat >"$fixture_path" <<'RUST'
pub fn stack_risk_fixture_reviewed() {
    let _buffer: [u8; 5000] = [0; 5000]; // stack-risk-reviewed
    let _ = _buffer.len();
}
RUST

"$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null

echo "stack-risk fixture tests passed"
