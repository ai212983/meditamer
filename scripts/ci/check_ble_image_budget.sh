#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/ble-release/meditamer}"
ceiling="${BLE_IMAGE_CEILING_BYTES:-1900544}"

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
