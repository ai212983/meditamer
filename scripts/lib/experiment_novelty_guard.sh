#!/usr/bin/env bash

set -euo pipefail

_EXPERIMENT_GUARD_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

_experiment_guard_repo_root() {
    cd "$_EXPERIMENT_GUARD_LIB_DIR/../.." && pwd
}

_experiment_guard_ledger_path() {
    local repo_root
    repo_root="$(_experiment_guard_repo_root)"
    printf '%s/docs/development/wifi-upload-decision-ledger.md\n' "$repo_root"
}

_experiment_guard_docs_hint() {
    printf '%s\n' "docs/development/wifi-upload-decision-ledger.md"
}

_experiment_guard_norm() {
    printf '%s' "$1" | tr -d '_`,' | tr '[:upper:]' '[:lower:]'
}

_experiment_guard_equals() {
    local left right
    left="$(_experiment_guard_norm "$1")"
    right="$(_experiment_guard_norm "$2")"
    [[ "$left" == "$right" ]]
}

_experiment_guard_check_knob() {
    local ledger="$1"
    local canonical_key="$2"
    local source_var="$3"
    local source_value="$4"
    local default_value="$5"
    local caller="$6"

    [[ -n "$source_value" ]] || return 0
    grep -Fq "$canonical_key" "$ledger" || return 0
    _experiment_guard_equals "$source_value" "$default_value" && return 0

    echo "${caller}: experiment novelty guard blocked ${source_var}=${source_value}" >&2
    echo "${caller}: ${canonical_key} already has a durable decision in $(_experiment_guard_docs_hint)." >&2
    echo "${caller}: to reconfirm intentionally, set HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE=1." >&2
    return 1
}

enforce_wifi_upload_experiment_novelty_guard() {
    local caller="${1:-script}"
    local enabled="${HOSTCTL_EXPERIMENT_NOVELTY_GUARD:-1}"
    local override="${HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE:-0}"
    local ledger

    if [[ "$enabled" != "0" && "$enabled" != "1" ]]; then
        echo "${caller}: HOSTCTL_EXPERIMENT_NOVELTY_GUARD must be 0 or 1" >&2
        return 1
    fi
    if [[ "$override" != "0" && "$override" != "1" ]]; then
        echo "${caller}: HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE must be 0 or 1" >&2
        return 1
    fi

    [[ "$enabled" == "1" ]] || return 0

    if [[ "$override" == "1" ]]; then
        echo "${caller}: novelty guard override enabled (HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE=1)." >&2
        return 0
    fi

    ledger="$(_experiment_guard_ledger_path)"
    if [[ ! -f "$ledger" ]]; then
        echo "${caller}: novelty guard ledger missing: ${ledger}" >&2
        return 1
    fi

    local rx_buf_target=""
    if [[ -n "${MEDITAMER_HTTP_RX_BUF_TARGET:-}" ]]; then
        rx_buf_target="${MEDITAMER_HTTP_RX_BUF_TARGET}"
        _experiment_guard_check_knob "$ledger" "HTTP_RX_BUF_TARGET" "MEDITAMER_HTTP_RX_BUF_TARGET" "$rx_buf_target" "65536" "$caller" || return 1
    elif [[ -n "${HTTP_RX_BUF_TARGET:-}" ]]; then
        rx_buf_target="${HTTP_RX_BUF_TARGET}"
        _experiment_guard_check_knob "$ledger" "HTTP_RX_BUF_TARGET" "HTTP_RX_BUF_TARGET" "$rx_buf_target" "65536" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_SD_UPLOAD_CHUNK_MAX:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "SD_UPLOAD_CHUNK_MAX_DEFAULT" "MEDITAMER_SD_UPLOAD_CHUNK_MAX" "${MEDITAMER_SD_UPLOAD_CHUNK_MAX}" "65536" "$caller" || return 1
    elif [[ -n "${SD_UPLOAD_CHUNK_MAX:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "SD_UPLOAD_CHUNK_MAX_DEFAULT" "SD_UPLOAD_CHUNK_MAX" "${SD_UPLOAD_CHUNK_MAX}" "65536" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_SD_SPI_DATA_MHZ:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "MEDITAMER_SD_SPI_DATA_MHZ" "MEDITAMER_SD_SPI_DATA_MHZ" "${MEDITAMER_SD_SPI_DATA_MHZ}" "36" "$caller" || return 1
    fi

    if [[ -n "${HOSTCTL_UPLOAD_TCP_NODELAY:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HOSTCTL_UPLOAD_TCP_NODELAY" "HOSTCTL_UPLOAD_TCP_NODELAY" "${HOSTCTL_UPLOAD_TCP_NODELAY}" "1" "$caller" || return 1
    fi

    if [[ -n "${HOSTCTL_NET_REUSE_UPLOAD_CLIENT:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HOSTCTL_NET_REUSE_UPLOAD_CLIENT" "HOSTCTL_NET_REUSE_UPLOAD_CLIENT" "${HOSTCTL_NET_REUSE_UPLOAD_CLIENT}" "0" "$caller" || return 1
    fi

    if [[ -n "${HOSTCTL_UPLOAD_DIRECT_BURST_SENDER:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HOSTCTL_UPLOAD_DIRECT_BURST_SENDER" "HOSTCTL_UPLOAD_DIRECT_BURST_SENDER" "${HOSTCTL_UPLOAD_DIRECT_BURST_SENDER}" "0" "$caller" || return 1
    fi

    if [[ -n "${HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS" "HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS" "${HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS}" "0" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT" "MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS" "${MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS}" "2" "$caller" || return 1
    elif [[ -n "${HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT" "HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS" "${HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS}" "2" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_COOP_YIELD_*_DEFAULT" "MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES" "${MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES}" "32768" "$caller" || return 1
    elif [[ -n "${HTTP_INGRESS_COOP_YIELD_BYTES:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_COOP_YIELD_*_DEFAULT" "HTTP_INGRESS_COOP_YIELD_BYTES" "${HTTP_INGRESS_COOP_YIELD_BYTES}" "32768" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_COOP_YIELD_*_DEFAULT" "MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS" "${MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS}" "64" "$caller" || return 1
    elif [[ -n "${HTTP_INGRESS_COOP_YIELD_READS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_INGRESS_COOP_YIELD_*_DEFAULT" "HTTP_INGRESS_COOP_YIELD_READS" "${HTTP_INGRESS_COOP_YIELD_READS}" "64" "$caller" || return 1
    fi

    if [[ -n "${MEDITAMER_HTTP_UPLOAD_BODY_READ_TIMEOUT_MS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_UPLOAD_BODY_READ_TIMEOUT_MS" "MEDITAMER_HTTP_UPLOAD_BODY_READ_TIMEOUT_MS" "${MEDITAMER_HTTP_UPLOAD_BODY_READ_TIMEOUT_MS}" "6000" "$caller" || return 1
    elif [[ -n "${HTTP_UPLOAD_BODY_READ_TIMEOUT_MS:-}" ]]; then
        _experiment_guard_check_knob "$ledger" "HTTP_UPLOAD_BODY_READ_TIMEOUT_MS" "HTTP_UPLOAD_BODY_READ_TIMEOUT_MS" "${HTTP_UPLOAD_BODY_READ_TIMEOUT_MS}" "6000" "$caller" || return 1
    fi
}
