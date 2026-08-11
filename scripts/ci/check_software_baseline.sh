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
        '  host-tests        all host regression tests, including scene tools and packages/sdcard' \
        '  host-lint         strict host-tool and packages/sdcard Clippy' \
        '  host              host-tests plus host-lint' \
        '  firmware-builds   locked default, BLE candidate, minimal, slim, telemetry, and all-feature builds' \
        '  firmware-clippy   strict minimal and all-feature firmware Clippy' \
        '  firmware          firmware-builds plus firmware-clippy' \
        '  static-source     stack, FAT, and panel ownership guards' \
        '  static-firmware   release-ELF waveform, linker, and IRAM guards' \
        '  static            all source and release-ELF static guards' \
        '  quality           LOC advisories, rust-analyzer, and blocking code-analysis ratchet' \
        '  all               every lane above (default)'
}

run() {
    echo
    echo "baseline: $*"
    "$@"
}

host_target() {
    rustup run "$host_toolchain" rustc -vV | awk '/^host:/ {print $2}'
}

run_source() {
    cd "$repo_root"
    run rustup run "$host_toolchain" cargo fmt --all -- --check
    echo
    echo "baseline: locked Cargo metadata"
    rustup run "$host_toolchain" cargo metadata --locked --no-deps --format-version 1 >/dev/null
    run git diff --check
    run "$repo_root/scripts/ci/check_secrets.sh"
    run "$repo_root/scripts/ci/check_ble_controller_patch.sh"
}

run_host_tests() {
    cd "$repo_root"
    run "$repo_root/scripts/tests/host/test_code_analysis_guard.sh"
    run "$repo_root/scripts/tests/host/test_include_usage.sh"
    run "$repo_root/scripts/tests/host/test_orphan_modules.sh"
    run "$repo_root/scripts/tests/host/test_event_config_host.sh"
    run "$repo_root/scripts/tests/host/test_event_engine_host.sh"
    run "$repo_root/scripts/tests/host/test_app_state_store_host.sh"
    run "$repo_root/scripts/tests/host/test_ui_shell_host.sh"
    run "$repo_root/scripts/tests/host/test_touch_core_host.sh"
    run "$repo_root/scripts/tests/host/test_touch_replay_host.sh"
    run "$repo_root/scripts/tests/host/test_hostctl_host.sh"

    local target
    target="$(host_target)"
    if [[ -z "$target" ]]; then
        echo "baseline: could not determine host target triple" >&2
        exit 2
    fi
    (
        local host_test_workdir
        host_test_workdir="$(mktemp -d)"
        trap 'rm -rf "$host_test_workdir"' EXIT
        cd "$host_test_workdir"
        run rustup run "$host_toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/tools/scene_maker/Cargo.toml" \
            --target "$target"
        run rustup run "$host_toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/tools/scene_viewer/Cargo.toml" \
            --target "$target"
        run rustup run "$host_toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/packages/sdcard/Cargo.toml" \
            --features host-tests \
            --target "$target"
    )
}

run_host_lint() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/lint_host_tools.sh"

    local target
    target="$(host_target)"
    if [[ -z "$target" ]]; then
        echo "baseline: could not determine host target triple" >&2
        exit 2
    fi
    (
        local host_lint_workdir
        host_lint_workdir="$(mktemp -d)"
        trap 'rm -rf "$host_lint_workdir"' EXIT
        cd "$host_lint_workdir"
        run rustup run "$host_toolchain" cargo clippy \
            --locked \
            --manifest-path "$repo_root/packages/sdcard/Cargo.toml" \
            --features host-tests \
            --target "$target" \
            --all-targets \
            -- \
            -D warnings
    )
}

run_firmware_builds() {
    cd "$repo_root"
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" release default
    run env -u CARGO_NO_DEFAULT_FEATURES CARGO_FEATURES=ble-foundation \
        CARGO_LOCKED=1 "$build_script" ble-release default
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" debug minimal
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" debug slim
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" debug telemetry
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" debug all-features
}

run_firmware_clippy() {
    cd "$repo_root"
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" clippy minimal
    run env -u CARGO_FEATURES -u CARGO_NO_DEFAULT_FEATURES \
        CARGO_LOCKED=1 "$build_script" clippy all-features
}

run_static_source() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_stack_risk.sh"
    run "$repo_root/scripts/tests/host/test_check_stack_risk.sh"
    run "$repo_root/scripts/ci/check_fat_engine_stackless.sh"
    run "$repo_root/scripts/ci/check_panel_bus_gating.sh"
    run "$repo_root/scripts/ci/check_ui_shell_ownership.sh"
}

run_static_firmware() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_panel_waveform_placement.sh"
    run "$repo_root/scripts/ci/check_pinned_linker_scripts.sh"
    run "$repo_root/scripts/ci/check_iram_flash_refs.sh"
    run "$repo_root/scripts/ci/check_ble_image_budget.sh"
}

run_static() {
    run_static_source
    run_static_firmware
}

run_quality() {
    cd "$repo_root"
    run "$repo_root/scripts/ci/check_rust_loc.sh"
    run "$repo_root/scripts/ci/check_markdown_loc.sh"
    run env INCLUDE_USAGE_ENFORCE=1 "$repo_root/scripts/ci/check_include_usage.sh"
    run "$repo_root/scripts/ci/check_orphan_modules.sh"
    run "$repo_root/scripts/ci/lint_rust_analyzer.sh"
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
