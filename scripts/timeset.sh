#!/usr/bin/env bash

set -euo pipefail

baud="${ESPFLASH_BAUD:-115200}"
port="${ESPFLASH_PORT:-}"
epoch_arg="${1:-}"
tz_arg="${2:-}"
settle_ms="${TIMESET_SETTLE_MS:-1500}"
retries="${TIMESET_RETRIES:-8}"
retry_delay_ms="${TIMESET_RETRY_DELAY_MS:-700}"
wait_ack="${TIMESET_WAIT_ACK:-1}"
ack_timeout_ms="${TIMESET_ACK_TIMEOUT_MS:-1200}"

calc_local_tz_offset_minutes() {
    local raw sign hh mm total
    raw="$(date +%z)"
    sign="${raw:0:1}"
    hh="${raw:1:2}"
    mm="${raw:3:2}"
    total=$((10#$hh * 60 + 10#$mm))
    if [[ "$sign" == "-" ]]; then
        total=$((-total))
    fi
    printf '%s' "$total"
}

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

if [[ -n "$epoch_arg" ]]; then
    epoch="$epoch_arg"
else
    epoch="$(date -u +%s)"
fi

if [[ -n "$tz_arg" ]]; then
    tz_offset_minutes="$tz_arg"
else
    tz_offset_minutes="$(calc_local_tz_offset_minutes)"
fi

if ! [[ "$epoch" =~ ^[0-9]+$ ]]; then
    echo "epoch must be an unsigned integer (UTC seconds since Unix epoch)" >&2
    exit 1
fi

if ! [[ "$tz_offset_minutes" =~ ^-?[0-9]+$ ]]; then
    echo "tz_offset_minutes must be an integer number of minutes (e.g. -300)" >&2
    exit 1
fi

if (( tz_offset_minutes < -720 || tz_offset_minutes > 840 )); then
    echo "tz_offset_minutes must be within -720..840" >&2
    exit 1
fi

if ! [[ "$settle_ms" =~ ^[0-9]+$ ]] || ! [[ "$retries" =~ ^[0-9]+$ ]] || ! [[ "$retry_delay_ms" =~ ^[0-9]+$ ]] || ! [[ "$wait_ack" =~ ^[01]$ ]] || ! [[ "$ack_timeout_ms" =~ ^[0-9]+$ ]]; then
    echo "TIMESET_* values must be valid integers (TIMESET_WAIT_ACK: 0|1)" >&2
    exit 1
fi
if (( retries == 0 )); then
    echo "TIMESET_RETRIES must be >= 1" >&2
    exit 1
fi

python3 - "$port" "$baud" "$epoch" "$tz_offset_minutes" "$settle_ms" "$retries" "$retry_delay_ms" "$wait_ack" "$ack_timeout_ms" <<'PY'
import os
import select
import struct
import sys
import termios
import time
import tty
import fcntl

port = sys.argv[1]
baud = int(sys.argv[2])
epoch = sys.argv[3]
tz = sys.argv[4]
settle_ms = int(sys.argv[5])
retries = int(sys.argv[6])
retry_delay_ms = int(sys.argv[7])
wait_ack = int(sys.argv[8]) == 1
ack_timeout_ms = int(sys.argv[9])

speed = getattr(termios, f"B{baud}", None)
if speed is None:
    print(f"Unsupported baud rate for termios: {baud}", file=sys.stderr)
    sys.exit(2)

fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
try:
    tty.setraw(fd)
    attrs = termios.tcgetattr(fd)
    attrs[4] = speed
    attrs[5] = speed
    attrs[6][termios.VMIN] = 0
    attrs[6][termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, attrs)

    dtr = getattr(termios, "TIOCM_DTR", 0x002)
    rts = getattr(termios, "TIOCM_RTS", 0x004)
    tiocmbic = getattr(termios, "TIOCMBIC", None)
    if tiocmbic is not None:
        try:
            fcntl.ioctl(fd, tiocmbic, struct.pack("I", dtr | rts))
        except OSError:
            pass

    if settle_ms:
        time.sleep(settle_ms / 1000.0)

    payload = f"TIMESET {epoch} {tz}\r\n".encode("ascii")
    rx = bytearray()
    for attempt in range(retries):
        os.write(fd, payload)
        if wait_ack:
            deadline = time.monotonic() + (ack_timeout_ms / 1000.0)
            while time.monotonic() < deadline:
                r, _, _ = select.select([fd], [], [], 0.05)
                if not r:
                    continue
                try:
                    data = os.read(fd, 256)
                except BlockingIOError:
                    continue
                if not data:
                    continue
                rx.extend(data)
                if len(rx) > 4096:
                    del rx[:-1024]
                if b"TIMESET OK" in rx:
                    print(
                        f"Sent ({attempt + 1}x) with ACK: TIMESET {epoch} {tz} -> {port} @ {baud}"
                    )
                    sys.exit(0)
        if attempt + 1 < retries and retry_delay_ms:
            time.sleep(retry_delay_ms / 1000.0)

    if wait_ack:
        print(
            f"No TIMESET ACK after {retries} attempts: TIMESET {epoch} {tz} -> {port} @ {baud}",
            file=sys.stderr,
        )
        sys.exit(3)

    print(f"Sent ({retries}x): TIMESET {epoch} {tz} -> {port} @ {baud}")
finally:
    os.close(fd)
PY
