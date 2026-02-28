#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/serial_port.sh
source "$script_dir/lib/serial_port.sh"

cycles="${1:-5}"
window_sec="${COLD_BOOT_WINDOW_SEC:-45}"
connect_timeout_sec="${COLD_BOOT_CONNECT_TIMEOUT_SEC:-40}"
arm_timeout_sec="${COLD_BOOT_ARM_TIMEOUT_SEC:-20}"
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

configure_port() {
    local stty_flag
    stty_flag="$(port_flag)"
    stty "$stty_flag" "$port" "$baud" cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw >/dev/null 2>&1 || true
}

capture_pid=""

stop_capture() {
    if [[ -n "$capture_pid" ]]; then
        kill "$capture_pid" >/dev/null 2>&1 || true
        wait "$capture_pid" >/dev/null 2>&1 || true
        capture_pid=""
    fi
}

start_capture() {
    local log_file="$1"
    configure_port
    cat "$port" >>"$log_file" 2>/dev/null &
    capture_pid="$!"
}

clear_stale_port_reader() {
    local pid
    local cmd
    for pid in $(lsof -t "$port" 2>/dev/null || true); do
        [[ "$pid" -eq "$$" ]] && continue
        cmd="$(ps -o command= -p "$pid" 2>/dev/null || true)"
        case "$cmd" in
            "cat $port")
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

restore_tty() {
    stty sane <"$tty_in" >/dev/null 2>&1 || true
}

cleanup_dir=0
run_dir=""
cleanup() {
    stop_capture
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

if ! [[ "$arm_timeout_sec" =~ ^[0-9]+$ ]] || [[ "$arm_timeout_sec" -lt 1 ]]; then
    echo "COLD_BOOT_ARM_TIMEOUT_SEC must be a positive integer"
    exit 1
fi

# Accept either legacy startup markers or the current runtime signature.
required_patterns=(
    "core init complete|BOOT_RESET reason="
    "frontlight brightness write: ok|touch: ready phase=boot"
    "display test pattern: ok|sdprobe\\[request\\]: card_detected"
    "render loop: uptime clock|SDDONE id=0 op=probe status=ok code=ok"
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

for cycle in $(seq 1 "$cycles"); do
    log_file="$run_dir/cycle_${cycle}.log"
    : >"$log_file"

    say ""
    say "cold boot cycle $cycle/$cycles"
    say "  1) disconnect power"
    say "  2) wait ~5 seconds"
    say "  3) press Enter to arm capture"
    say "  4) reconnect power immediately"

    IFS= read -r _ <"$tty_in"

    say ""
    say "Capture armed. Reconnect power now."

    arm_deadline=$((SECONDS + arm_timeout_sec))
    while [[ ! -e "$port" && "$SECONDS" -lt "$arm_deadline" ]]; do
        sleep 0.2
    done

    if [[ ! -e "$port" ]]; then
        stop_capture
        fails=$((fails + 1))
        say "cycle $cycle/$cycles: FAIL"
        say "  serial port did not reappear within ${arm_timeout_sec}s"
        say "  log: $log_file"
        continue
    fi

    clear_stale_port_reader
    start_capture "$log_file"

    data_deadline=$((SECONDS + connect_timeout_sec))
    next_progress=$((SECONDS + 5))
    while [[ "$SECONDS" -lt "$data_deadline" ]]; do
        if has_readable_serial "$log_file"; then
            break
        fi

        if [[ "$SECONDS" -ge "$next_progress" ]]; then
            elapsed=$((connect_timeout_sec - (data_deadline - SECONDS)))
            say "  waiting for readable serial bytes... (${elapsed}s/${connect_timeout_sec}s)"
            next_progress=$((SECONDS + 5))
        fi

        sleep 0.2
    done

    if ! has_readable_serial "$log_file"; then
        stop_capture
        fails=$((fails + 1))
        say "cycle $cycle/$cycles: FAIL"
        if [[ -s "$log_file" ]]; then
            say "  only non-text bytes captured within ${connect_timeout_sec}s after arm"
            say "  log: $log_file"
            say "  hint: this often happens if USB was unplugged but board stayed on battery."
            say "  hint: perform a true power-off (board OFF, LED off), then power on."
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
say "cold-boot summary: pass=$passes fail=$fails cycles=$cycles"
if [[ "$fails" -gt 0 ]]; then
    if [[ "$cleanup_dir" -eq 1 ]]; then
        say "set COLD_BOOT_LOG_DIR to keep failed logs"
    fi
    exit 2
fi
