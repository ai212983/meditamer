#!/usr/bin/env bash

set -euo pipefail

_RUN_HOSTCTL_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./serial_port.sh
source "$_RUN_HOSTCTL_LIB_DIR/serial_port.sh"

_run_hostctl_repo_root() {
    cd "$_RUN_HOSTCTL_LIB_DIR/../.." && pwd
}

_run_hostctl_abs_path() {
    local repo_root="$1"
    local raw_path="$2"
    if [[ "$raw_path" == /* ]]; then
        printf '%s\n' "$raw_path"
    else
        printf '%s/%s\n' "$repo_root" "${raw_path#./}"
    fi
}

_run_hostctl_normalize_path_env_vars() {
    local repo_root name raw_path abs_path
    repo_root="$(_run_hostctl_repo_root)"
    for name in "$@"; do
        raw_path="${!name:-}"
        if [[ -z "$raw_path" ]]; then
            continue
        fi
        abs_path="$(_run_hostctl_abs_path "$repo_root" "$raw_path")"
        printf -v "$name" '%s' "$abs_path"
        export "$name"
    done
}

_run_hostctl_port_cache_path() {
    local repo_root raw_path
    repo_root="$(_run_hostctl_repo_root)"
    raw_path="${HOSTCTL_SERIAL_PORT_CACHE_PATH:-logs/.state/hostctl_last_usbserial_port}"
    _run_hostctl_abs_path "$repo_root" "$raw_path"
}

_run_hostctl_read_cached_port() {
    local cache_path cached
    cache_path="$(_run_hostctl_port_cache_path)"
    [[ -f "$cache_path" ]] || return 1
    cached="$(head -n1 "$cache_path" 2>/dev/null | tr -d '\r\n' || true)"
    [[ -n "$cached" ]] || return 1
    [[ -e "$cached" ]] || return 1
    printf '%s\n' "$cached"
}

_run_hostctl_write_cached_port() {
    local port="$1"
    local cache_path
    [[ -n "$port" ]] || return 0
    [[ -e "$port" ]] || return 0
    cache_path="$(_run_hostctl_port_cache_path)"
    mkdir -p "$(dirname "$cache_path")"
    printf '%s\n' "$port" >"$cache_path"
}

resolve_hostctl_serial_port() {
    local explicit_var="${1:-HOSTCTL_NET_PORT}"
    local caller="${2:-script}"
    local explicit cached detected

    explicit="${!explicit_var:-}"
    if [[ -n "$explicit" ]]; then
        if [[ -e "$explicit" ]]; then
            printf '%s\n' "$explicit"
            return 0
        fi
        echo "${caller}: ${explicit_var} is set but not present: ${explicit}" >&2
    fi

    if cached="$(_run_hostctl_read_cached_port)"; then
        echo "${caller}: using cached serial port: ${cached}" >&2
        printf '%s\n' "$cached"
        return 0
    fi

    if detected="$(detect_serial_port)"; then
        echo "${caller}: using detected serial port: ${detected}" >&2
        printf '%s\n' "$detected"
        return 0
    fi

    echo "${caller}: could not resolve serial port from ${explicit_var}, cache, or autodetect." >&2
    return 1
}

ensure_hostctl_net_port() {
    local caller="${1:-script}"
    if [[ -n "${HOSTCTL_NET_PORT:-}" && -e "${HOSTCTL_NET_PORT}" ]]; then
        return 0
    fi
    local resolved
    resolved="$(resolve_hostctl_serial_port HOSTCTL_NET_PORT "$caller")" || return 1
    export HOSTCTL_NET_PORT="$resolved"
}

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
    local script_dir repo_root manifest_path toolchain host_target host_target_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    repo_root="$(cd "$script_dir/../.." && pwd)"
    manifest_path="$repo_root/tools/hostctl/Cargo.toml"
    toolchain="${HOSTCTL_HOST_RUSTUP_TOOLCHAIN:-stable}"

    # hostctl is intentionally launched from /tmp to avoid workspace-target
    # bleed, so force repo-relative env paths to absolute first.
    _run_hostctl_normalize_path_env_vars \
        HOSTCTL_LOG_JSON_PATH \
        HOSTCTL_FLASH_CAPTURE_LOG_PATH \
        HOSTCTL_FIRMWARE_UPDATE_LOG_PATH \
        HOSTCTL_REPAINT_LOG_PATH \
        HOSTCTL_NET_LOG_PATH \
        HOSTCTL_NET_POLICY_PATH \
        HOSTCTL_NET_DISCOVERY_PROFILE_PATH \
        HOSTCTL_NET_LOCK_PATH \
        HOSTCTL_UPLOAD_SEND_DIAG_PATH

    host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
    if [[ -z "$host_target" ]]; then
        echo "could not determine host target triple" >&2
        return 1
    fi
    host_target_dir="$repo_root/target/host-tools/hostctl/$host_target"

    if (
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
            rustup run "$toolchain" cargo run \
            --locked \
            --target "$host_target" \
            --manifest-path "$manifest_path" \
            -- "$@"
    ); then
        if [[ -n "${HOSTCTL_NET_PORT:-}" ]]; then
            _run_hostctl_write_cached_port "$HOSTCTL_NET_PORT"
        elif [[ -n "${HOSTCTL_PORT:-}" ]]; then
            _run_hostctl_write_cached_port "$HOSTCTL_PORT"
        fi
        return 0
    fi
    return 1
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
