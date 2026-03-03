#!/usr/bin/env bash

set -euo pipefail

if [[ -f "$HOME/export-esp.sh" ]]; then
    # Ensure Xtensa toolchain is available for linking.
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

run_cargo_build() {
    local mode="$1"
    local cmd=(cargo build)

    if [[ "$mode" == "release" ]]; then
        cmd+=(--release)
    fi
    if [[ "${CARGO_NO_DEFAULT_FEATURES:-0}" == "1" ]]; then
        cmd+=(--no-default-features)
    fi
    if [[ -n "${CARGO_FEATURES:-}" ]]; then
        cmd+=(--features "$CARGO_FEATURES")
    fi

    "${cmd[@]}"
}

case "${1:-}" in
"" | "release")
    run_cargo_build "release"
    ;;
"debug")
    run_cargo_build "debug"
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac
