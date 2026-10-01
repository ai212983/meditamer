#!/usr/bin/env bash
# Watches the board's USB-Serial-JTAG console for a "TIME_REQUEST" line and
# replies with that session's "TIME_REPLY" the instant it sees one, then logs
# the board's own "TIME_SYNC OK|ERR" confirmation if it arrives before this
# script's own follow-up timeout.
#
# The board sends one request after a non-deep-sleep boot: an invalid-clock
# requirement on first provisioning, or an optional valid-clock offer on an
# ordinary USB reset/reflash. The normal target flash script answers this
# automatically. Run this helper *before* a manual reset when validating the
# protocol directly; it sits quietly across timer/key deep-sleep wakes.
#
# Wire protocol: docs/references/wall-clock-sync.md.
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

# Host's current fixed UTC offset in minutes, from `date`'s own %z (e.g.
# "+0200" -> 120, "-0530" -> -330). Computed fresh for each reply rather
# than once at script start, so a long wait for TIME_REQUEST never sends a
# stale offset.
offset_minutes() {
    local z sign hh mm total
    z=$(date +%z)
    sign="${z:0:1}"
    hh="${z:1:2}"
    mm="${z:3:2}"
    total=$((10#$hh * 60 + 10#$mm))
    if [ "$sign" = "-" ]; then
        total=$((-total))
    fi
    echo "$total"
}

end=$(( $(date +%s) + TIMEOUT ))
while [ "$(date +%s)" -lt "$end" ]; do
    if IFS= read -r -t 1 line < "$PORT"; then
        case "$line" in
            *TIME_REQUEST*)
                session=$(sed -n 's/.*session=\([0-9]*\).*/\1/p' <<<"$line")
                nonce=$(sed -n 's/.*nonce=\([0-9]*\).*/\1/p' <<<"$line")
                if [ -z "$session" ] || [ -z "$nonce" ]; then
                    echo "malformed TIME_REQUEST, ignoring: $line" >&2
                    continue
                fi

                epoch=$(date -u +%s)
                offset=$(offset_minutes)
                reply="TIME_REPLY version=1 session=${session} nonce=${nonce} utc=${epoch} offset_min=${offset}"
                printf '%s\n' "$reply" > "$PORT"
                echo "saw TIME_REQUEST, replied: $reply ($(date -u -r "$epoch" 2>/dev/null || date -u))" >&2

                # The board's own TIME_SYNC OK|ERR is diagnostic here, not
                # this script's success signal -- log it if it shows up
                # within a short follow-up window, but don't fail if it
                # doesn't (a busy console, or this script racing a reset).
                confirm_end=$(( $(date +%s) + 5 ))
                confirmed=0
                while [ "$(date +%s)" -lt "$confirm_end" ]; do
                    if IFS= read -r -t 1 confirm < "$PORT"; then
                        case "$confirm" in
                            *"TIME_SYNC "*"session=${session}"*)
                                echo "board confirmed: $confirm" >&2
                                confirmed=1
                                break
                                ;;
                        esac
                    fi
                done
                if [ "$confirmed" -eq 0 ]; then
                    echo "no TIME_SYNC confirmation seen for session=${session} within 5s" >&2
                fi
                exit 0
                ;;
        esac
    fi
done

echo "timed out waiting for TIME_REQUEST -- board will preserve a valid RTC or use its fallback if invalid" >&2
exit 1
