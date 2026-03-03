#!/usr/bin/env bash

set -euo pipefail

load_repo_env_file_if_present() {
    local relative_path="${1:-.env.local}"
    local script_dir repo_root env_path
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    repo_root="$(cd "$script_dir/../.." && pwd)"
    env_path="$repo_root/$relative_path"

    if [[ -f "$env_path" ]]; then
        # Preserve explicitly provided environment values so callers can override
        # defaults from .env.local (for example per-stage log paths in gate scripts).
        local names values name idx
        names=()
        values=()
        while IFS= read -r name; do
            if [[ -n "${!name+x}" ]]; then
                names+=("$name")
                values+=("${!name}")
            fi
        done < <(sed -nE 's/^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)=.*/\1/p' "$env_path")

        # shellcheck source=/dev/null
        set -a
        source "$env_path"
        set +a

        for idx in "${!names[@]}"; do
            export "${names[$idx]}=${values[$idx]}"
        done
    fi
}

run_hostctl() {
    local script_dir repo_root manifest_path toolchain host_target
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    repo_root="$(cd "$script_dir/../.." && pwd)"
    manifest_path="$repo_root/tools/hostctl/Cargo.toml"
    toolchain="${RUSTUP_TOOLCHAIN:-stable}"

    host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
    if [[ -z "$host_target" ]]; then
        echo "could not determine host target triple" >&2
        return 1
    fi

    (
        cd /tmp
        RUSTUP_TOOLCHAIN="$toolchain" cargo run \
            --locked \
            --target "$host_target" \
            --manifest-path "$manifest_path" \
            -- "$@"
    )
}

reject_legacy_env_vars() {
    local prefix="$1"
    shift
    local found=0
    local name
    for name in "$@"; do
        if [[ -n "${!name:-}" ]]; then
            if [[ "$found" -eq 0 ]]; then
                echo "$prefix: legacy environment variables are no longer supported. Use HOSTCTL_* names." >&2
                found=1
            fi
            echo "  - $name is set" >&2
        fi
    done
    if [[ "$found" -eq 1 ]]; then
        return 1
    fi
}
