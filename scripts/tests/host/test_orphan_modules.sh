#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p \
  "$fixture_root/src/platform" \
  "$fixture_root/tests/snapshots" \
  "$fixture_root/tests" \
  "$fixture_root/tools/helper/src"

cat >"$fixture_root/Cargo.toml" <<'TOML'
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"
TOML
cat >"$fixture_root/src/lib.rs" <<'RUST'
mod live;
#[cfg(feature = "full")]
#[path = "platform/full.rs"]
mod platform;
#[cfg(not(feature = "full"))]
#[path = "platform/stub.rs"]
mod platform;
include!("included.rs");
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
RUST
printf '%s\n' 'pub fn live() {}' >"$fixture_root/src/live.rs"
printf '%s\n' 'pub fn full() {}' >"$fixture_root/src/platform/full.rs"
printf '%s\n' 'pub fn stub() {}' >"$fixture_root/src/platform/stub.rs"
printf '%s\n' 'pub fn included() {}' >"$fixture_root/src/included.rs"
printf '%s\n' 'pub fn orphan() {}' >"$fixture_root/src/orphan.rs"
printf '%s\n' '#[test] fn integration_root() {}' >"$fixture_root/tests/smoke.rs"
printf '%s\n' 'snapshot text with a Rust suffix' >"$fixture_root/tests/snapshots/golden.rs"

cat >"$fixture_root/tools/helper/Cargo.toml" <<'TOML'
[package]
name = "helper"
version = "0.1.0"
edition = "2021"
TOML
cat >"$fixture_root/tools/helper/src/main.rs" <<'RUST'
mod support;
fn main() {}
RUST
printf '%s\n' 'pub fn support() {}' >"$fixture_root/tools/helper/src/support.rs"

git -C "$fixture_root" init -q
git -C "$fixture_root" add .

if (cd "$fixture_root" && "$repo_root/scripts/ci/check_orphan_modules.py") \
  >"$fixture_root/orphan.out" 2>&1; then
  echo "expected orphan checker to reject an unreachable module" >&2
  exit 1
fi
grep -q 'src/orphan.rs' "$fixture_root/orphan.out"
if grep -q 'platform/full.rs\|platform/stub.rs\|tests/smoke.rs\|tests/snapshots/golden.rs\|tools/helper/src/support.rs' \
  "$fixture_root/orphan.out"; then
  echo "reachable cfg, integration-test, or nested-package source was misclassified" >&2
  exit 1
fi

python3 - "$fixture_root/src/lib.rs" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
path.write_text(path.read_text() + "mod orphan;\n")
PY
git -C "$fixture_root" add src/lib.rs

(cd "$fixture_root" && "$repo_root/scripts/ci/check_orphan_modules.py") >/dev/null

# Default Git-root discovery must resolve correctly from a nested directory
# within the fixture repo, not just from its root.
nested_out="$(cd "$fixture_root/tools/helper/src" && "$repo_root/scripts/ci/check_orphan_modules.py")"
if [[ "$nested_out" != *"zero unreachable tracked Rust files"* ]]; then
  echo "expected nested-directory invocation to discover the fixture repo root" >&2
  exit 1
fi

# Outside any Git checkout, default discovery must fail closed (not silently
# fall back to some other root), while an explicit --repo-root still works --
# the escape hatch fixtures (and this self-test) depend on.
outside_git_dir="$(mktemp -d)"
trap 'rm -rf "$fixture_root" "$outside_git_dir"' EXIT
if (cd "$outside_git_dir" && "$repo_root/scripts/ci/check_orphan_modules.py") \
  >"$outside_git_dir/no-repo-root.out" 2>&1; then
  echo "expected default discovery to fail outside any Git checkout" >&2
  exit 1
fi
grep -q 'must run inside a git work tree' "$outside_git_dir/no-repo-root.out"

(cd "$outside_git_dir" && "$repo_root/scripts/ci/check_orphan_modules.py" --repo-root "$fixture_root") >/dev/null

# Index mode must traverse index blobs even when an unstaged worktree edit
# appears to repair reachability.
printf '%s\n' 'pub fn index_only_orphan() {}' >"$fixture_root/src/index_only_orphan.rs"
git -C "$fixture_root" add src/index_only_orphan.rs
printf '%s\n' 'mod index_only_orphan;' >>"$fixture_root/src/lib.rs"

if (cd "$fixture_root" && "$repo_root/scripts/ci/check_orphan_modules.py" --staged) \
  >"$fixture_root/staged-orphan.out" 2>&1; then
  echo "expected staged orphan checker to reject index-only orphan" >&2
  exit 1
fi
grep -q 'src/index_only_orphan.rs' "$fixture_root/staged-orphan.out"

(cd "$fixture_root" && "$repo_root/scripts/ci/check_orphan_modules.py") >/dev/null

echo "orphan-module fixture tests passed"
