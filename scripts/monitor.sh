#!/usr/bin/env bash

set -euo pipefail

baud="${ESPFLASH_BAUD:-115200}"
before="${ESPFLASH_MONITOR_BEFORE:-default-reset}"
after="${ESPFLASH_MONITOR_AFTER:-hard-reset}"
mode="${ESPFLASH_MONITOR_MODE:-espflash}"
persist_raw="${ESPFLASH_MONITOR_PERSIST_RAW:-1}"
raw_backend="${ESPFLASH_MONITOR_RAW_BACKEND:-auto}"
raw_output_mode="${ESPFLASH_MONITOR_OUTPUT_MODE:-normal}"
raw_tio_mute="${ESPFLASH_MONITOR_RAW_TIO_MUTE:-1}"
output_file="${ESPFLASH_MONITOR_OUTPUT_FILE:-}"
child_pid=""
child_pgid=""
child_has_own_pgid=0

# Prefer espflash non-interactive mode when supported.
# Interactive input reader fails in some terminals; wrapper trap still handles Ctrl+C.
monitor_noninteractive_arg=""
if command -v espflash >/dev/null 2>&1 && espflash monitor --help 2>&1 | grep -q -- '--non-interactive'; then
    monitor_noninteractive_arg="--non-interactive"
fi

if [[ -n "$output_file" ]]; then
    mkdir -p "$(dirname "$output_file")"
    : >"$output_file"
    echo "monitor output -> $output_file" >&2
fi

cleanup_child() {
    if [[ -n "${child_pid:-}" ]]; then
        if [[ "$child_has_own_pgid" -eq 1 ]] && [[ -n "${child_pgid:-}" ]]; then
            kill -TERM -- "-$child_pgid" >/dev/null 2>&1 || true
            sleep 0.1
            kill -KILL -- "-$child_pgid" >/dev/null 2>&1 || true
        elif kill -0 "$child_pid" >/dev/null 2>&1; then
            kill -TERM "$child_pid" >/dev/null 2>&1 || true
            sleep 0.1
            kill -KILL "$child_pid" >/dev/null 2>&1 || true
        fi
        wait "$child_pid" >/dev/null 2>&1 || true
    fi
    child_pid=""
    child_pgid=""
    child_has_own_pgid=0
}

on_signal() {
    cleanup_child
    exit 130
}

run_interruptible() {
    child_has_own_pgid=0
    if command -v setsid >/dev/null 2>&1; then
        if [[ -n "$output_file" ]]; then
            setsid "$@" >>"$output_file" 2>&1 &
        else
            setsid "$@" &
        fi
        child_has_own_pgid=1
    else
        if [[ -n "$output_file" ]]; then
            "$@" >>"$output_file" 2>&1 &
        else
            "$@" &
        fi
    fi
    child_pid=$!
    child_pgid="$(ps -o pgid= -p "$child_pid" 2>/dev/null | tr -d '[:space:]')"
    local rc=0
    while kill -0 "$child_pid" >/dev/null 2>&1; do
        sleep 0.1
    done
    wait "$child_pid" || rc=$?
    child_pid=""
    child_pgid=""
    child_has_own_pgid=0
    return "$rc"
}

trap on_signal INT TERM

port_flag() {
    if stty --help >/dev/null 2>&1; then
        printf -- '-F'
    else
        printf -- '-f'
    fi
}

if [[ "$mode" == "raw" ]]; then
    if [[ -z "${ESPFLASH_PORT:-}" ]]; then
        echo "ESPFLASH_PORT must be set when ESPFLASH_MONITOR_MODE=raw"
        exit 1
    fi

    backend="$raw_backend"
    if [[ "$backend" == "auto" ]]; then
        if command -v tio >/dev/null 2>&1; then
            backend="tio"
        else
            backend="cat"
        fi
    fi

    if [[ "$backend" == "tio" ]]; then
        announced_wait=0
        showed_tio_quit_hint=0
        while true; do
            if [[ -e "$ESPFLASH_PORT" ]]; then
                announced_wait=0
                if [[ "$showed_tio_quit_hint" -eq 0 ]]; then
                    echo "raw monitor (tio): press Ctrl+T then q to quit." >&2
                    showed_tio_quit_hint=1
                fi
                tio_args=(
                    "$ESPFLASH_PORT"
                    -b "$baud"
                    -d 8
                    -f none
                    -s 1
                    -p none
                    --output-mode "$raw_output_mode"
                )
                if [[ "$raw_tio_mute" == "1" ]]; then
                    tio_args+=(--mute)
                fi
                if [[ "$persist_raw" != "1" ]]; then
                    tio_args+=(--no-reconnect)
                fi
                set +e
                run_interruptible tio "${tio_args[@]}"
                rc=$?
                set -e
                if [[ "$rc" -eq 130 ]]; then
                    exit 130
                fi

                if [[ "$persist_raw" != "1" ]]; then
                    exit 0
                fi
                echo "raw monitor: serial disconnected, waiting for reconnect..." >&2
                sleep 0.2
            else
                if [[ "$announced_wait" -eq 0 ]]; then
                    echo "raw monitor: waiting for serial port $ESPFLASH_PORT..." >&2
                    announced_wait=1
                fi
                sleep 0.2
            fi
        done
    fi

    if [[ "$backend" == "cat" ]]; then
        stty_flag="$(port_flag)"
        announced_wait=0
        while true; do
            if [[ -e "$ESPFLASH_PORT" ]]; then
                announced_wait=0
                stty "$stty_flag" "$ESPFLASH_PORT" "$baud" cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw clocal || true
                set +e
                run_interruptible cat "$ESPFLASH_PORT"
                rc=$?
                set -e
                if [[ "$rc" -eq 130 ]]; then
                    exit 130
                fi

                if [[ "$persist_raw" != "1" ]]; then
                    exit 0
                fi
                echo "raw monitor: serial disconnected, waiting for reconnect..." >&2
                sleep 0.2
            else
                if [[ "$announced_wait" -eq 0 ]]; then
                    echo "raw monitor: waiting for serial port $ESPFLASH_PORT..." >&2
                    announced_wait=1
                fi
                sleep 0.2
            fi
        done
    fi

    echo "unsupported ESPFLASH_MONITOR_RAW_BACKEND=$backend (use auto|tio|cat)" >&2
    exit 1
fi

if [[ -n "${ESPFLASH_PORT:-}" ]]; then
    set +e
    if [[ -n "$monitor_noninteractive_arg" ]]; then
        run_interruptible \
            espflash monitor \
            -p "$ESPFLASH_PORT" \
            -c esp32 \
            -B "$baud" \
            --before "$before" \
            --after "$after" \
            "$monitor_noninteractive_arg"
    else
        run_interruptible \
            espflash monitor \
            -p "$ESPFLASH_PORT" \
            -c esp32 \
            -B "$baud" \
            --before "$before" \
            --after "$after"
    fi
    rc=$?
    set -e
    exit "$rc"
else
    set +e
    if [[ -n "$monitor_noninteractive_arg" ]]; then
        run_interruptible \
            espflash monitor \
            -c esp32 \
            -B "$baud" \
            --before "$before" \
            --after "$after" \
            "$monitor_noninteractive_arg"
    else
        run_interruptible \
            espflash monitor \
            -c esp32 \
            -B "$baud" \
            --before "$before" \
            --after "$after"
    fi
    rc=$?
    set -e
    exit "$rc"
fi
