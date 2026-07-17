#!/usr/bin/env bash

set -euo pipefail

_esp_idf_env_repo_root() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    cd "$script_dir/../.." && pwd
}

esp_idf_resolve_root() {
    local repo_root latest candidate
    if [[ -n "${IDF_APP_ROOT:-}" ]]; then
        printf '%s\n' "$IDF_APP_ROOT"
        return 0
    fi

    repo_root="$(_esp_idf_env_repo_root)"
    latest=""
    shopt -s nullglob
    for candidate in "$repo_root"/.embuild/espressif/esp-idf/v*; do
        [[ -d "$candidate" ]] || continue
        latest="$candidate"
    done
    shopt -u nullglob

    [[ -n "$latest" ]] || return 1
    printf '%s\n' "$latest"
}

esp_idf_require_explicit_root() {
    if [[ -n "${IDF_APP_ROOT:-}" ]]; then
        printf '%s\n' "$IDF_APP_ROOT"
        return 0
    fi
    return 1
}

esp_idf_source_env() {
    local caller="$1"
    local mode="${2:-auto}"
    local idf_root=""

    case "$mode" in
        auto)
            idf_root="$(esp_idf_resolve_root)" || {
                echo "${caller}: no local ESP-IDF install found and IDF_APP_ROOT is not set." >&2
                return 1
            }
            ;;
        explicit)
            idf_root="$(esp_idf_require_explicit_root)" || {
                echo "${caller}: set IDF_APP_ROOT explicitly for the external ESP-IDF install." >&2
                return 1
            }
            ;;
        *)
            echo "${caller}: unsupported ESP-IDF resolution mode: ${mode}" >&2
            return 1
            ;;
    esac

    if [[ ! -f "$idf_root/export.sh" ]]; then
        echo "${caller}: ESP-IDF export.sh not found at $idf_root/export.sh" >&2
        return 1
    fi

    # shellcheck disable=SC1090
    source "$idf_root/export.sh" >/dev/null
    export ESP_IDF_ROOT_RESOLVED="$idf_root"
    export ESP_IDF_PYTHON_BIN="$(command -v python)"
    export ESP_IDF_ESPTOOL_BIN="$(command -v esptool.py || true)"
    export ESP_IDF_IDF_PY_BIN="$(command -v idf.py || true)"

    if [[ -z "${ESP_IDF_PYTHON_BIN:-}" ]]; then
        echo "${caller}: python unavailable after sourcing $idf_root/export.sh" >&2
        return 1
    fi

    echo "${caller}: using ESP-IDF root: $idf_root" >&2
}
