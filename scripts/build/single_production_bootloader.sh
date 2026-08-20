#!/usr/bin/env bash
# Builds the pinned ESP-IDF v5.5.2 bootloader and partition table for the
# single-production layout (ADR-0014 Phase 2, config/partitions-single-production.csv).
# Mirrors scripts/build/ota_bootloader.sh (the A/B build) exactly, pointed at
# a distinct sdkconfig defaults file and build directory so the two layouts
# never clobber each other's build artifacts.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bootloader_idf_root="${IDF_PATH:-$repo_root/.embuild/espressif/esp-idf/v5.5.2}"
build_dir="${MEDITAMER_SINGLE_PRODUCTION_BOOTLOADER_BUILD_DIR:-$repo_root/target/single-production-bootloader}"
defaults_file="$repo_root/tools/ota_bootloader/sdkconfig.single-production.defaults"

if [[ ! -f "$bootloader_idf_root/export.sh" ]]; then
    echo "ESP-IDF export script not found: $bootloader_idf_root/export.sh" >&2
    exit 1
fi
if [[ ! -f "$defaults_file" ]]; then
    echo "single-production sdkconfig defaults not found: $defaults_file" >&2
    exit 1
fi

# shellcheck disable=SC1091
source "$bootloader_idf_root/export.sh" >/dev/null

idf_version="$(idf.py --version)"
if [[ "$idf_version" != "ESP-IDF v5.5.2" ]]; then
    echo "expected ESP-IDF v5.5.2, got: $idf_version" >&2
    exit 1
fi

expected_idf_commit="30aaf64524299d3bde422ca9a2848090d1bc5d0f"
idf_commit="$(git -C "$bootloader_idf_root" rev-parse HEAD)"
if [[ "$idf_commit" != "$expected_idf_commit" ]]; then
    echo "expected ESP-IDF commit $expected_idf_commit, got: $idf_commit" >&2
    exit 1
fi
if [[ -n "$(git -C "$bootloader_idf_root" status --porcelain=v1)" ]]; then
    echo "refusing to build the pinned bootloader from a dirty ESP-IDF checkout" >&2
    exit 1
fi

idf.py \
    -C "$repo_root/tools/ota_bootloader" \
    -B "$build_dir" \
    -D "SDKCONFIG=$build_dir/sdkconfig" \
    -D "SDKCONFIG_DEFAULTS=$defaults_file" \
    build

bootloader="$build_dir/bootloader/bootloader.bin"
partition_table="$build_dir/partition_table/partition-table.bin"
for artifact in "$bootloader" "$partition_table"; do
    if [[ ! -f "$artifact" ]]; then
        echo "expected build artifact not found: $artifact" >&2
        exit 1
    fi
done

shasum -a 256 "$bootloader" "$partition_table"
