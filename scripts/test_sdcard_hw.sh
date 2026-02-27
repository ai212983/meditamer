#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

if [[ -z "${ESPFLASH_PORT:-}" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

flash_first="${SDCARD_TEST_FLASH_FIRST:-0}"
build_mode="${1:-debug}"
verify_lba="${SDCARD_TEST_VERIFY_LBA:-2048}"
run_tag="$(date +%H%M%S)"
base_path="${SDCARD_TEST_BASE_PATH:-/sd${run_tag}}"
output_path="${2:-$repo_root/logs/sdcard_hw_test_$(date +%Y%m%d_%H%M%S).log}"
suite="${SDCARD_TEST_SUITE:-all}"
sdwait_timeout_ms="${SDCARD_TEST_SDWAIT_TIMEOUT_MS:-300000}"
monitor_mode="${SDCARD_TEST_MONITOR_MODE:-raw}"
monitor_raw_backend="${SDCARD_TEST_MONITOR_RAW_BACKEND:-cat}"
monitor_persist_raw="${SDCARD_TEST_MONITOR_PERSIST_RAW:-1}"
monitor_raw_tio_mute="${SDCARD_TEST_MONITOR_RAW_TIO_MUTE:-0}"
monitor_port="${SDCARD_TEST_MONITOR_PORT:-}"

case "$suite" in
all | baseline | burst | failures) ;;
*)
    echo "Invalid SDCARD_TEST_SUITE=$suite (use all|baseline|burst|failures)" >&2
    exit 1
    ;;
esac

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

if [[ -z "$monitor_port" ]]; then
    monitor_port="$ESPFLASH_PORT"
    if [[ "$monitor_raw_backend" != "cat" && "$ESPFLASH_PORT" == /dev/cu.* ]]; then
        candidate_monitor_port="/dev/tty.${ESPFLASH_PORT#/dev/cu.}"
        if [[ -e "$candidate_monitor_port" ]]; then
            monitor_port="$candidate_monitor_port"
        fi
    fi
fi

port_flag() {
    if stty --help >/dev/null 2>&1; then
        printf -- '-F'
    else
        printf -- '-f'
    fi
}

cleanup() {
    if [[ "${serial_fd_open:-0}" == "1" ]]; then
        exec 3>&-
        serial_fd_open=0
    fi
    if [[ -n "${monitor_pid:-}" ]]; then
        kill "$monitor_pid" >/dev/null 2>&1 || true
        wait "$monitor_pid" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT INT TERM

if [[ "$flash_first" == "1" ]]; then
    echo "Flashing firmware ($build_mode) before SD-card hardware test..."
    ESPFLASH_PORT="$ESPFLASH_PORT" FLASH_SET_TIME_AFTER_FLASH=0 "$script_dir/flash.sh" "$build_mode"
fi

echo "Starting monitor capture: $output_path"
echo "Monitor port: $monitor_port (command port: $ESPFLASH_PORT)"
ESPFLASH_PORT="$monitor_port" \
ESPFLASH_MONITOR_MODE="$monitor_mode" \
ESPFLASH_MONITOR_RAW_BACKEND="$monitor_raw_backend" \
ESPFLASH_MONITOR_PERSIST_RAW="$monitor_persist_raw" \
ESPFLASH_MONITOR_RAW_TIO_MUTE="$monitor_raw_tio_mute" \
ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" \
"$script_dir/monitor.sh" >/dev/null 2>&1 &
monitor_pid=$!
sleep 1

stty "$(port_flag)" "$ESPFLASH_PORT" 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw clocal
exec 3>"$ESPFLASH_PORT"
serial_fd_open=1

send_line() {
    printf '%s\r\n' "$1" >&3
}

wait_for_pattern_from_line() {
    local start_line="$1"
    local pattern="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        if tail -n +$((start_line + 1)) "$output_path" | rg -q "$pattern"; then
            return 0
        fi
        sleep 1
    done
    return 1
}

ack_status_from_line() {
    local start_line="$1"
    local ack_tag="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local match_line
        match_line="$(tail -n +$((start_line + 1)) "$output_path" | rg -m1 "${ack_tag} (OK|BUSY)" || true)"
        if [[ "$match_line" == *" OK"* ]]; then
            echo "OK"
            return 0
        fi
        if [[ "$match_line" == *" BUSY"* ]]; then
            echo "BUSY"
            return 0
        fi
        sleep 1
    done
    echo "NONE"
    return 0
}

wait_for_sdreq_id_from_line() {
    local start_line="$1"
    local timeout_s="$2"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local match_line
        match_line="$(tail -n +$((start_line + 1)) "$output_path" | rg -m1 "SDREQ id=[0-9]+ op=" || true)"
        if [[ -n "$match_line" ]]; then
            sed -E 's/.*SDREQ id=([0-9]+) .*/\1/' <<<"$match_line"
            return 0
        fi
        sleep 1
    done
    return 1
}

wait_for_sdreq_count_from_line() {
    local start_line="$1"
    local expected_count="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local count
        count="$(tail -n +$((start_line + 1)) "$output_path" | rg -c "SDREQ id=[0-9]+ op=" || true)"
        if [[ "${count:-0}" -ge "$expected_count" ]]; then
            return 0
        fi
        sleep 1
    done
    return 1
}

last_sdreq_id_from_line() {
    local start_line="$1"
    local match_line
    match_line="$(
        tail -n +$((start_line + 1)) "$output_path" \
            | rg "SDREQ id=[0-9]+ op=" \
            | tail -n1 || true
    )"
    if [[ -n "$match_line" ]]; then
        sed -E 's/.*SDREQ id=([0-9]+) .*/\1/' <<<"$match_line"
    fi
}

sdwait_result_from_line() {
    local start_line="$1"
    local timeout_s="$2"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local match_line
        match_line="$(tail -n +$((start_line + 1)) "$output_path" | rg -m1 "SDWAIT (DONE|TIMEOUT|ERR)" || true)"
        if [[ "$match_line" == *"SDWAIT DONE"* ]]; then
            local status
            local code
            status="$(sed -nE 's/.* status=([a-z]+) .*/\1/p' <<<"$match_line")"
            code="$(sed -nE 's/.* code=([a-z0-9_]+) .*/\1/p' <<<"$match_line")"
            if [[ -n "$status" && -n "$code" ]]; then
                echo "$status $code"
                return 0
            fi
            echo "err -"
            return 0
        fi
        if [[ "$match_line" == *"SDWAIT TIMEOUT"* ]]; then
            echo "timeout -"
            return 0
        fi
        if [[ "$match_line" == *"SDWAIT ERR"* ]]; then
            echo "err -"
            return 0
        fi
        sleep 1
    done
    echo "none -"
    return 0
}

wait_for_sd_result() {
    local request_id="$1"
    local expected_status="$2"
    local expected_code="${3:-}"
    local sdwait_start_line
    local sdwait_wait_timeout_s
    sdwait_start_line="$(wc -l <"$output_path")"
    sdwait_wait_timeout_s=$((((sdwait_timeout_ms + 999) / 1000) + 15))
    send_line "SDWAIT $request_id $sdwait_timeout_ms"
    local result status code
    result="$(sdwait_result_from_line "$sdwait_start_line" "$sdwait_wait_timeout_s")"
    read -r status code <<<"$result"
    if [[ "$status" != "$expected_status" ]]; then
        return 1
    fi
    if [[ -n "$expected_code" && "$code" != "$expected_code" ]]; then
        return 1
    fi
    return 0
}

run_step() {
    local name="$1"
    local command="$2"
    local ack_tag="$3"
    local completion_pattern="$4"
    local max_attempts="${5:-12}"

    local attempt=1
    while ((attempt <= max_attempts)); do
        local start_line
        start_line="$(wc -l <"$output_path")"
        send_line "$command"
        local status
        status="$(ack_status_from_line "$start_line" "$ack_tag" 8)"

        if [[ "$status" == "OK" ]]; then
            local request_id
            request_id="$(wait_for_sdreq_id_from_line "$start_line" 8 || true)"
            if [[ -n "$request_id" ]] && wait_for_sd_result "$request_id" "ok"; then
                if wait_for_pattern_from_line "$start_line" "$completion_pattern" 90; then
                    echo "[PASS] $name"
                    return 0
                fi
            fi
        fi

        if [[ "$status" == "BUSY" || "$status" == "NONE" ]]; then
            sleep 2
            attempt=$((attempt + 1))
            continue
        fi

        attempt=$((attempt + 1))
    done

    echo "[FAIL] $name"
    tail -n 160 "$output_path" >&2
    return 1
}

run_step_expect_error() {
    local name="$1"
    local command="$2"
    local ack_tag="$3"
    local error_pattern="$4"
    local expected_code="${5:-}"
    local max_attempts="${6:-12}"

    local attempt=1
    while ((attempt <= max_attempts)); do
        local start_line
        start_line="$(wc -l <"$output_path")"
        send_line "$command"
        local status
        status="$(ack_status_from_line "$start_line" "$ack_tag" 8)"

        if [[ "$status" == "OK" ]]; then
            local request_id
            request_id="$(wait_for_sdreq_id_from_line "$start_line" 8 || true)"
            if [[ -n "$request_id" ]] && wait_for_sd_result "$request_id" "error" "$expected_code"; then
                if wait_for_pattern_from_line "$start_line" "$error_pattern" 90; then
                    echo "[PASS] $name"
                    return 0
                fi
            fi
        fi

        if [[ "$status" == "BUSY" || "$status" == "NONE" ]]; then
            sleep 2
            attempt=$((attempt + 1))
            continue
        fi

        attempt=$((attempt + 1))
    done

    echo "[FAIL] $name"
    tail -n 160 "$output_path" >&2
    return 1
}

run_raw_command_expect_pattern() {
    local name="$1"
    local command="$2"
    local pattern="$3"
    local timeout_s="${4:-20}"
    local start_line
    start_line="$(wc -l <"$output_path")"
    send_line "$command"
    if wait_for_pattern_from_line "$start_line" "$pattern" "$timeout_s"; then
        echo "[PASS] $name"
        return 0
    fi
    echo "[FAIL] $name"
    tail -n 120 "$output_path" >&2
    return 1
}

run_burst_sequence() {
    local burst_root="/b${run_tag}"
    local burst_file="$burst_root/io.txt"
    local start_line

    start_line="$(wc -l <"$output_path")"
    send_line "SDFATMKDIR $burst_root"
    send_line "SDFATWRITE $burst_file A"
    send_line "SDFATAPPEND $burst_file BC"
    send_line "SDFATSTAT $burst_file"
    send_line "SDFATREAD $burst_file"

    if ! wait_for_sdreq_count_from_line "$start_line" 5 30; then
        echo "[FAIL] burst_sdreq_count"
        tail -n 160 "$output_path" >&2
        return 1
    fi
    local burst_last_id
    burst_last_id="$(last_sdreq_id_from_line "$start_line")"
    if [[ -z "$burst_last_id" ]] || ! wait_for_sd_result "$burst_last_id" "ok" "ok"; then
        echo "[FAIL] burst_wait_last"
        tail -n 160 "$output_path" >&2
        return 1
    fi

    wait_for_pattern_from_line "$start_line" "sdfat\[(manual|request)\]: mkdir_ok path=$burst_root" 120
    wait_for_pattern_from_line "$start_line" "sdfat\[(manual|request)\]: write_ok path=$burst_file bytes=1 verify=ok" 120
    wait_for_pattern_from_line "$start_line" "sdfat\[(manual|request)\]: append_ok path=$burst_file bytes=2" 120
    wait_for_pattern_from_line "$start_line" "sdfat\[(manual|request)\]: stat_ok path=$burst_file kind=file" 120
    wait_for_pattern_from_line "$start_line" "sdfat\[(manual|request)\]: read_ok path=$burst_file bytes=3 preview_hex=414243" 120

    if tail -n +$((start_line + 1)) "$output_path" | rg -q "SDFAT(MKDIR|WRITE|APPEND|STAT|READ) BUSY"; then
        echo "[FAIL] burst_no_busy"
        tail -n 160 "$output_path" >&2
        return 1
    fi

    run_step "burst_cleanup_file" \
        "SDFATRM $burst_file" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$burst_file"

    run_step "burst_cleanup_dir" \
        "SDFATRM $burst_root" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$burst_root"

    echo "[PASS] burst_sequence"
}

run_failure_sequence() {
    local fail_root="/f${run_tag}"
    local rename_root="/r${run_tag}"
    local file_a="$rename_root/a.txt"
    local file_b="$rename_root/b.txt"

    run_step "fail_mkdir_nonempty" \
        "SDFATMKDIR $fail_root" \
        "SDFATMKDIR" \
        "sdfat\[(manual|request)\]: mkdir_ok path=$fail_root"

    run_step "fail_write_nonempty" \
        "SDFATWRITE $fail_root/child.txt x" \
        "SDFATWRITE" \
        "sdfat\[(manual|request)\]: write_ok path=$fail_root/child.txt bytes=1 verify=ok"

    run_step_expect_error "fail_rm_non_empty_dir" \
        "SDFATRM $fail_root" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_error path=$fail_root err=NotEmpty" \
        "operation_failed"

    run_step "fail_cleanup_child" \
        "SDFATRM $fail_root/child.txt" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$fail_root/child.txt"

    run_step "fail_cleanup_dir" \
        "SDFATRM $fail_root" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$fail_root"

    run_step "fail_mkdir_rename" \
        "SDFATMKDIR $rename_root" \
        "SDFATMKDIR" \
        "sdfat\[(manual|request)\]: mkdir_ok path=$rename_root"

    run_step "fail_write_a" \
        "SDFATWRITE $file_a one" \
        "SDFATWRITE" \
        "sdfat\[(manual|request)\]: write_ok path=$file_a bytes=3 verify=ok"

    run_step "fail_write_b" \
        "SDFATWRITE $file_b two" \
        "SDFATWRITE" \
        "sdfat\[(manual|request)\]: write_ok path=$file_b bytes=3 verify=ok"

    run_step_expect_error "fail_rename_collision" \
        "SDFATREN $file_a $file_b" \
        "SDFATREN" \
        "sdfat\[(manual|request)\]: ren_error src=$file_a dst=$file_b err=AlreadyExists" \
        "operation_failed"

    run_step_expect_error "fail_read_not_found" \
        "SDFATREAD $rename_root/missing.txt" \
        "SDFATREAD" \
        "sdfat\[(manual|request)\]: read_error path=$rename_root/missing.txt err=NotFound" \
        "not_found"

    run_step_expect_error "fail_rw_refused_lba0" \
        "SDRWVERIFY 0" \
        "SDRWVERIFY" \
        "sdrw\[(manual|request)\]: refused_lba0" \
        "refused_lba0"

    run_step "fail_cleanup_a" \
        "SDFATRM $file_a" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$file_a"

    run_step "fail_cleanup_b" \
        "SDFATRM $file_b" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$file_b"

    run_step "fail_cleanup_rename_dir" \
        "SDFATRM $rename_root" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$rename_root"

    local long_payload
    long_payload="$(printf 'x%.0s' {1..260})"
    run_raw_command_expect_pattern "fail_oversize_payload_cmd_err" \
        "SDFATWRITE $base_path/overflow.txt $long_payload" \
        "^CMD ERR"
}

run_baseline_sequence() {
    run_step "mkdir" \
        "SDFATMKDIR $base_path" \
        "SDFATMKDIR" \
        "sdfat\[(manual|request)\]: mkdir_ok path=$base_path"

    run_step "write" \
        "SDFATWRITE $test_file hello" \
        "SDFATWRITE" \
        "sdfat\[(manual|request)\]: write_ok path=$test_file bytes=5 verify=ok"

    run_step "read_hello" \
        "SDFATREAD $test_file" \
        "SDFATREAD" \
        "sdfat\[(manual|request)\]: read_ok path=$test_file bytes=5 preview_hex=68656c6c6f"

    run_step "append" \
        "SDFATAPPEND $test_file _world" \
        "SDFATAPPEND" \
        "sdfat\[(manual|request)\]: append_ok path=$test_file bytes=6"

    run_step "read_hello_world" \
        "SDFATREAD $test_file" \
        "SDFATREAD" \
        "sdfat\[(manual|request)\]: read_ok path=$test_file bytes=11 preview_hex=68656c6c6f5f776f726c64"

    run_step "stat" \
        "SDFATSTAT $test_file" \
        "SDFATSTAT" \
        "sdfat\[(manual|request)\]: stat_ok path=$test_file kind=file"

    run_step "truncate" \
        "SDFATTRUNC $test_file 5" \
        "SDFATTRUNC" \
        "sdfat\[(manual|request)\]: trunc_ok path=$test_file size=5"

    run_step "rename" \
        "SDFATREN $test_file $test_file_renamed" \
        "SDFATREN" \
        "sdfat\[(manual|request)\]: ren_ok src=$test_file dst=$test_file_renamed"

    run_step "remove_file" \
        "SDFATRM $test_file_renamed" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$test_file_renamed"

    run_step "remove_dir" \
        "SDFATRM $base_path" \
        "SDFATRM" \
        "sdfat\[(manual|request)\]: rm_ok path=$base_path"

    run_step "raw_rw_verify" \
        "SDRWVERIFY $verify_lba" \
        "SDRWVERIFY" \
        "sdrw\[(manual|request)\]: verify_ok lba=$verify_lba bytes=512"
}

test_file="$base_path/io.txt"
test_file_renamed="$base_path/io2.txt"

echo "Running SD-card command validation on $ESPFLASH_PORT"
echo "Test root path: $base_path"
echo "Suite: $suite"

run_step "probe" \
    "SDPROBE" \
    "SDPROBE" \
    "sdprobe\\[(manual|request)\\]: card_detected"

if [[ "$suite" == "all" || "$suite" == "baseline" ]]; then
    run_baseline_sequence
fi

if [[ "$suite" == "all" || "$suite" == "burst" ]]; then
    run_burst_sequence
fi

if [[ "$suite" == "all" || "$suite" == "failures" ]]; then
    run_failure_sequence
fi

echo "SD-card hardware test passed"
echo "Log: $output_path"
