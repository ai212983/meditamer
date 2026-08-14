#!/usr/bin/env bash

# Authoritative dispatcher for host test/lint suites. `scripts/host-suites.tsv`
# is the sole membership registry; `scripts/ci/check_software_baseline.sh` and
# `scripts/ci/coverage_host.sh` consume it directly rather than hardcoding
# suite lists of their own. Each suite's invocation mechanics below are a
# direct port of the wrapper it replaces (see
# docs/archive/host-tooling/scripts-tools-surface-cleanup-ledger.md change set C-201/C-202).

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
registry="$script_dir/host-suites.tsv"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"

usage() {
    printf '%s\n' \
        'usage: scripts/host-test.sh --list' \
        '       scripts/host-test.sh <test|lint> <suite|all> [<host-target>] [-- <args>...]' \
        '' \
        'suites are defined in scripts/host-suites.tsv (name/test/lint/coverage/mode/manifest).'
}

host_target() {
    rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}'
}

# column($1=name, $2=column) -> tab-separated field from the registry.
suite_field() {
    local name="$1" column="$2"
    awk -F'\t' -v name="$name" -v col="$column" \
        'NR>1 && $1==name { print $col; found=1 } END { if (!found) exit 1 }' \
        "$registry"
}

suite_exists() {
    awk -F'\t' -v name="$1" 'NR>1 && $1==name { found=1 } END { exit !found }' "$registry"
}

suites_for_lane() {
    local lane_col="$1"
    awk -F'\t' -v col="$lane_col" 'NR>1 && $col=="yes" { print $1 }' "$registry"
}

list_suites() {
    awk -F'\t' \
        'NR==1 { next }
         { printf "%-20s test=%-4s lint=%-4s coverage=%-4s %s\n", $1, $2, $3, $4, $7 }' \
        "$registry"
}

# ---- per-suite test invocations -------------------------------------------

run_test_self() {
    local script="$1"
    "$repo_root/scripts/tests/host/$script"
}

run_test_crate() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        RUSTUP_TOOLCHAIN="$toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            "$@"
    )
}

run_test_crate_lvgl() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        DEP_LV_CONFIG_PATH="$repo_root/config/lvgl" \
            RUSTUP_TOOLCHAIN="$toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            "$@"
    )
}

run_test_crate_ephemeral() {
    local manifest="$1" target="$2"
    shift 2
    (
        target_dir="$(mktemp -d)"
        trap 'rm -rf "$target_dir"' EXIT
        cd /tmp
        CARGO_TARGET_DIR="$target_dir" RUSTUP_TOOLCHAIN="$toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            "$@"
    )
}

run_test_crate_hostctl() {
    local manifest="$1" target="$2"
    shift 2
    local hostctl_toolchain="${HOSTCTL_HOST_RUSTUP_TOOLCHAIN:-stable}"
    local host_target_dir="$repo_root/target/host-tools/hostctl-tests/$target"
    (
        cd /tmp
        env \
            -u RUSTUP_TOOLCHAIN \
            -u CARGO_BUILD_TARGET \
            -u CARGO_TARGET_DIR \
            -u CARGO_ENCODED_RUSTFLAGS \
            -u CARGO_UNSTABLE_BUILD_STD \
            -u RUSTFLAGS \
            -u RUSTDOCFLAGS \
            -u RUSTC_WRAPPER \
            -u RUSTC_WORKSPACE_WRAPPER \
            CARGO_TARGET_DIR="$host_target_dir" \
            rustup run "$hostctl_toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            "$@"
    )
}

run_test_fixture_script() {
    "$repo_root/tools/touch_replay/run_fixtures.sh" "$@"
}

run_test_crate_workdir() {
    local manifest="$1" target="$2"
    shift 2
    (
        workdir="$(mktemp -d)"
        trap 'rm -rf "$workdir"' EXIT
        cd "$workdir"
        RUSTUP_TOOLCHAIN="$toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            "$@"
    )
}

run_test_crate_feature() {
    local manifest="$1" target="$2"
    shift 2
    (
        workdir="$(mktemp -d)"
        trap 'rm -rf "$workdir"' EXIT
        cd "$workdir"
        RUSTUP_TOOLCHAIN="$toolchain" cargo test \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --features host-tests \
            --target "$target" \
            "$@"
    )
}

# ---- per-suite lint invocations --------------------------------------------

run_lint_crate() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        RUSTUP_TOOLCHAIN="$toolchain" cargo clippy \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            --all-targets \
            -- \
            -D warnings \
            "$@"
    )
}

run_lint_crate_lvgl() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        DEP_LV_CONFIG_PATH="$repo_root/config/lvgl" \
            RUSTUP_TOOLCHAIN="$toolchain" cargo clippy \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            --all-targets \
            -- \
            -D warnings \
            "$@"
    )
}

run_lint_crate_hostctl() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        RUSTUP_TOOLCHAIN="$toolchain" cargo clippy \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --target "$target" \
            --all-targets \
            -- \
            -D warnings \
            "$@"
    )
}

run_lint_crate_feature() {
    local manifest="$1" target="$2"
    shift 2
    (
        cd /tmp
        RUSTUP_TOOLCHAIN="$toolchain" cargo clippy \
            --locked \
            --manifest-path "$repo_root/$manifest" \
            --features host-tests \
            --target "$target" \
            --all-targets \
            -- \
            -D warnings \
            "$@"
    )
}

# ---- dispatch ---------------------------------------------------------------

dispatch_test() {
    local name="$1" target="$2"
    shift 2
    local mode manifest
    mode="$(suite_field "$name" 5)"
    manifest="$(suite_field "$name" 6)"
    case "$mode" in
    self-test)
        case "$name" in
        code-analysis-guard) run_test_self "test_code_analysis_guard.sh" ;;
        include-usage) run_test_self "test_include_usage.sh" ;;
        orphan-modules) run_test_self "test_orphan_modules.sh" ;;
        *)
            echo "host-test: no self-test mapping for suite '$name'" >&2
            exit 2
            ;;
        esac
        ;;
    crate) run_test_crate "$manifest" "$target" "$@" ;;
    crate-lvgl) run_test_crate_lvgl "$manifest" "$target" "$@" ;;
    crate-ephemeral) run_test_crate_ephemeral "$manifest" "$target" "$@" ;;
    crate-hostctl) run_test_crate_hostctl "$manifest" "$target" "$@" ;;
    fixture-script) run_test_fixture_script "$@" ;;
    crate-workdir) run_test_crate_workdir "$manifest" "$target" "$@" ;;
    crate-feature) run_test_crate_feature "$manifest" "$target" "$@" ;;
    *)
        echo "host-test: unknown mode '$mode' for suite '$name'" >&2
        exit 2
        ;;
    esac
}

dispatch_lint() {
    local name="$1" target="$2"
    shift 2
    local mode manifest
    mode="$(suite_field "$name" 5)"
    manifest="$(suite_field "$name" 6)"
    case "$mode" in
    crate | crate-workdir) run_lint_crate "$manifest" "$target" "$@" ;;
    crate-lvgl) run_lint_crate_lvgl "$manifest" "$target" "$@" ;;
    crate-hostctl) run_lint_crate_hostctl "$manifest" "$target" "$@" ;;
    fixture-script) run_lint_crate "$manifest" "$target" "$@" ;;
    crate-feature) run_lint_crate_feature "$manifest" "$target" "$@" ;;
    *)
        echo "host-test: suite '$name' has no lint mode (mode='$mode')" >&2
        exit 2
        ;;
    esac
}

run_one() {
    local action="$1" name="$2" target="$3"
    shift 3
    echo
    echo "host-$action: $name"
    if ! suite_exists "$name"; then
        echo "host-test: unknown suite '$name' (see --list)" >&2
        exit 2
    fi
    local membership_column=2
    [[ "$action" == "lint" ]] && membership_column=3
    local membership
    membership="$(suite_field "$name" "$membership_column")"
    if [[ "$membership" != "yes" ]]; then
        echo "host-test: suite '$name' is not registered for '$action' (see --list)" >&2
        exit 2
    fi
    if [[ "$action" == "test" ]]; then
        dispatch_test "$name" "$target" "$@"
    else
        dispatch_lint "$name" "$target" "$@"
    fi
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

if [[ "${1:-}" == "--list" ]]; then
    list_suites
    exit 0
fi

action="${1:-}"
suite="${2:-}"
if [[ "$action" != "test" && "$action" != "lint" ]] || [[ -z "$suite" ]]; then
    usage >&2
    exit 2
fi
shift 2

target=""
if [[ "${1:-}" != "" && "${1:-}" != "--" ]]; then
    target="$1"
    shift
fi
if [[ -z "$target" ]]; then
    target="$(host_target)"
fi
if [[ -z "$target" ]]; then
    echo "host-test: could not determine host target triple" >&2
    exit 2
fi

if [[ "${1:-}" == "--" ]]; then
    shift
fi

if [[ "$suite" == "all" ]]; then
    lane_col=2
    [[ "$action" == "lint" ]] && lane_col=3
    while IFS= read -r name; do
        run_one "$action" "$name" "$target" "$@"
    done < <(suites_for_lane "$lane_col")
else
    run_one "$action" "$suite" "$target" "$@"
fi
