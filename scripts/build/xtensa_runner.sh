#!/usr/bin/env bash

set -euo pipefail

if (( $# < 1 )); then
    echo "usage: xtensa_runner.sh <elf-image> [ignored-runner-args...]" >&2
    exit 2
fi

image="$1"

chip="${ESPFLASH_CHIP:-esp32}"
port="${ESPFLASH_PORT:-}"
baud="${ESPFLASH_BAUD:-}"
before="${ESPFLASH_BEFORE:-default-reset}"
after="${ESPFLASH_AFTER:-hard-reset}"
run_monitor="${ESPFLASH_RUN_MONITOR:-0}"

cmd=(espflash flash -c "$chip")

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bootloader="$repo_root/target/single-production-bootloader/bootloader/bootloader.bin"
partition_table="$repo_root/config/partitions-single-production.csv"
if [[ ! -f "$bootloader" ]]; then
    "$repo_root/scripts/build/single_production_bootloader.sh"
fi
cmd+=(--bootloader "$bootloader" --partition-table "$partition_table" --target-app-partition ota_0)

if [[ -n "$port" ]]; then
    cmd+=(-p "$port")
fi
if [[ -n "$baud" ]]; then
    cmd+=(-B "$baud")
fi
if [[ -n "$before" ]]; then
    cmd+=(--before "$before")
fi
if [[ -n "$after" ]]; then
    cmd+=(--after "$after")
fi
if [[ "$run_monitor" == "1" ]]; then
    cmd+=(--monitor)
fi

cmd+=("$image")

exec "${cmd[@]}"
