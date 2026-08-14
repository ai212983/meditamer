#!/usr/bin/env bash

# Shared serial-port autodetection for hardware scripts, plus the
# hostctl-specific port/env resolution helpers used by the guarded Wi-Fi
# acceptance/discovery/regression scripts. The hostctl launch capability lives
# in scripts/hostctl.sh.
#
# Autodetection rules:
# - Respect explicitly provided ESPFLASH_PORT.
# - Prefer a single /dev/cu.* port on macOS.
# - Fall back to a single Linux USB serial port (/dev/ttyUSB* or /dev/ttyACM*).
# - If ambiguous, require explicit ESPFLASH_PORT.

_SERIAL_PORT_CANDIDATES=()

_serial_port_reset_candidates() {
    _SERIAL_PORT_CANDIDATES=()
}

_serial_port_append_unique() {
    local candidate="$1"
    local existing
    for existing in ${_SERIAL_PORT_CANDIDATES[@]+"${_SERIAL_PORT_CANDIDATES[@]}"}; do
        if [[ "$existing" == "$candidate" ]]; then
            return 0
        fi
    done
    _SERIAL_PORT_CANDIDATES+=("$candidate")
}

_serial_port_add_glob_matches() {
    local pattern="$1"
    local entry
    shopt -s nullglob
    for entry in $pattern; do
        [[ -e "$entry" ]] || continue
        _serial_port_append_unique "$entry"
    done
    shopt -u nullglob
}

_serial_port_collect_candidates() {
    _serial_port_reset_candidates

    _serial_port_add_glob_matches "/dev/cu.usbserial*"
    _serial_port_add_glob_matches "/dev/cu.usbmodem*"
    _serial_port_add_glob_matches "/dev/cu.SLAB_USBtoUART*"
    _serial_port_add_glob_matches "/dev/cu.wchusbserial*"

    _serial_port_add_glob_matches "/dev/tty.usbserial*"
    _serial_port_add_glob_matches "/dev/tty.usbmodem*"
    _serial_port_add_glob_matches "/dev/tty.SLAB_USBtoUART*"
    _serial_port_add_glob_matches "/dev/tty.wchusbserial*"

    _serial_port_add_glob_matches "/dev/ttyUSB*"
    _serial_port_add_glob_matches "/dev/ttyACM*"
}

serial_port_candidates() {
    _serial_port_collect_candidates
    local candidate
    for candidate in ${_SERIAL_PORT_CANDIDATES[@]+"${_SERIAL_PORT_CANDIDATES[@]}"}; do
        printf '%s\n' "$candidate"
    done
}

detect_serial_port() {
    _serial_port_collect_candidates

    local hint="${ESPFLASH_PORT_HINT:-}"
    local -a candidates=()
    local candidate

    if [[ -n "$hint" ]]; then
        for candidate in ${_SERIAL_PORT_CANDIDATES[@]+"${_SERIAL_PORT_CANDIDATES[@]}"}; do
            if [[ "$candidate" == *"$hint"* ]]; then
                candidates+=("$candidate")
            fi
        done
    else
        for candidate in ${_SERIAL_PORT_CANDIDATES[@]+"${_SERIAL_PORT_CANDIDATES[@]}"}; do
            candidates+=("$candidate")
        done
    fi

    local -a cu_ports=()
    local -a linux_ports=()
    local -a tty_ports=()
    for candidate in "${candidates[@]}"; do
        case "$candidate" in
            /dev/cu.*) cu_ports+=("$candidate") ;;
            /dev/ttyUSB* | /dev/ttyACM*) linux_ports+=("$candidate") ;;
            /dev/tty.*) tty_ports+=("$candidate") ;;
        esac
    done

    if [[ ${#cu_ports[@]} -eq 1 ]]; then
        printf '%s\n' "${cu_ports[0]}"
        return 0
    fi
    if [[ ${#linux_ports[@]} -eq 1 ]]; then
        printf '%s\n' "${linux_ports[0]}"
        return 0
    fi
    if [[ ${#tty_ports[@]} -eq 1 ]]; then
        printf '%s\n' "${tty_ports[0]}"
        return 0
    fi
    if [[ ${#candidates[@]} -eq 1 ]]; then
        printf '%s\n' "${candidates[0]}"
        return 0
    fi

    return 1
}

ensure_espflash_port() {
    local caller="${1:-script}"

    if [[ -n "${ESPFLASH_PORT:-}" ]]; then
        return 0
    fi

    local detected_port=""
    if detected_port="$(detect_serial_port)"; then
        export ESPFLASH_PORT="$detected_port"
        echo "${caller}: using detected serial port: $ESPFLASH_PORT" >&2
        return 0
    fi

    echo "${caller}: ESPFLASH_PORT is not set and autodetection was not conclusive." >&2
    echo "${caller}: set ESPFLASH_PORT explicitly (example: /dev/cu.usbserial-540)." >&2
    local listed_any=0
    local candidate
    while IFS= read -r candidate; do
        [[ -z "$candidate" ]] && continue
        if [[ "$listed_any" -eq 0 ]]; then
            echo "${caller}: detected serial candidates:" >&2
            listed_any=1
        fi
        echo "  - $candidate" >&2
    done < <(serial_port_candidates)
    return 1
}

# ---- hostctl port cache + env helpers --------------------------------------

_run_hostctl_repo_root() {
    cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd
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
