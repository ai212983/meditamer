#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

ensure_tool() {
    local tool="$1"
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "missing required tool: $tool" >&2
        exit 1
    fi
}

validate_positive_integer() {
    local value="$1"
    local name="$2"
    if ! [[ "$value" =~ ^[0-9]+$ ]] || ((value <= 0)); then
        echo "$name must be a positive integer (got: $value)" >&2
        exit 1
    fi
}

resolve_output_dir() {
    local root="${WIFI_FLASH_DUMP_OUTPUT_ROOT:-$repo_root/logs/flash_dumps}"
    local stamp="${WIFI_FLASH_DUMP_TIMESTAMP:-$(date +%Y%m%d_%H%M%S)}"
    printf '%s/wifi_partitions_%s\n' "$root" "$stamp"
}

run_logged() {
    local stdout_file="$1"
    local stderr_file="$2"
    shift 2
    "$@" >"$stdout_file" 2>"$stderr_file"
}

dump_partition_md5() {
    local name="$1"
    local address="$2"
    local length="$3"
    local out_dir="$4"
    run_logged \
        "$out_dir/${name}_md5.stdout.log" \
        "$out_dir/${name}_md5.stderr.log" \
        espflash checksum-md5 \
        -p "$ESPFLASH_PORT" \
        -c esp32 \
        -B "$ESPFLASH_BAUD" \
        --skip-update-check \
        --address "$address" \
        --length "$length"
}

dump_partition_raw() {
    local name="$1"
    local address="$2"
    local length="$3"
    local out_dir="$4"
    local bin_path="$out_dir/${name}.bin"
    run_logged \
        "$out_dir/${name}.read.stdout.log" \
        "$out_dir/${name}.read.stderr.log" \
        espflash read-flash \
        -p "$ESPFLASH_PORT" \
        -c esp32 \
        -B "$ESPFLASH_BAUD" \
        --skip-update-check \
        --block-size "$WIFI_FLASH_DUMP_BLOCK_SIZE" \
        --max-in-flight "$WIFI_FLASH_DUMP_MAX_IN_FLIGHT" \
        "$address" \
        "$length" \
        "$bin_path"
    xxd -g 1 -l "$WIFI_FLASH_DUMP_HEXDUMP_BYTES" "$bin_path" \
        >"$out_dir/${name}.hexdump.txt"
}

append_summary_line() {
    local summary_file="$1"
    shift
    printf '%s\n' "$*" >>"$summary_file"
}

md5_from_stdout() {
    local stdout_file="$1"
    tr -d '[:space:]' <"$stdout_file"
}

main() {
    ensure_tool espflash
    ensure_tool xxd
    ensure_espflash_port "dump_wifi_partitions.sh" || exit 1

    export ESPFLASH_BAUD="${ESPFLASH_BAUD:-115200}"
    export WIFI_FLASH_DUMP_BLOCK_SIZE="${WIFI_FLASH_DUMP_BLOCK_SIZE:-0x100}"
    export WIFI_FLASH_DUMP_MAX_IN_FLIGHT="${WIFI_FLASH_DUMP_MAX_IN_FLIGHT:-1}"
    export WIFI_FLASH_DUMP_HEXDUMP_BYTES="${WIFI_FLASH_DUMP_HEXDUMP_BYTES:-128}"

    validate_positive_integer "$ESPFLASH_BAUD" "ESPFLASH_BAUD"
    validate_positive_integer "$WIFI_FLASH_DUMP_MAX_IN_FLIGHT" \
        "WIFI_FLASH_DUMP_MAX_IN_FLIGHT"
    validate_positive_integer "$WIFI_FLASH_DUMP_HEXDUMP_BYTES" \
        "WIFI_FLASH_DUMP_HEXDUMP_BYTES"

    local out_dir
    out_dir="$(resolve_output_dir)"
    mkdir -p "$out_dir"

    local summary_file="$out_dir/summary.txt"
    : >"$summary_file"

    local nvs_address="${WIFI_FLASH_DUMP_NVS_ADDRESS:-0x9000}"
    local nvs_length="${WIFI_FLASH_DUMP_NVS_LENGTH:-0x6000}"
    local phy_address="${WIFI_FLASH_DUMP_PHY_INIT_ADDRESS:-0xF000}"
    local phy_length="${WIFI_FLASH_DUMP_PHY_INIT_LENGTH:-0x1000}"

    append_summary_line "$summary_file" "port=$ESPFLASH_PORT"
    append_summary_line "$summary_file" "output_dir=$out_dir"
    append_summary_line "$summary_file" "baud=$ESPFLASH_BAUD"
    append_summary_line \
        "$summary_file" \
        "read_flash_block_size=$WIFI_FLASH_DUMP_BLOCK_SIZE"
    append_summary_line \
        "$summary_file" \
        "read_flash_max_in_flight=$WIFI_FLASH_DUMP_MAX_IN_FLIGHT"
    append_summary_line \
        "$summary_file" \
        "nvs address=$nvs_address length=$nvs_length"
    append_summary_line \
        "$summary_file" \
        "phy_init address=$phy_address length=$phy_length"

    dump_partition_md5 "nvs" "$nvs_address" "$nvs_length" "$out_dir"
    dump_partition_raw "nvs" "$nvs_address" "$nvs_length" "$out_dir"
    dump_partition_md5 "phy_init" "$phy_address" "$phy_length" "$out_dir"
    dump_partition_raw "phy_init" "$phy_address" "$phy_length" "$out_dir"

    append_summary_line \
        "$summary_file" \
        "nvs md5=$(md5_from_stdout "$out_dir/nvs_md5.stdout.log")"
    append_summary_line \
        "$summary_file" \
        "phy_init md5=$(md5_from_stdout "$out_dir/phy_init_md5.stdout.log")"
    append_summary_line \
        "$summary_file" \
        "nvs bytes=$(wc -c <"$out_dir/nvs.bin" | tr -d '[:space:]')"
    append_summary_line \
        "$summary_file" \
        "phy_init bytes=$(wc -c <"$out_dir/phy_init.bin" | tr -d '[:space:]')"

    echo "wifi partition dump complete: $out_dir"
    cat "$summary_file"
}

main "$@"
