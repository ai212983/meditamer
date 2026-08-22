#!/usr/bin/env bash
# Watches the board's USB-Serial-JTAG console for a "TIME_REQUEST" line and
# replies with "SET_EPOCH <current UTC epoch>\n" the instant it sees one.
#
# The board only ever sends this once per RTC's actual lifetime: the first
# flash ever, or after the PCF85063A's backup supply has been fully drained.
# Every other boot finds the RTC already valid and never asks. Run this
# *before* triggering a flash/reset if you're deliberately re-provisioning
# (see main.rs's read-first provisioning block) -- it just sits quietly if
# the board never asks.
#
# Usage: reply_time_request.sh [port] [timeout_seconds]

set -euo pipefail

PORT="${1:-/dev/cu.usbmodem21101}"
TIMEOUT="${2:-30}"

if [ ! -e "$PORT" ]; then
    echo "no such port: $PORT" >&2
    exit 1
fi

# -hupcl: opening/closing this fd must not toggle DTR, which the S3's native
# USB-Serial-JTAG reads as a reset-into-bootloader request.
stty -f "$PORT" -hupcl clocal raw -echo

echo "watching $PORT for TIME_REQUEST (up to ${TIMEOUT}s)..." >&2

end=$(( $(date +%s) + TIMEOUT ))
while [ "$(date +%s)" -lt "$end" ]; do
    if IFS= read -r -t 1 line < "$PORT"; then
        case "$line" in
            *TIME_REQUEST*)
                epoch=$(date -u +%s)
                printf 'SET_EPOCH %s\n' "$epoch" > "$PORT"
                echo "saw TIME_REQUEST, replied: SET_EPOCH $epoch ($(date -u -r "$epoch" 2>/dev/null || date -u))" >&2
                exit 0
                ;;
        esac
    fi
done

echo "timed out waiting for TIME_REQUEST -- board will fall back to its compiled-in KNOWN_EPOCH" >&2
exit 1
