#!/usr/bin/env bash
#
# Self-test for scripts/ci/check_bt_dram_reservation.sh.
#
# Uses a temporary fake objdump plus dummy ELFs so the real guard runs
# without a firmware build. The default firmware links BLE, so probing a
# release artifact can no longer stand in for either direction.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
guard="$repo_root/scripts/ci/check_bt_dram_reservation.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Dummy ELFs are empty: the guard only stats the path and parses objdump output.
linked_unreserved="$tmp/bt-linked-unreserved.elf"
linked_reserved="$tmp/bt-linked-reserved.elf"
free_unreserved="$tmp/bt-free-unreserved.elf"
free_reserved="$tmp/bt-free-reserved.elf"
stray_origin="$tmp/stray-origin.elf"
touch "$linked_unreserved" "$linked_reserved" \
    "$free_unreserved" "$free_reserved" "$stray_origin"

# Fixture origins are already aligned to the emitted 2**2, so the guard's
# align-up step is the identity and the VMA selects the case exactly.
cat > "$tmp/fake-objdump" <<'FAKE_EOF'
#!/usr/bin/env bash
set -euo pipefail
mode="${1:-}"
elf="${@: -1}"
name="$(basename "$elf")"
case "$name" in
    bt-linked-reserved.elf|bt-free-reserved.elf) vma="3ffbdb5c" ;;
    stray-origin.elf) vma="3ffc0000" ;;
    *) vma="3ffb0000" ;;
esac
if [[ "$mode" == "-h" ]]; then
    cat <<EOF
$name:     file format elf32-xtensa-le

Sections:
Idx Name          Size      VMA       LMA       File off  Algn
  0 .text         00001000  400d0000  400d0000  00001000  2**2
  1 .data         00000010  $vma  $vma  00002000  2**2
  2 .bss          00002000  3ffc0000  3ffc0000  00002010  2**3
EOF
elif [[ "$mode" == "-t" ]]; then
    cat <<EOF
$name:     file format elf32-xtensa-le

SYMBOL TABLE:
400d0000 l    d  .text  00000000 .text
00000000 g     O .data  00000010 some_other_symbol
EOF
    case "$name" in
        bt-linked-*.elf)
            echo "$vma g     O .data  00000004 btdm_controller_init"
            ;;
    esac
else
    echo "fake objdump: unexpected mode: $mode" >&2
    exit 2
fi
FAKE_EOF
chmod +x "$tmp/fake-objdump"
export XTENSA_OBJDUMP="$tmp/fake-objdump"

failures=0
expect_pass() {
    local desc="$1"
    shift
    local out
    if out="$("$guard" "$@" 2>&1)"; then
        printf 'self-test PASS: %s\n%s\n' "$desc" "$out"
    else
        printf 'self-test FAIL (expected pass): %s\n%s\n' "$desc" "$out" >&2
        failures=$((failures + 1))
    fi
}
expect_fail() {
    local desc="$1"
    shift
    local out
    if out="$("$guard" "$@" 2>&1)"; then
        printf 'self-test FAIL (expected failure): %s\n%s\n' "$desc" "$out" >&2
        failures=$((failures + 1))
    else
        printf 'self-test PASS (correctly failed): %s\n%s\n' "$desc" "$out"
    fi
}

# Reserved-without-BT stays a pass: the reservation is per crate build, so a
# non-BLE sibling of a BLE-enabled build legitimately carries it.
expect_fail "linked without reservation is corruption" "$linked_unreserved"
expect_pass "linked with reservation" "$linked_reserved"
expect_pass "unlinked without reservation" "$free_unreserved"
expect_pass "unlinked with reservation (safe sibling)" "$free_reserved"
expect_fail "unexpected .data origin" "$stray_origin"

if ((failures > 0)); then
    printf 'bt dram reservation self-test: %d case(s) failed\n' "$failures" >&2
    exit 1
fi
echo "bt dram reservation self-test: PASS"
