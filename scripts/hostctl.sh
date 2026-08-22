#!/usr/bin/env bash

# Native Cargo launch preparation for `hostctl` -- the direct, advanced
# explicit-mode path (scripts/device/flash.sh remains the canonical
# flash-and-capture wrapper; docs continue to point operators there).
#
# Scope is intentionally narrow: process/env hygiene for the cargo run
# itself, host target + toolchain resolution, a dedicated Cargo target dir,
# and an opt-in HOSTCTL_PORT cache. It does not source .env.local or require
# a port unless the invoked hostctl command does. The Rust CLI resets its
# runtime working directory to the repository root, so typed arguments and
# default evidence paths remain repository-relative despite the isolated
# /tmp Cargo launch below.
#
# usage: scripts/hostctl.sh <hostctl-subcommand> [args...]
#
# Port order for commands that need one: explicit --port (resolved by
# hostctl itself) beats the command-specific environment variable
# (HOSTCTL_PORT, or HOSTCTL_NET_PORT for wifi/net commands -- callers set
# that themselves before invoking this launcher), which beats a valid cache
# entry (filled in below only when neither is already set), which beats
# hostctl's own unambiguous autodetect. A missing explicit port fails
# loudly; the cache path is always resolved against the repo root, never
# the /tmp working directory this script launches cargo from.

set -euo pipefail

if [[ "$#" -eq 0 ]]; then
    echo "usage: scripts/hostctl.sh <hostctl-subcommand> [args...]" >&2
    exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
manifest_path="$repo_root/tools/hostctl/Cargo.toml"
toolchain="${HOSTCTL_HOST_RUSTUP_TOOLCHAIN:-stable}"

_hostctl_abs_path() {
    local raw_path="$1"
    if [[ "$raw_path" == /* ]]; then
        printf '%s\n' "$raw_path"
    else
        printf '%s/%s\n' "$repo_root" "${raw_path#./}"
    fi
}

_hostctl_port_cache_path() {
    # Internal plumbing: cache path is fixed, not env-overridable
    # (hostctl-env-audit.md cat 3).
    _hostctl_abs_path "logs/.state/hostctl_last_usbserial_port"
}

_hostctl_read_cached_port() {
    local cache_path cached
    cache_path="$(_hostctl_port_cache_path)"
    [[ -f "$cache_path" ]] || return 1
    cached="$(head -n1 "$cache_path" 2>/dev/null | tr -d '\r\n' || true)"
    [[ -n "$cached" ]] || return 1
    [[ -e "$cached" ]] || return 1
    printf '%s\n' "$cached"
}

_hostctl_write_cached_port() {
    local port="$1"
    [[ -n "$port" && -e "$port" ]] || return 0
    local cache_path
    cache_path="$(_hostctl_port_cache_path)"
    mkdir -p "$(dirname "$cache_path")"
    printf '%s\n' "$port" >"$cache_path"
}

_hostctl_reject_legacy_env_vars() {
    local prefix="$1"
    shift
    local found=0 name
    for name in "$@"; do
        [[ -n "${!name:-}" ]] || continue
        if [[ "$found" -eq 0 ]]; then
            echo "$prefix: legacy environment variables are no longer supported; use HOSTCTL_* names" >&2
        fi
        echo "  - $name is set" >&2
        found=1
    done
    [[ "$found" -eq 0 ]]
}

if [[ "$1" == "upload" ]]; then
    _hostctl_reject_legacy_env_vars "hostctl upload" \
        UPLOAD_TOKEN \
        UPLOAD_CHUNK_SIZE \
        UPLOAD_SD_BUSY_TOTAL_RETRY_SEC \
        UPLOAD_NET_RECOVERY_TIMEOUT_SEC \
        UPLOAD_NET_RECOVERY_POLL_SEC \
        UPLOAD_CONNECT_TIMEOUT_SEC \
        UPLOAD_SKIP_MKDIR \
        UPLOAD_TRACE_REQUESTS
fi

if [[ -z "${HOSTCTL_PORT:-}" && -z "${HOSTCTL_NET_PORT:-}" ]]; then
    if cached_port="$(_hostctl_read_cached_port)"; then
        export HOSTCTL_PORT="$cached_port"
    fi
fi

# hostctl runs from /tmp (below) to avoid workspace-target bleed, so
# repo-relative path env vars must be made absolute first.
for name in \
    HOSTCTL_LOG_JSON_PATH \
    HOSTCTL_FLASH_CAPTURE_LOG_PATH \
    HOSTCTL_NET_LOG_PATH \
    HOSTCTL_UPLOAD_SEND_DIAG_PATH; do
    raw_path="${!name:-}"
    [[ -n "$raw_path" ]] || continue
    abs_path="$(_hostctl_abs_path "$raw_path")"
    printf -v "$name" '%s' "$abs_path"
    export "${name?}"
done

host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
if [[ -z "$host_target" ]]; then
    echo "hostctl.sh: could not determine host target triple" >&2
    exit 1
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
        _hostctl_write_cached_port "$HOSTCTL_NET_PORT"
    elif [[ -n "${HOSTCTL_PORT:-}" ]]; then
        _hostctl_write_cached_port "$HOSTCTL_PORT"
    fi
    exit 0
fi
exit 1
