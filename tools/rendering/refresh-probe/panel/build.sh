#!/usr/bin/env bash

# Build one stand-alone Inkplate panel probe binary at production timing.
#
# Every mode uses the compiled-in production waveform; this helper only
# selects the probe mode and run length. Flashing and capture stay in the
# canonical hostctl flash-capture workflow.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../../.." && pwd)"
target_dir="$repo_root/targets/meditamer-inkplate"
spec_path="$script_dir/../common/spec.toml"

python3 "$script_dir/../common/generate.py" --check
read -r default_partial_samples default_partial_profile_samples default_partial_span_samples default_full_performance_samples default_full_profile_samples default_full_soak_cycles default_interval_ms < <(
    python3 - "$spec_path" <<'PY'
import sys
import tomllib
with open(sys.argv[1], "rb") as source:
    spec = tomllib.load(source)
print(
    spec["partial_samples"],
    spec["partial_profile_samples"],
    spec["partial_span_samples"],
    spec["full_performance_samples"],
    spec["full_profile_samples"],
    spec["full_soak_cycles"],
    spec["refresh_interval_ms"],
)
PY
)

mode="${1:-}"
case "$mode" in
    partial-1bit|profile-partial-1bit|partial-spans|full-1bit|full-3bit|profile-full-1bit|profile-full-3bit|full-soak|transition|ghosting) ;;
    *)
        echo "usage: $0 partial-1bit" >&2
        echo "       $0 profile-partial-1bit" >&2
        echo "       $0 partial-spans [samples-per-span] [interval-ms]" >&2
        echo "       $0 full-1bit" >&2
        echo "       $0 full-3bit" >&2
        echo "       $0 profile-full-1bit" >&2
        echo "       $0 profile-full-3bit" >&2
        echo "       $0 full-soak [cycles] [interval-ms]" >&2
        echo "       $0 transition [pairs] [interval-ms]" >&2
        echo "       $0 ghosting [conditioning-cycles] [interval-ms]" >&2
        exit 2
        ;;
esac

validate_cycles() {
    if ! [[ "$1" =~ ^[1-9][0-9]*$ ]]; then
        echo "cycles must be a positive integer" >&2
        exit 2
    fi
}

validate_interval_ms() {
    if ! [[ "$1" =~ ^[0-9]+$ ]]; then
        echo "interval-ms must be a non-negative integer" >&2
        exit 2
    fi
}

candidate_env=(
    FIRMWARE_BIN=panel-refresh-probe
)
cargo_features=panel-refresh-probe
# Fixed production labels kept in artifact names for capture compatibility.
partial_hold=reference
full_hold=12
interval_ms="$default_interval_ms"

case "$mode" in
    full-1bit|profile-full-1bit|full-soak|transition|ghosting)
        default_panel_i2c_khz=400
        ;;
    *) default_panel_i2c_khz=100 ;;
esac
panel_i2c_khz="${MEDITAMER_REFRESH_PROBE_I2C_KHZ:-$default_panel_i2c_khz}"

case "$panel_i2c_khz" in
    100) ;;
    400) candidate_env+=(MEDITAMER_PANEL_I2C_400_KHZ=1) ;;
    *)
        echo "MEDITAMER_REFRESH_PROBE_I2C_KHZ must be 100 or 400" >&2
        exit 2
        ;;
esac

case "$mode" in
    partial-1bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_partial_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_BENCHMARK_PARTIAL_1BIT=1)
        ;;
    profile-partial-1bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_partial_profile_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_PROFILE_PARTIAL_1BIT=1)
        candidate_env+=(MEDITAMER_PANEL_PARTIAL_SCAN_PHASE_TIMING=1)
        ;;
    partial-spans)
        [[ "$#" -le 3 ]] || { echo "partial-spans accepts optional samples-per-span and interval-ms arguments" >&2; exit 2; }
        cycles="${2:-$default_partial_span_samples}"
        interval_ms="${3:-$default_interval_ms}"
        candidate_env+=(MEDITAMER_PANEL_SOAK_PARTIAL_SPANS=1)
        ;;
    full-1bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_full_performance_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_BENCHMARK_FULL_1BIT=1)
        ;;
    full-3bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_full_performance_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_BENCHMARK_FULL_3BIT=1)
        ;;
    profile-full-1bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_full_profile_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_PROFILE_FULL_1BIT=1)
        candidate_env+=(MEDITAMER_PANEL_FULL_SCAN_PHASE_TIMING=1)
        ;;
    profile-full-3bit)
        [[ "$#" -le 1 ]] || { echo "$mode takes no additional arguments" >&2; exit 2; }
        cycles="$default_full_profile_samples"
        interval_ms="$default_interval_ms"
        candidate_env+=(MEDITAMER_PANEL_PROFILE_FULL_3BIT=1)
        ;;
    full-soak)
        [[ "$#" -le 3 ]] || { echo "full-soak accepts optional cycles and interval-ms arguments" >&2; exit 2; }
        cycles="${2:-$default_full_soak_cycles}"
        interval_ms="${3:-$default_interval_ms}"
        candidate_env+=(MEDITAMER_PANEL_SOAK_FULL_ONLY=1)
        ;;
    transition)
        [[ "$#" -le 3 ]] || { echo "transition accepts optional pairs and interval-ms arguments" >&2; exit 2; }
        cycles="${2:-$default_partial_samples}"
        interval_ms="${3:-$default_interval_ms}"
        candidate_env+=(MEDITAMER_PANEL_SOAK_PARTIAL_THEN_FULL=1)
        ;;
    ghosting)
        [[ "$#" -le 3 ]] || { echo "ghosting accepts optional conditioning-cycles and interval-ms arguments" >&2; exit 2; }
        cycles="${2:-20}"
        interval_ms="${3:-0}"
        candidate_env+=(MEDITAMER_PANEL_SOAK_GHOSTING=1)
        ;;
esac

validate_cycles "$cycles"
validate_interval_ms "$interval_ms"
candidate_env+=(CARGO_FEATURES="$cargo_features")
candidate_env+=(MEDITAMER_PANEL_SOAK_CYCLES="$cycles")
candidate_env+=(MEDITAMER_PANEL_SOAK_REFRESH_INTERVAL_MS="$interval_ms")
candidate_env+=(MEDITAMER_PANEL_SOAK_CHECKPOINT_EVERY=0)
candidate_env+=(MEDITAMER_PANEL_SOAK_CHECKPOINT_HOLD_MS=0)

# Remove every retired timing selector before applying this build. An
# inherited selector must never silently change the compiled waveform.
while IFS= read -r panel_variable; do
    unset "$panel_variable"
done < <(compgen -v MEDITAMER_PANEL_)

env "${candidate_env[@]}" "$target_dir/build.sh" release minimal

elf="$target_dir/target/xtensa-esp32-none-elf/release/panel-refresh-probe"
if [[ ! -f "$elf" ]]; then
    echo "missing built ELF: $elf" >&2
    exit 1
fi

artifact_suffix="${cycles}cycles"
if [[ "$interval_ms" != 250 ]]; then
    artifact_suffix+="-${interval_ms}ms-interval"
fi
if [[ "$panel_i2c_khz" != "$default_panel_i2c_khz" ]]; then
    artifact_suffix+="-i2c-${panel_i2c_khz}khz"
fi
artifact_dir="$repo_root/logs/refresh-probe/panel/builds/${mode}-partial-${partial_hold}-full-${full_hold}-${artifact_suffix}"
mkdir -p "$artifact_dir"
install -m 0644 "$elf" "$artifact_dir/firmware.elf"
sha256sum "$artifact_dir/firmware.elf"
echo "mode=$mode partial_hold=$partial_hold full_hold=$full_hold panel_i2c_khz=$panel_i2c_khz cycles=$cycles interval_ms=$interval_ms"
echo "artifact=$artifact_dir/firmware.elf"
