#!/usr/bin/env bash
#
# `ld/meditamer-memory.x` is a pinned copy of esp-hal's esp32 `memory.x` with
# exactly one line changed: `dram2_seg` is extended down over the APP CPU ROM
# stack. See docs/development/dram-budget.md.
#
# A pinned copy goes stale silently on an esp-hal upgrade, which would revert
# unrelated upstream fixes without anyone noticing. This fails if upstream
# changed anything other than that one line.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
pinned="$repo_root/ld/meditamer-memory.x"

# Every intentional edit carries a MEDITAMER marker, and the only upstream line
# they replace is the dram2_seg one. Strip both sides and require equality, so
# any other upstream change shows up.
marker='MEDITAMER:'
upstream_replaced_line='dram2_seg              : ORIGIN'
expected_markers=1

if [[ ! -f "$pinned" ]]; then
    echo "pinned linker scripts: missing $pinned" >&2
    exit 2
fi

version="$(awk '
    /^name = "esp-hal"$/ { in_pkg = 1; next }
    in_pkg && /^version = / { gsub(/[",]/, "", $3); print $3; exit }
    /^\[\[package\]\]/ { in_pkg = 0 }
' "$repo_root/Cargo.lock")"

if [[ -z "$version" ]]; then
    echo "pinned linker scripts: could not read esp-hal version from Cargo.lock" >&2
    exit 2
fi

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
upstream=""
for candidate in "$cargo_home"/registry/src/*/"esp-hal-$version"/ld/esp32/memory.x; do
    if [[ -f "$candidate" ]]; then
        upstream="$candidate"
        break
    fi
done

if [[ -z "$upstream" ]]; then
    echo "pinned linker scripts: esp-hal $version sources not vendored; run a build first" >&2
    exit 2
fi

marker_count="$(grep -Fc "$marker" "$pinned" || true)"
if [[ "$marker_count" != "$expected_markers" ]]; then
    echo "pinned linker scripts: FAIL: expected $expected_markers '$marker' lines in ld/meditamer-memory.x, found $marker_count" >&2
    echo "  Every deliberate divergence from esp-hal must carry that marker." >&2
    exit 1
fi

normalised="$(mktemp -t meditamer-memory.XXXXXX)"
trap 'rm -f "$normalised"' EXIT
grep -Fv "$marker" "$pinned" >"$normalised"

if ! diff -q <(grep -Fv "$upstream_replaced_line" "$upstream") "$normalised" >/dev/null; then
    echo "pinned linker scripts: FAIL: ld/meditamer-memory.x diverges from esp-hal $version beyond its marked lines" >&2
    diff <(grep -Fv "$upstream_replaced_line" "$upstream") "$normalised" >&2 || true
    echo "  Re-pin from $upstream, then re-apply the marked changes." >&2
    exit 1
fi

echo "pinned linker scripts: PASS"
echo "  ld/meditamer-memory.x matches esp-hal $version except $marker_count marked lines"
