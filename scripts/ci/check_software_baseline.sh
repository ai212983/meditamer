#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
build_script="$repo_root/scripts/build/build.sh"
host_toolchain="${RUSTUP_TOOLCHAIN:-stable}"

usage() {
    printf '%s\n' \
        'usage: scripts/ci/check_software_baseline.sh [lane]' \
        '' \
        'lanes:' \
        '  source            formatting, lock metadata, diff, and secret checks' \
        '  host-tests        all registered host regression tests' \
        '  host-lint         strict host-tool and platform/storage/sdcard Clippy' \
        '  host              host-tests plus host-lint' \
        '  firmware-builds   locked default/minimal builds and selected diagnostic artifacts' \
        '  firmware-clippy   strict default/minimal and selected diagnostic firmware Clippy' \
        '  firmware          firmware-builds plus firmware-clippy' \
        '  static-source     stack, FAT, and panel ownership guards' \
        '  static-firmware   release-ELF placement, linker, and IRAM guards' \
        '  static            all source and release-ELF static guards' \
        '  quality           Markdown advisory, source reachability, and code-analysis ratchets' \
        '  all               every lane above (default)'
}

run() {
    echo
    echo "baseline: $*"
    "$@"
}

run_source() {
    cd "$repo_root"
    run rustup run "$host_toolchain" cargo fmt --all -- --check
    echo
    echo "baseline: locked Cargo metadata"
    rustup run "$host_toolchain" cargo metadata --locked --no-deps --format-version 1 >/dev/null
    run git diff --check
    run "$repo_root/scripts/ci/check_repository_assets.py"
    run "$repo_root/scripts/ci/check_secrets.sh"
    run "$repo_root/scripts/ci/check_ble_controller_patch.sh"
    run "$repo_root/scripts/ci/check_network_owner_source.sh"
}

run_host_tests() {
    cd "$repo_root"
    run "$repo_root/scripts/host-test.sh" test all
}

run_host_lint() {
    cd "$repo_root"
    run "$repo_root/scripts/host-test.sh" lint all
}

# The same explicit composition matrix is built and linted. Diagnostic boot
# fixtures stay out of the ordinary default image; standalone binaries get
# only their required feature set.
run_inkplate_matrix() {
    local command="$1"
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES -u FIRMWARE_BIN \
        CARGO_LOCKED=1 "$command" debug minimal
    run env -u CARGO_NO_DEFAULT_FEATURES -u FIRMWARE_BIN \
        CARGO_FEATURES=telemetry-defmt,ui-interaction-trace,ui-provider-fixture,ui-initialization-fixture,sd-runner-allocation-fixture \
        CARGO_LOCKED=1 "$command" debug default
    run env -u CARGO_NO_DEFAULT_FEATURES -u FIRMWARE_BIN \
        CARGO_FEATURES=wifi-debug-slim-app CARGO_LOCKED=1 "$command" debug minimal
    run env -u CARGO_NO_DEFAULT_FEATURES -u FIRMWARE_BIN \
        CARGO_FEATURES=panel-waveform-fixture CARGO_LOCKED=1 "$command" debug minimal
    run env -u CARGO_NO_DEFAULT_FEATURES FIRMWARE_BIN=ble-shared-runtime-probe \
        CARGO_FEATURES=shared-ble-runtime CARGO_LOCKED=1 "$command" release minimal
    run env -u CARGO_NO_DEFAULT_FEATURES FIRMWARE_BIN=updater \
        CARGO_FEATURES=factory-updater CARGO_LOCKED=1 "$command" release minimal
    # Default comes last so static gates always inspect the production ELF.
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES -u FIRMWARE_BIN \
        CARGO_LOCKED=1 "$command" release default
}

run_firmware_builds() {
    cd "$repo_root"
    run_inkplate_matrix "$build_script"
    run "$repo_root/targets/medinote-waveshare/build.sh" --locked --bin medinote-waveshare --features crash-screen
}

run_firmware_clippy() {
    cd "$repo_root"
    run_inkplate_matrix "$repo_root/scripts/ci/clippy-firmware.sh"
}

run_static_source() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_stack_risk.sh"
    run "$repo_root/scripts/tests/host/test_check_stack_risk.sh"
    run python3 "$repo_root/scripts/tests/host/test_check_repository_assets.py"
    run "$repo_root/scripts/tests/host/test_check_bt_dram_reservation.sh"
    run "$repo_root/scripts/ci/check_fat_engine_stackless.sh"
    run "$repo_root/scripts/ci/check_panel_bus_gating.sh"
    run "$repo_root/scripts/ci/check_ui_shell_ownership.sh"
}

run_static_firmware() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_panel_waveform_placement.sh"
    run "$repo_root/scripts/ci/check_pinned_linker_scripts.sh"
    run "$repo_root/scripts/ci/check_bt_dram_reservation.sh" \
        "$repo_root/target/xtensa-esp32-none-elf/release/meditamer"
    run "$repo_root/scripts/ci/check_iram_flash_refs.sh"
    run "$repo_root/scripts/ci/check_ble_image_budget.sh"
    run "$repo_root/scripts/ci/check_stack_risk.sh" --inkplate-elf \
        "$repo_root/target/xtensa-esp32-none-elf/release/meditamer"
    run "$repo_root/scripts/ci/check_stack_risk.sh" --medinote-elf \
        "$repo_root/targets/medinote-waveshare/target/xtensa-esp32s3-none-elf/release/medinote-waveshare"
}

run_static() {
    run_static_source
    run_static_firmware
}

run_quality() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_markdown_loc.sh"
    run env INCLUDE_USAGE_ENFORCE=1 "$repo_root/scripts/ci/check_include_usage.sh"
    run "$repo_root/scripts/ci/check_orphan_modules.py"
    run env RCA_ENFORCE=1 RCA_RATCHET=1 "$repo_root/scripts/ci/lint_code_analysis.sh"
}

lane="${1:-all}"
if [[ "$#" -gt 1 ]]; then
    usage >&2
    exit 2
fi

case "$lane" in
"source")
    run_source
    ;;
"host-tests")
    run_host_tests
    ;;
"host-lint")
    run_host_lint
    ;;
"host")
    run_host_tests
    run_host_lint
    ;;
"firmware-builds")
    run_firmware_builds
    ;;
"firmware-clippy")
    run_firmware_clippy
    ;;
"firmware")
    run_firmware_builds
    run_firmware_clippy
    ;;
"static-source")
    run_static_source
    ;;
"static-firmware")
    run_static_firmware
    ;;
"static")
    run_static
    ;;
"quality")
    run_quality
    ;;
"all")
    run_source
    run_host_tests
    run_host_lint
    run_firmware_builds
    run_firmware_clippy
    run_static
    run_quality
    ;;
"-h" | "--help")
    usage
    ;;
*)
    usage >&2
    exit 2
    ;;
esac
