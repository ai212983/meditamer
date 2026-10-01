#!/usr/bin/env bash
# Manual production scan-shape qualification (not CI).
#
# Run after changing the panel waveform source, then record the result with
# the change. CI owns only placement in scripts/ci/check_panel_waveform_placement.sh.
# This script checks compiled assembly shape; it makes no signal or image claim.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"

if [[ "$#" -gt 1 ]]; then
    echo "usage: $0 [elf]" >&2
    exit 2
fi
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/release/meditamer}"
if [[ ! -f "$elf" ]]; then
    echo "scan contract: missing release ELF: $elf" >&2
    exit 2
fi

objdump="${XTENSA_OBJDUMP:-}"
if [[ -z "$objdump" ]]; then
    objdump="$(command -v xtensa-esp32-elf-objdump || true)"
fi
if [[ -z "$objdump" ]]; then
    rust_sysroot="$(rustup run esp rustc --print sysroot)"
    objdump="$(find "$rust_sysroot/xtensa-esp-elf" -type f -name xtensa-esp32-elf-objdump -print -quit 2>/dev/null || true)"
fi
if [[ -z "$objdump" || ! -x "$objdump" ]]; then
    echo "scan contract: xtensa-esp32-elf-objdump not found" >&2
    exit 2
fi

symbols="$(mktemp -t meditamer-scan-contract.XXXXXX)"
trap 'rm -f "$symbols"' EXIT
"$objdump" -t "$elf" >"$symbols"

fail() {
    echo "scan contract: FAIL: $1" >&2
    exit 1
}

# Disassemble one .rwtext symbol; fail closed when absent.
scan_asm() {
    local name="$1" mangled
    mangled="$(awk -v name="$name" '$0 ~ /[.]rwtext/ && index($NF, name) { print $NF; exit }' "$symbols")"
    [[ -n "$mangled" ]] || fail "$name is not in .rwtext"
    "$objdump" --disassemble="$mangled" "$elf"
}

for symbol in scan_full_refresh_clean_pass scan_full_binary_pass scan_partial_framebuffer_rows_with_neutral_drain; do
    grep -Eq "[.]rwtext[[:space:]].*${symbol}" "$symbols" || fail "$symbol is not in .rwtext"
done
for symbol in LUT2 LUTB LUTW; do
    grep -Eq "[.]data[[:space:]].*${symbol}$" "$symbols" || fail "$symbol is not in .data"
done

if grep -Eq '[.]rwtext[[:space:]].*scan_full_(framebuffer|settle)_pass' "$symbols"; then
    fail "retired duplicate full-refresh scanner is still linked"
fi

# Immediate ordered GPIO writes for all production scans.
for symbol in scan_full_refresh_clean_pass scan_full_binary_pass scan_partial_framebuffer_rows_with_neutral_drain; do
    assembly="$(scan_asm "$symbol")"
    grep -q 'rsr[.]ccount' <<<"$assembly" && fail "$symbol retains a CCOUNT hold"
    grep -q '3ff44008' <<<"$assembly" || fail "$symbol lacks GPIO W1TS register"
    grep -q '3ff4400c' <<<"$assembly" || fail "$symbol lacks GPIO W1TC register"
    set_line="$(grep -n -m1 '3ff44008' <<<"$assembly" | cut -d: -f1)"
    clear_line="$(grep -n -m1 '3ff4400c' <<<"$assembly" | cut -d: -f1)"
    ((set_line < clear_line)) || fail "$symbol does not order W1TS before W1TC"
    grep -q 'memw' <<<"$assembly" || fail "$symbol lacks ordered memory barriers"
done

# Full rows use ordered row writes without a numeric delay call.
for symbol in scan_full_refresh_clean_pass scan_full_binary_pass; do
    assembly="$(scan_asm "$symbol")"
    grep -q 'ets_delay_us' <<<"$assembly" && fail "$symbol retains a row-boundary delay"
    awk 'index($0, "3ff44018") { a=1; next } a && index($0, "3ff44008") { b=1; next } b && index($0, "3ff4400c") { ok=1 } END { exit ok ? 0 : 1 }' <<<"$assembly" \
        || fail "$symbol lacks ordered CKV-clear/LE-set/LE-clear row writes"
    grep -q 'hold_full_refresh_panel_clock_high' <<<"$assembly" && fail "$symbol retains a row-start helper call"
    grep -q 'panic_bounds_check' <<<"$assembly" && fail "$symbol retains a bounds-check path"
    [[ "$(grep -Ec '[[:space:]]nop([.]n)?[[:space:]]*$' <<<"$assembly" || true)" == 0 ]] || fail "$symbol retains NOP padding"
done

# Production source and clean loops: one 74-iteration hardware loop each.
for symbol in scan_full_binary_pass scan_full_refresh_clean_pass; do
    assembly="$(scan_asm "$symbol")"
    grep -Eq 'movi([.]n)?[[:space:]]+a[0-9]+,[[:space:]]*74([[:space:]]|$)' <<<"$assembly" \
        || fail "$symbol lacks the 74-iteration loop count"
    [[ "$(grep -Ec '[[:space:]]loop[[:space:]]' <<<"$assembly" || true)" == 1 ]] \
        || fail "$symbol does not have exactly one hardware loop"
done
clean_asm="$(scan_asm scan_full_refresh_clean_pass)"
grep -q 'hold_full_refresh_panel_clock_high' <<<"$clean_asm" && fail "clean pass retains edge helper calls"

# Partial production shape: one hardware loop, one neutral-drain delay call.
partial_asm="$(scan_asm scan_partial_framebuffer_rows_with_neutral_drain)"
[[ "$(grep -c '[[:space:]]callx8[[:space:]]' <<<"$partial_asm" || true)" == 1 ]] \
    || fail "partial scan does not have exactly one indirect delay call"
[[ "$(grep -c '<ets_delay_us>' <<<"$partial_asm" || true)" == 1 ]] \
    || fail "partial scan does not have exactly one ets_delay_us reference"
[[ "$(grep -Ec '[[:space:]]loop[[:space:]]' <<<"$partial_asm" || true)" == 1 ]] \
    || fail "partial scan does not have exactly one hardware loop"

# Bounded transition preparation used by the partial reverse scan.
prep_mangled="$(awk 'index($NF, "prepare_panel_partial_transition_for_span") { print $NF; exit }' "$symbols")"
[[ -n "$prep_mangled" ]] || fail "prepare_panel_partial_transition_for_span is not linked"
prep_asm="$("$objdump" --disassemble="$prep_mangled" "$elf")"
grep -Eq 'movi([.]n)?[[:space:]]+a[0-9]+,[[:space:]]*75([[:space:]]|$)' <<<"$prep_asm" \
    || fail "transition preparation lacks the bounded-row dispatch"
grep -Eq '[[:space:]]mull[[:space:]]' <<<"$prep_asm" \
    || fail "transition preparation lacks the bounded-row multiply"
grep -Eq 'movi([.]n)?[[:space:]]+a[0-9]+,[[:space:]]*0x258([[:space:]]|$)' <<<"$prep_asm" \
    || fail "transition preparation lacks the bounded-row size"

# No retained inter-pass spacing in the full orchestration.
found_waveform=false
for caller in display_bw_waveform_async clean_full_refresh_async; do
    while IFS= read -r mangled; do
        [[ -n "$mangled" ]] || continue
        if [[ "$caller" == display_bw_waveform_async ]]; then
            found_waveform=true
        fi
        assembly="$("$objdump" --disassemble="$mangled" "$elf")"
        grep -Eq 'movi([.]n)?[[:space:]]+a[0-9]+,[[:space:]]*(230|0xe6)([[:space:]]|$)' <<<"$assembly" \
            && fail "$caller retains an inter-pass delay value"
        grep -q 'DelayOps8delay_us' <<<"$assembly" && fail "$caller retains an inter-pass delay call"
    done < <(awk -v name="$caller" '$0 ~ /[.]text/ && index($NF, name) && !index($NF, "drop_glue") { print $NF }' "$symbols")
done
[[ "$found_waveform" == true ]] || fail "display_bw_waveform_async is not linked"

echo "scan contract: PASS"
