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
