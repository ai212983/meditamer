#!/usr/bin/env bash

set -euo pipefail

if (( $# < 1 )); then
    echo "usage: xtensa_runner.sh <elf-image> [ignored-runner-args...]" >&2
    exit 2
fi

image="$1"
image_base="$(basename "$image")"

# embedded-test integration tests require probe-rs and should not go through
# the default UART flasher runner.
if [[ "$image_base" == embedded_smoke_test-* || "$image_base" == embedded_smoke_test ]]; then
    cat >&2 <<'EOF'
embedded_smoke_test must use the probe-rs runner.
Run with:
  CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUNNER='probe-rs run --chip ESP32 --preverify --always-print-stacktrace --no-location' cargo test --test embedded_smoke_test
EOF
    exit 2
fi

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
