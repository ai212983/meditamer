#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p "$fixture_root/src" "$fixture_root/tools/scene_fixture/src"
cat >"$fixture_root/src/lib.rs" <<'RUST'
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
RUST
cat >"$fixture_root/tools/scene_fixture/src/main.rs" <<'RUST'
include!("manual.rs");
fn main() {}
RUST
printf '%s\n' 'pub fn manual() {}' >"$fixture_root/tools/scene_fixture/src/manual.rs"

git -C "$fixture_root" init -q
git -C "$fixture_root" add .

(cd "$fixture_root" && "$repo_root/scripts/ci/check_include_usage.sh") \
  >"$fixture_root/advisory.out"
grep -q '1 generated include(s) OK' "$fixture_root/advisory.out"
grep -q 'include! of hand-written source (1)' "$fixture_root/advisory.out"
grep -q 'tools/scene_fixture/src/main.rs' "$fixture_root/advisory.out"

if (cd "$fixture_root" && INCLUDE_USAGE_ENFORCE=1 \
  "$repo_root/scripts/ci/check_include_usage.sh") >/dev/null 2>&1; then
  echo "expected enforcing include guard to reject hand-written include!" >&2
  exit 1
fi

# A staged check must inspect the index blob, not a masking worktree rewrite.
cat >"$fixture_root/tools/scene_fixture/src/main.rs" <<'RUST'
include!("staged-manual.rs");
fn main() {}
RUST
git -C "$fixture_root" add tools/scene_fixture/src/main.rs
cat >"$fixture_root/tools/scene_fixture/src/main.rs" <<'RUST'
include!(concat!(env!("OUT_DIR"), "/generated.rs"));
fn main() {}
RUST

if (cd "$fixture_root" && INCLUDE_USAGE_ENFORCE=1 \
  "$repo_root/scripts/ci/check_include_usage.sh" --staged) \
  >"$fixture_root/staged.out" 2>&1; then
  echo "expected staged include guard to reject the violation stored in the index" >&2
  exit 1
fi
grep -q 'include!("staged-manual.rs")' "$fixture_root/staged.out"
grep -q '1 generated include(s) OK' "$fixture_root/staged.out"

echo "include-usage fixture tests passed"
