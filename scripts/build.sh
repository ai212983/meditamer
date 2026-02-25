#!/usr/bin/env bash

set -euo pipefail

if [[ -f "$HOME/export-esp.sh" ]]; then
    # Ensure Xtensa toolchain is available for linking.
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

feature_args=()
if [[ -n "${CARGO_FEATURES:-}" ]]; then
    feature_args+=(--features "$CARGO_FEATURES")
fi

case "$1" in
"" | "release")
    if [[ ${#feature_args[@]} -gt 0 ]]; then
        cargo build --release "${feature_args[@]}"
    else
        cargo build --release
    fi
    ;;
"debug")
    if [[ ${#feature_args[@]} -gt 0 ]]; then
        cargo build "${feature_args[@]}"
    else
        cargo build
    fi
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac
