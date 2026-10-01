#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../../.." && pwd)"
mode="${1:-all}"
case "$mode" in
    all|partial-1bit|profile-partial-1bit|full-1bit|full-3bit|profile-full-1bit|profile-full-3bit) ;;
    *)
        echo "usage: $0 [all|partial-1bit|profile-partial-1bit|full-1bit|full-3bit|profile-full-1bit|profile-full-3bit]" >&2
        exit 1
        ;;
esac
arduino_user_dir="$(arduino-cli config dump | awk '$1 == "user:" { print $2; exit }')"
reference_library="${INKPLATE_REFERENCE_LIBRARY:-$arduino_user_dir/libraries/InkplateLibrary}"
output_dir="${REFERENCE_BUILD_OUTPUT_DIR:-$repo_root/logs/refresh-probe/reference/build/$mode}"

if [[ ! -f "$reference_library/src/boards/Inkplate4TEMPERA/Inkplate4TEMPERADriver.cpp" ]]; then
    echo "missing installed Inkplate reference library at: $reference_library" >&2
    exit 1
fi
if ! grep -qx 'version=11.1.4' "$reference_library/library.properties"; then
    echo "reference timing patch requires InkplateLibrary 11.1.4: $reference_library" >&2
    exit 1
fi

temp_root="$(mktemp -d)"
trap 'rm -rf "$temp_root"' EXIT
temp_libraries="$temp_root/libraries"
mkdir -p "$temp_libraries/InkplateLibrary"
rsync -a --exclude .git "$reference_library/" "$temp_libraries/InkplateLibrary/"
patch -s -d "$temp_libraries/InkplateLibrary" -p1 \
    < "$script_dir/patches/inkplate-11.1.4-partial-timing.patch"
mkdir -p "$output_dir"

cpp_flags=""
if [[ "$mode" == "all" ]]; then
    cpp_flags="-DINKPLATE_REFRESH_PROBE_TIMING=1"
elif [[ "$mode" == "partial-1bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_PARTIAL_1BIT_ONLY=1"
elif [[ "$mode" == "profile-partial-1bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_PROFILE_PARTIAL_1BIT_ONLY=1 -DINKPLATE_REFRESH_PROBE_TIMING=1"
elif [[ "$mode" == "full-1bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_FULL_1BIT_ONLY=1"
elif [[ "$mode" == "full-3bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_FULL_3BIT_ONLY=1"
elif [[ "$mode" == "profile-full-1bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_PROFILE_FULL_1BIT_ONLY=1 -DINKPLATE_REFRESH_PROBE_FULL_TIMING=1"
elif [[ "$mode" == "profile-full-3bit" ]]; then
    cpp_flags+=" -DREFRESH_PROBE_PROFILE_FULL_3BIT_ONLY=1 -DINKPLATE_REFRESH_PROBE_FULL_TIMING=1"
fi

arduino-cli compile \
    --fqbn Inkplate_Boards:esp32:Inkplate4TEMPERA \
    --libraries "$temp_libraries" \
    --build-property "compiler.cpp.extra_flags=$cpp_flags" \
    --output-dir "$output_dir" \
    "$script_dir"

sha256sum "$output_dir/reference.ino.bin" "$output_dir/reference.ino.elf"
echo "artifact_dir=$output_dir"
