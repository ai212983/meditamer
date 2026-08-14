#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
# shellcheck source=../../lib/serial_port.sh
source "$script_dir/../../lib/serial_port.sh"
# shellcheck source=../../lib/experiment_novelty_guard.sh
source "$script_dir/../../lib/experiment_novelty_guard.sh"
load_repo_env_file_if_present ".env.local"
ensure_hostctl_net_port "test_wifi_regression_gate.sh"
enforce_wifi_upload_experiment_novelty_guard "test_wifi_regression_gate.sh"

reject_legacy_env_vars "test_wifi_regression_gate.sh" \
    HOSTCTL_PORT \
    HOSTCTL_BAUD \
    HOSTCTL_WIFI_UPLOAD_CYCLES \
    HOSTCTL_WIFI_UPLOAD_SSID \
    HOSTCTL_WIFI_UPLOAD_PASSWORD

required=(
    HOSTCTL_NET_PORT
    HOSTCTL_NET_BAUD
    HOSTCTL_NET_SSID
    HOSTCTL_NET_PASSWORD
    HOSTCTL_NET_POLICY_PATH
)
for name in "${required[@]}"; do
    if [[ -z "${!name:-}" ]]; then
        echo "test_wifi_regression_gate.sh: missing required env var: $name" >&2
        exit 1
    fi
done

if ! command -v jq >/dev/null 2>&1; then
    echo "test_wifi_regression_gate.sh: jq is required to write report.json" >&2
    exit 1
fi

panic_context_lines="${HOSTCTL_NET_PANIC_CONTEXT_LINES:-80}"
if ! [[ "$panic_context_lines" =~ ^[0-9]+$ ]]; then
    echo "test_wifi_regression_gate.sh: HOSTCTL_NET_PANIC_CONTEXT_LINES must be an integer" >&2
    exit 1
fi
panic_auto_troubleshoot="${HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT:-1}"
if [[ "$panic_auto_troubleshoot" != "0" && "$panic_auto_troubleshoot" != "1" ]]; then
    echo "test_wifi_regression_gate.sh: HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT must be 0 or 1" >&2
    exit 1
fi
soak_cycles="${HOSTCTL_NET_SOAK_CYCLES:-0}"
if ! [[ "$soak_cycles" =~ ^[0-9]+$ ]]; then
    echo "test_wifi_regression_gate.sh: HOSTCTL_NET_SOAK_CYCLES must be an integer" >&2
    exit 1
fi

run_id="$(date +%Y%m%d_%H%M%S)"
run_dir="${HOSTCTL_NET_REGRESSION_OUTPUT_DIR:-./logs/wifi_regression_gate_${run_id}}"
mkdir -p "$run_dir"
report_path="${HOSTCTL_NET_REGRESSION_REPORT_PATH:-${run_dir}/report.json}"

epoch_ms() {
    local ts
    ts="$(date +%s%3N 2>/dev/null || true)"
    if [[ "$ts" =~ ^[0-9]+$ ]]; then
        printf '%s\n' "$ts"
        return
    fi
    python3 -c 'import time; print(int(time.time() * 1000))'
}

to_abs_path() {
    local path="$1"
    if [[ "$path" == /* ]]; then
        printf '%s\n' "$path"
    else
        printf '%s/%s\n' "$repo_root" "${path#./}"
    fi
}

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
panic_detected=false
unexpected_reboot_detected=false
panic_class=""
panic_marker_line=""
panic_marker_index=""
panic_source_log=""
panic_excerpt_path="${HOSTCTL_NET_PANIC_EXCERPT_PATH:-${run_dir}/panic_excerpt.log}"
troubleshoot_invoked=false
troubleshoot_log_path=""
failure_stage=""
failure_class=""
failure_code=""

stage1_name="discovery_debug"
stage1_log="${run_dir}/${stage1_name}.log"
stage1_log_abs="$(to_abs_path "$stage1_log")"
stage1_cmd="HOSTCTL_NET_LOG_PATH=${stage1_log} scripts/tests/hw/test_wifi_discovery_debug.sh"
stage1_status="skipped"
stage1_duration_ms=0

stage2_name="acceptance_1_cycle"
stage2_log="${run_dir}/${stage2_name}.log"
stage2_log_abs="$(to_abs_path "$stage2_log")"
stage2_cmd="HOSTCTL_NET_CYCLES=1 HOSTCTL_NET_LOG_PATH=${stage2_log} scripts/tests/hw/test_wifi_acceptance.sh"
stage2_status="skipped"
stage2_duration_ms=0

stage3_name="acceptance_3_cycle"
stage3_log="${run_dir}/${stage3_name}.log"
stage3_log_abs="$(to_abs_path "$stage3_log")"
stage3_cmd="HOSTCTL_NET_CYCLES=3 HOSTCTL_NET_LOG_PATH=${stage3_log} scripts/tests/hw/test_wifi_acceptance.sh"
stage3_status="skipped"
stage3_duration_ms=0

stage4_name="acceptance_soak"
stage4_log="${run_dir}/${stage4_name}.log"
stage4_log_abs="$(to_abs_path "$stage4_log")"
stage4_cmd="HOSTCTL_NET_CYCLES=${soak_cycles} HOSTCTL_NET_LOG_PATH=${stage4_log} scripts/tests/hw/test_wifi_acceptance.sh"
stage4_status="skipped"
stage4_duration_ms=0

classify_panic_line() {
    local line="$1"
    local lower
    lower="$(printf '%s' "$line" | tr '[:upper:]' '[:lower:]')"
    if [[ "$lower" == *"boot_reset reason="* ]]; then
        printf 'runtime_unexpected_reboot'
        return
    fi
    if [[ "$lower" == *"guru meditation"* ]]; then
        printf 'runtime_panic_guru'
        return
    fi
    if [[ "$lower" == *"stack overflow"* || "$lower" == *"stack smashing"* ]]; then
        printf 'runtime_panic_stack'
        return
    fi
    if [[ "$lower" == *"assertion failed"* ]]; then
        printf 'runtime_panic_assert'
        return
    fi
    printf 'runtime_panic_other'
}

scan_panic_in_log() {
    local log_path="$1"
    if [[ ! -f "$log_path" ]]; then
        return
    fi
    if [[ "$panic_detected" == "true" ]]; then
        return
    fi

    local candidate marker_index marker_line marker_lower
    local startup_boot_reset_line_max=120
    while IFS= read -r candidate; do
        marker_index="${candidate%%:*}"
        marker_line="${candidate#*:}"
        marker_lower="$(printf '%s' "$marker_line" | tr '[:upper:]' '[:lower:]')"
        # Ignore the first boot reset signature at startup; we only want
        # unexpected resets after workflow activity has begun.
        if [[ "$marker_lower" == *"boot_reset reason="* ]] && (( marker_index <= startup_boot_reset_line_max )); then
            continue
        fi

        panic_detected=true
        panic_marker_index="$marker_index"
        panic_marker_line="$marker_line"
        panic_source_log="$log_path"
        panic_class="$(classify_panic_line "$panic_marker_line")"
        if [[ "$panic_class" == "runtime_unexpected_reboot" ]]; then
            unexpected_reboot_detected=true
        fi
        break
    done < <(grep -Ein 'guru meditation|panic|backtrace|stack overflow|stack smashing|assertion failed|BOOT_RESET reason=' "$log_path" || true)

    if [[ "$panic_detected" != "true" ]]; then
        return
    fi

    local start_line=1
    if (( panic_marker_index > panic_context_lines )); then
        start_line=$((panic_marker_index - panic_context_lines))
    fi
    local end_line=$((panic_marker_index + panic_context_lines))
    sed -n "${start_line},${end_line}p" "$panic_source_log" >"$panic_excerpt_path"
}

extract_failure_from_log() {
    local log_path="$1"
    if [[ ! -f "$log_path" ]]; then
        return
    fi
    local last_status
    last_status="$(grep -E 'NET_STATUS \{' "$log_path" | tail -n1 || true)"
    if [[ -n "$last_status" ]]; then
        local parsed_class parsed_code
        parsed_class="$(printf '%s' "$last_status" | sed -n 's/.*"failure_class":"\([^"]*\)".*/\1/p')"
        parsed_code="$(printf '%s' "$last_status" | sed -n 's/.*"failure_code":\([0-9]\+\).*/\1/p')"
        if [[ -n "$parsed_class" ]]; then
            failure_class="$parsed_class"
        fi
        if [[ -n "$parsed_code" ]]; then
            failure_code="$parsed_code"
        fi
    fi

    if [[ -z "$failure_class" || "$failure_class" == "none" ]]; then
        local inferred_host_class
        inferred_host_class="$(infer_host_failure_class_from_log "$log_path")"
        if [[ -n "$inferred_host_class" ]]; then
            failure_class="$inferred_host_class"
            if [[ -z "$failure_code" ]]; then
                case "$failure_class" in
                    host_health_send_fail) failure_code="9001" ;;
                    host_transport_send_fail) failure_code="9002" ;;
                    host_transport_connection_reset) failure_code="9003" ;;
                    host_transport_connect_refused) failure_code="9004" ;;
                esac
            fi
        fi
    fi
}

infer_host_failure_class_from_log() {
    local log_path="$1"
    if [[ ! -f "$log_path" ]]; then
        return
    fi

    local marker_class
    marker_class="$(grep -Eo 'HOST_FAILURE class=[A-Za-z0-9_]+' "$log_path" | tail -n1 | sed -E 's/.*class=//' || true)"
    if [[ -n "$marker_class" ]]; then
        printf '%s\n' "$marker_class"
        return
    fi

    if grep -Eiq 'health check failed: GET .*send failed|health check failed: GET .*connection reset|health check failed: GET .*connection refused|health check failed: GET .*error sending request' "$log_path"; then
        printf 'host_health_send_fail\n'
        return
    fi
    if grep -Eiq 'connection refused|connect error.*refused' "$log_path"; then
        printf 'host_transport_connect_refused\n'
        return
    fi
    if grep -Eiq 'send failed|error sending request' "$log_path"; then
        printf 'host_transport_send_fail\n'
        return
    fi
    if grep -Eiq 'connection reset|broken pipe|body read err=ConnectionReset' "$log_path"; then
        printf 'host_transport_connection_reset\n'
        return
    fi
}

run_stage() {
    local stage_name="$1"
    local stage_log="$2"
    shift 2
    local started_epoch_ms ended_epoch_ms rc
    started_epoch_ms="$(epoch_ms)"
    set +e
    "$@"
    rc=$?
    set -e
    ended_epoch_ms="$(epoch_ms)"
    local duration=$((ended_epoch_ms - started_epoch_ms))

    case "$stage_name" in
        "$stage1_name")
            stage1_duration_ms="$duration"
            if (( rc == 0 )); then stage1_status="passed"; else stage1_status="failed"; fi
            ;;
        "$stage2_name")
            stage2_duration_ms="$duration"
            if (( rc == 0 )); then stage2_status="passed"; else stage2_status="failed"; fi
            ;;
        "$stage3_name")
            stage3_duration_ms="$duration"
            if (( rc == 0 )); then stage3_status="passed"; else stage3_status="failed"; fi
            ;;
        "$stage4_name")
            stage4_duration_ms="$duration"
            if (( rc == 0 )); then stage4_status="passed"; else stage4_status="failed"; fi
            ;;
    esac

    scan_panic_in_log "$stage_log"
    if [[ "$panic_detected" == "true" ]]; then
        return 2
    fi
    return "$rc"
}

echo "wifi_regression_gate: run_id=${run_id} output_dir=${run_dir}"

if run_stage "$stage1_name" "$stage1_log_abs" env HOSTCTL_NET_LOG_PATH="$stage1_log_abs" "$script_dir/test_wifi_discovery_debug.sh"; then
    :
else
    failure_stage="$stage1_name"
fi

if [[ -z "$failure_stage" ]]; then
    if run_stage "$stage2_name" "$stage2_log_abs" env HOSTCTL_NET_CYCLES=1 HOSTCTL_NET_LOG_PATH="$stage2_log_abs" "$script_dir/test_wifi_acceptance.sh"; then
        :
    else
        failure_stage="$stage2_name"
    fi
fi

if [[ -z "$failure_stage" ]]; then
    if run_stage "$stage3_name" "$stage3_log_abs" env HOSTCTL_NET_CYCLES=3 HOSTCTL_NET_LOG_PATH="$stage3_log_abs" "$script_dir/test_wifi_acceptance.sh"; then
        :
    else
        failure_stage="$stage3_name"
    fi
fi

if [[ -z "$failure_stage" && "$soak_cycles" -gt 0 ]]; then
    if run_stage "$stage4_name" "$stage4_log_abs" env HOSTCTL_NET_CYCLES="$soak_cycles" HOSTCTL_NET_LOG_PATH="$stage4_log_abs" "$script_dir/test_wifi_acceptance.sh"; then
        :
    else
        failure_stage="$stage4_name"
    fi
fi

if [[ -n "$failure_stage" ]]; then
    case "$failure_stage" in
        "$stage1_name") extract_failure_from_log "$stage1_log_abs" ;;
        "$stage2_name") extract_failure_from_log "$stage2_log_abs" ;;
        "$stage3_name") extract_failure_from_log "$stage3_log_abs" ;;
        "$stage4_name") extract_failure_from_log "$stage4_log_abs" ;;
    esac
fi

if [[ "$panic_detected" == "true" && "$panic_auto_troubleshoot" == "1" ]]; then
    troubleshoot_invoked=true
    troubleshoot_log_path="${run_dir}/troubleshoot.log"
    echo "wifi_regression_gate: panic detected, running troubleshoot workflow"
    set +e
    HOSTCTL_PORT="${HOSTCTL_NET_PORT}" "$repo_root/scripts/hostctl.sh" test troubleshoot \
        --build-mode debug --output "$troubleshoot_log_path"
    set -e
fi

boot_reset_before="$(grep -hE 'METRICS BOOT reset_code=' "$stage1_log_abs" "$stage2_log_abs" "$stage3_log_abs" "$stage4_log_abs" 2>/dev/null | sed -n 's/.*reset_code=\([0-9]\+\).*/\1/p' | head -n1 || true)"
boot_reset_after="$(grep -hE 'METRICS BOOT reset_code=' "$stage1_log_abs" "$stage2_log_abs" "$stage3_log_abs" "$stage4_log_abs" 2>/dev/null | sed -n 's/.*reset_code=\([0-9]\+\).*/\1/p' | tail -n1 || true)"

final_status="passed"
if [[ -n "$failure_stage" || "$panic_detected" == "true" ]]; then
    final_status="failed"
fi
finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$(dirname "$report_path")"
jq -n \
    --arg run_id "$run_id" \
    --arg started_at "$started_at" \
    --arg finished_at "$finished_at" \
    --arg port "$HOSTCTL_NET_PORT" \
    --arg baud "$HOSTCTL_NET_BAUD" \
    --arg policy_path "$HOSTCTL_NET_POLICY_PATH" \
    --arg profile_path "${HOSTCTL_NET_DISCOVERY_PROFILE_PATH:-./tools/hostctl/scenarios/wifi-discovery-debug.default.toml}" \
    --arg s1_name "$stage1_name" \
    --arg s1_status "$stage1_status" \
    --arg s1_cmd "$stage1_cmd" \
    --arg s1_log "$stage1_log" \
    --argjson s1_duration "$stage1_duration_ms" \
    --arg s2_name "$stage2_name" \
    --arg s2_status "$stage2_status" \
    --arg s2_cmd "$stage2_cmd" \
    --arg s2_log "$stage2_log" \
    --argjson s2_duration "$stage2_duration_ms" \
    --arg s3_name "$stage3_name" \
    --arg s3_status "$stage3_status" \
    --arg s3_cmd "$stage3_cmd" \
    --arg s3_log "$stage3_log" \
    --argjson s3_duration "$stage3_duration_ms" \
    --arg s4_name "$stage4_name" \
    --arg s4_status "$stage4_status" \
    --arg s4_cmd "$stage4_cmd" \
    --arg s4_log "$stage4_log" \
    --argjson s4_duration "$stage4_duration_ms" \
    --arg final_status "$final_status" \
    --arg failure_stage "$failure_stage" \
    --arg failure_class "$failure_class" \
    --arg failure_code "$failure_code" \
    --arg panic_class "$panic_class" \
    --arg panic_marker_line "$panic_marker_line" \
    --arg panic_marker_index "$panic_marker_index" \
    --arg panic_excerpt_path "$panic_excerpt_path" \
    --arg panic_source_log "$panic_source_log" \
    --arg boot_reset_before "$boot_reset_before" \
    --arg boot_reset_after "$boot_reset_after" \
    --arg troubleshoot_log_path "$troubleshoot_log_path" \
    --argjson panic_detected "$panic_detected" \
    --argjson unexpected_reboot_detected "$unexpected_reboot_detected" \
    --argjson troubleshoot_invoked "$troubleshoot_invoked" \
    '{
        run_id: $run_id,
        started_at: $started_at,
        finished_at: $finished_at,
        device: { port: $port, baud: ($baud | tonumber) },
        policy_path: $policy_path,
        profile_path: $profile_path,
        stages: [
            { name: $s1_name, status: $s1_status, command: $s1_cmd, log_path: $s1_log, duration_ms: $s1_duration },
            { name: $s2_name, status: $s2_status, command: $s2_cmd, log_path: $s2_log, duration_ms: $s2_duration },
            { name: $s3_name, status: $s3_status, command: $s3_cmd, log_path: $s3_log, duration_ms: $s3_duration },
            { name: $s4_name, status: $s4_status, command: $s4_cmd, log_path: $s4_log, duration_ms: $s4_duration }
        ],
        final_status: $final_status,
        failure_stage: (if $failure_stage == "" then null else $failure_stage end),
        failure_class: (if $failure_class == "" then null else $failure_class end),
        failure_code: (if $failure_code == "" then null else ($failure_code | tonumber) end),
        panic_detected: $panic_detected,
        panic_class: (if $panic_class == "" then null else $panic_class end),
        panic_marker_line: (if $panic_marker_line == "" then null else $panic_marker_line end),
        panic_marker_index: (if $panic_marker_index == "" then null else ($panic_marker_index | tonumber) end),
        panic_excerpt_path: (if $panic_detected then $panic_excerpt_path else null end),
        panic_source_log: (if $panic_source_log == "" then null else $panic_source_log end),
        unexpected_reboot_detected: $unexpected_reboot_detected,
        boot_reset_code_before: (if $boot_reset_before == "" then null else ($boot_reset_before | tonumber) end),
        boot_reset_code_after: (if $boot_reset_after == "" then null else ($boot_reset_after | tonumber) end),
        troubleshoot_invoked: $troubleshoot_invoked,
        troubleshoot_log_path: (if $troubleshoot_log_path == "" then null else $troubleshoot_log_path end),
        next_action_hint: (if $final_status == "passed"
            then "regression gate passed"
            else "inspect failure stage log, panic excerpt, and troubleshoot summary before applying one targeted fix"
            end)
    }' >"$report_path"

echo
echo "wifi_regression_gate summary"
printf "%-20s %-8s %-10s %s\n" "stage" "status" "duration" "log"
printf "%-20s %-8s %-10s %s\n" "$stage1_name" "$stage1_status" "${stage1_duration_ms}ms" "$stage1_log"
printf "%-20s %-8s %-10s %s\n" "$stage2_name" "$stage2_status" "${stage2_duration_ms}ms" "$stage2_log"
printf "%-20s %-8s %-10s %s\n" "$stage3_name" "$stage3_status" "${stage3_duration_ms}ms" "$stage3_log"
printf "%-20s %-8s %-10s %s\n" "$stage4_name" "$stage4_status" "${stage4_duration_ms}ms" "$stage4_log"
echo "final_status=${final_status}"
echo "report_path=${report_path}"
if [[ "$panic_detected" == "true" ]]; then
    echo "panic_class=${panic_class}"
    echo "panic_excerpt_path=${panic_excerpt_path}"
fi

if [[ "$final_status" != "passed" ]]; then
    exit 1
fi
