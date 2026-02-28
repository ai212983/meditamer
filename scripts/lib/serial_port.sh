#!/usr/bin/env bash

# Shared serial-port autodetection for hardware scripts.
# Rules:
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
