#!/usr/bin/env bash
#
# `config/linker/esp32/meditamer-memory.x` is a pinned copy of esp-hal's esp32
# `memory.x` with three deliberate changes: `dram2_seg` is extended down over
# the APP CPU ROM stack, `.dram2_uninit` ordering keeps heap storage above
# that reclaimed window, and (esp-hal 1.2.0+) the `RESERVE_DRAM` computation
# is an `INCLUDE` of a file this crate's own build.rs generates with the exact
# pinned controller footprint, replacing esp-hal's rounded inline
# `#IF CARGO_FEATURE(...)` directive -- only esp-hal's own build.rs preprocesses
# that directive, and this file bypasses that pipeline entirely (it replaces,
# not extends, esp-hal's memory.x via `-T`). See
# docs/references/memory/meditamer-inkplate/budget.md and
# `targets/meditamer-inkplate/build.rs`'s `write_memory_extras`.
#
# A pinned copy goes stale silently on an esp-hal upgrade, which would revert
# unrelated upstream fixes without anyone noticing. This fails if upstream
# changed anything outside the deliberate marked divergences.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
pinned="$repo_root/config/linker/esp32/meditamer-memory.x"

# Every intentional edit carries a MEDITAMER marker. Remove those marked lines
# and the marked output-section block, then require equality with upstream after
# removing the original lines/conditional blocks they replace.
marker='MEDITAMER:'
upstream_replaced_line='dram2_seg              : ORIGIN'
# Literal backticks are part of the linker marker.
# shellcheck disable=SC2016
ordering_block_start='/* MEDITAMER: pin the order of `.dram2_uninit` contents.'
# esp-hal 1.2.0's inline RESERVE_DRAM computation, replaced by this file's
# `INCLUDE "memory_extras.x" /* MEDITAMER: ... */` line (stripped as a marker
# line below, same as the dram2_seg line above).
upstream_reserve_dram_block=(
    '/* reserved at the start of DRAM for the BT stack */'
    '#IF CARGO_FEATURE("__bluetooth")'
    'RESERVE_DRAM = 0x10000;'
    '#ELSE'
    'RESERVE_DRAM = 0;'
    '#ENDIF'
)
expected_markers=4

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
    echo "pinned linker scripts: FAIL: expected $expected_markers '$marker' lines in config/linker/esp32/meditamer-memory.x, found $marker_count" >&2
    echo "  Every deliberate divergence from esp-hal must carry that marker." >&2
    exit 1
fi

normalised_raw="$(mktemp -t meditamer-memory-raw.XXXXXX)"
normalised="$(mktemp -t meditamer-memory.XXXXXX)"
trap 'rm -f "$normalised_raw" "$normalised"' EXIT
if ! awk -v marker="$marker" -v block_start="$ordering_block_start" '
    index($0, block_start) {
        in_ordering_block = 1
        block_starts++
        next
    }
    in_ordering_block {
        if ($0 == "}") {
            in_ordering_block = 0
            block_ends++
        }
        next
    }
    index($0, marker) { next }
    { print }
    END {
        if (in_ordering_block || block_starts != 1 || block_ends != 1) {
            exit 42
        }
    }
' "$pinned" >"$normalised_raw"; then
    echo "pinned linker scripts: malformed marked .dram2_uninit ordering block" >&2
    exit 1
fi

# The marked block is appended after upstream's final brace, separated by a
# blank line. Remove only trailing blank lines left by that extraction.
awk '
    NF { last_nonblank = NR }
    { lines[NR] = $0 }
    END {
        for (line = 1; line <= last_nonblank; line++) {
            print lines[line]
        }
    }
' "$normalised_raw" >"$normalised"

upstream_stripped="$(mktemp -t meditamer-memory-upstream.XXXXXX)"
trap 'rm -f "$normalised_raw" "$normalised" "$upstream_stripped"' EXIT
grep_args=(-Fv -e "$upstream_replaced_line")
for line in "${upstream_reserve_dram_block[@]}"; do
    grep_args+=(-e "$line")
done
grep "${grep_args[@]}" "$upstream" >"$upstream_stripped" || true

if ! diff -q "$upstream_stripped" "$normalised" >/dev/null; then
    echo "pinned linker scripts: FAIL: config/linker/esp32/meditamer-memory.x diverges from esp-hal $version beyond its marked lines" >&2
    diff "$upstream_stripped" "$normalised" >&2 || true
    echo "  Re-pin from $upstream, then re-apply the marked changes." >&2
    exit 1
fi

echo "pinned linker scripts: PASS"
echo "  config/linker/esp32/meditamer-memory.x matches esp-hal $version except $marker_count marked divergences"
