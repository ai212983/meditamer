#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

cycles="${1:-5}"
window_sec="${COLD_BOOT_WINDOW_SEC:-45}"
connect_timeout_sec="${COLD_BOOT_CONNECT_TIMEOUT_SEC:-40}"
baud="${ESPFLASH_BAUD:-115200}"

tty_in="/dev/tty"
tty_out="/dev/tty"

say() {
    stty sane <"$tty_in" >/dev/null 2>&1 || true
    printf '%s\n' "$*" >"$tty_out"
}

port_flag() {
    if stty --help >/dev/null 2>&1; then
        printf -- '-F'
    else
        printf -- '-f'
    fi
}

# stty settings on a *closed* device path do not persist on this platform --
# some USB-serial drivers reset to their own default (often 9600) on next
# open, silently corrupting every capture at the wrong baud. Configuring an
# already-open descriptor (via /dev/fd/N) is what actually sticks, so the
# port is opened once, up front, and held open for the whole run instead of
# being reopened per cycle.
configure_port() {
    local target="$1"
    local stty_flag
    stty_flag="$(port_flag)"
    stty "$stty_flag" "$target" "$baud" cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw >/dev/null 2>&1 || true
}

capture_pid=""
port_fd_open=0

open_port() {
    clear_stale_port_reader
    exec 9<>"$port"
    port_fd_open=1
    configure_port "/dev/fd/9"
}

close_port() {
    if [[ "$port_fd_open" -eq 1 ]]; then
        exec 9<&- 2>/dev/null || true
        exec 9>&- 2>/dev/null || true
        port_fd_open=0
    fi
}

stop_capture() {
    if [[ -n "$capture_pid" ]]; then
        kill "$capture_pid" >/dev/null 2>&1 || true
        wait "$capture_pid" >/dev/null 2>&1 || true
        capture_pid=""
    fi
}

start_capture() {
    local log_file="$1"
    cat <&9 >>"$log_file" 2>/dev/null &
    capture_pid="$!"
}

clear_stale_port_reader() {
    local pid
    local cmd
    for pid in $(lsof -t "$port" 2>/dev/null || true); do
        [[ "$pid" -eq "$$" ]] && continue
        cmd="$(ps -o command= -p "$pid" 2>/dev/null || true)"
        case "$cmd" in
            "cat $port"|"cat <&9")
                kill "$pid" >/dev/null 2>&1 || true
                ;;
        esac
    done
}

has_readable_serial() {
    local log_file="$1"
    [[ -s "$log_file" ]] || return 1
    LC_ALL=C grep -aEq '[A-Za-z]{3,}' "$log_file"
}

# The device isn't power-cycled (see docs/archive/hardware/cold-boot-validation.md), so the port
# keeps streaming pre-existing debug telemetry right up to the moment reset
# is pressed. Gating on "any readable byte" is satisfied by that leftover
# chatter before the reset even happens, which starts the marker-collection
# window early and can starve genuinely slower boot stages. Gate on the
# earliest marker a real boot actually produces instead.
has_boot_started() {
    local log_file="$1"
    [[ -s "$log_file" ]] || return 1
    LC_ALL=C grep -aEq 'rst:0x[0-9A-Fa-f]+ \(|BOOT_RESET reason=' "$log_file"
}

restore_tty() {
    stty sane <"$tty_in" >/dev/null 2>&1 || true
}

cleanup_dir=0
run_dir=""
cleanup() {
    stop_capture
    close_port
    restore_tty

    if [[ "$cleanup_dir" -eq 1 ]] && [[ -n "$run_dir" ]]; then
        rm -rf "$run_dir"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

ensure_espflash_port "cold_boot_matrix.sh" || exit 1
port="${ESPFLASH_PORT}"

if ! [[ "$cycles" =~ ^[0-9]+$ ]] || [[ "$cycles" -lt 1 ]]; then
    echo "cycles must be a positive integer"
    exit 1
fi

if ! [[ "$window_sec" =~ ^[0-9]+$ ]] || [[ "$window_sec" -lt 1 ]]; then
    echo "COLD_BOOT_WINDOW_SEC must be a positive integer"
    exit 1
fi

if ! [[ "$connect_timeout_sec" =~ ^[0-9]+$ ]] || [[ "$connect_timeout_sec" -lt 1 ]]; then
    echo "COLD_BOOT_CONNECT_TIMEOUT_SEC must be a positive integer"
    exit 1
fi

# Verified against this build's actual boot log (logs/source-tree-cleanup/s5-sd-baseline.log);
# the previous set never matched current output at all.
required_patterns=(
    "BOOT_RESET reason="
    "touch: ready phase="
    "LVGL init=ready"
    "RUNTIME_READY app_state=ready display=ready"
)

if [[ "${SOAK_REQUIRE_UPTIME:-0}" == "1" ]]; then
    required_patterns+=("display uptime screen: ok|STATE phase=OPERATING")
fi

run_dir="${COLD_BOOT_LOG_DIR:-}"
if [[ -z "$run_dir" ]]; then
    run_dir="$(mktemp -d -t meditamer_coldboot.XXXXXX)"
    cleanup_dir=1
else
    mkdir -p "$run_dir"
fi

passes=0
fails=0

open_port

for cycle in $(seq 1 "$cycles"); do
    log_file="$run_dir/cycle_${cycle}.log"
    : >"$log_file"

    say ""
    say "reset cycle $cycle/$cycles"
    say "  1) press Enter to arm capture"
    say "  2) press and release the reset button"

    IFS= read -r _ <"$tty_in"

    say ""
    say "Capture armed. Press the reset button now."

    start_capture "$log_file"

    data_deadline=$((SECONDS + connect_timeout_sec))
    next_progress=$((SECONDS + 5))
    while [[ "$SECONDS" -lt "$data_deadline" ]]; do
        if has_boot_started "$log_file"; then
            break
        fi

        if [[ "$SECONDS" -ge "$next_progress" ]]; then
            elapsed=$((connect_timeout_sec - (data_deadline - SECONDS)))
            say "  waiting for the reset to register... (${elapsed}s/${connect_timeout_sec}s)"
            next_progress=$((SECONDS + 5))
        fi

        sleep 0.2
    done

    if ! has_boot_started "$log_file"; then
        stop_capture
        fails=$((fails + 1))
        say "cycle $cycle/$cycles: FAIL"
        if has_readable_serial "$log_file"; then
            say "  serial data arrived but no reset marker was seen within ${connect_timeout_sec}s after arm"
            say "  log: $log_file"
            say "  hint: press the reset button firmly; a partial/bounce press may not reset the chip."
        else
            say "  no serial data captured within ${connect_timeout_sec}s after arm"
            say "  log: $log_file"
        fi
        continue
    fi

    window_deadline=$((SECONDS + window_sec))
    next_window_progress=$((SECONDS + 10))
    while [[ "$SECONDS" -lt "$window_deadline" ]]; do
        all_found=1
        for pattern in "${required_patterns[@]}"; do
            if ! grep -aEq "$pattern" "$log_file"; then
                all_found=0
                break
            fi
        done

        if [[ "$all_found" -eq 1 ]]; then
            break
        fi

        if [[ "$SECONDS" -ge "$next_window_progress" ]]; then
            elapsed=$((window_sec - (window_deadline - SECONDS)))
            say "  capturing boot markers... (${elapsed}s/${window_sec}s)"
            next_window_progress=$((SECONDS + 10))
        fi

        sleep 1
    done

    stop_capture
    restore_tty

    missing_patterns=()
    for pattern in "${required_patterns[@]}"; do
        if ! grep -aEq "$pattern" "$log_file"; then
            missing_patterns+=("$pattern")
        fi
    done

    if [[ "${#missing_patterns[@]}" -eq 0 ]]; then
        passes=$((passes + 1))
        say "cycle $cycle/$cycles: PASS"
    else
        fails=$((fails + 1))
        say "cycle $cycle/$cycles: FAIL"
        say "  log: $log_file"
        for pattern in "${missing_patterns[@]}"; do
            say "  missing: $pattern"
        done
    fi

done

say ""
say "reset-cycle summary: pass=$passes fail=$fails cycles=$cycles"
if [[ "$fails" -gt 0 ]]; then
    if [[ "$cleanup_dir" -eq 1 ]]; then
        say "set COLD_BOOT_LOG_DIR to keep failed logs"
    fi
    exit 2
fi
