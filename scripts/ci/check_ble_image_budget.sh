#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/ble-release/meditamer}"
# What the device actually accepts: ADR-0014 replaced ADR-0009's two 0x1f0000
# A/B slots with one 0x380000 `ota_0` production partition written from an SD
# bundle, so the binding limit is the SD updater's payload ceiling --
# `MAX_FIRMWARE_LEN` in src/updater/mod.rs (0x380000 - 0x2000). Keep this in
# step with that constant; an image the updater would reject must fail here.
ceiling="${BLE_IMAGE_CEILING_BYTES:-3661824}"
board_pool_ceiling="${BLE_BOARD_RUNTIME_POOL_CEILING_BYTES:-72}"
linked_stack_floor="${BLE_LINKED_STACK_FLOOR_BYTES:-33900}"

if [[ ! -f "$elf" ]]; then
  echo "BLE image budget check failed: missing ELF $elf" >&2
  exit 1
fi
if ! command -v espflash >/dev/null 2>&1; then
  echo "BLE image budget check failed: espflash is not on PATH" >&2
  exit 1
fi

image_dir="$(mktemp -d)"
trap 'rm -r "$image_dir"' EXIT
espflash save-image --skip-update-check --chip esp32 "$elf" "$image_dir/meditamer.bin" >/dev/null
image_bytes="$(wc -c <"$image_dir/meditamer.bin" | tr -d '[:space:]')"

if ((image_bytes > ceiling)); then
  echo "BLE image budget check failed: $image_bytes bytes exceeds $ceiling" >&2
  exit 1
fi

echo "BLE image budget check passed: $image_bytes/$ceiling bytes ($((ceiling - image_bytes)) headroom)"

if ! command -v nm >/dev/null 2>&1 || ! command -v objdump >/dev/null 2>&1; then
  echo "BLE image budget check failed: nm and objdump are required for memory ratchets" >&2
  exit 1
fi

board_pool_hex="$(nm -S -C "$elf" | awk '/board_runtime_task::POOL$/ { print $2 }')"
if [[ -z "$board_pool_hex" ]]; then
  echo "BLE image budget check failed: board runtime task pool symbol is missing" >&2
  exit 1
fi
board_pool_bytes="$((16#$board_pool_hex))"
if ((board_pool_bytes > board_pool_ceiling)); then
  echo "BLE image budget check failed: board runtime pool $board_pool_bytes exceeds $board_pool_ceiling" >&2
  exit 1
fi

linked_stack_hex="$(objdump -h "$elf" | awk '$2 == ".stack" { print $3 }')"
if [[ -z "$linked_stack_hex" ]]; then
  echo "BLE image budget check failed: linked .stack section is missing" >&2
  exit 1
fi
linked_stack_bytes="$((16#$linked_stack_hex))"
if ((linked_stack_bytes < linked_stack_floor)); then
  echo "BLE image budget check failed: linked .stack $linked_stack_bytes is below $linked_stack_floor" >&2
  exit 1
fi

echo "BLE memory ratchets passed: board_runtime_pool=$board_pool_bytes/$board_pool_ceiling linked_stack=$linked_stack_bytes floor=$linked_stack_floor"
