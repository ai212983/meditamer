#!/usr/bin/env bash

set -euo pipefail

baud="${ESPFLASH_BAUD:-115200}"
port="${ESPFLASH_PORT:-}"
ssid="${1:-}"
password="${2:-}"
settle_ms="${WIFISET_SETTLE_MS:-1200}"
retries="${WIFISET_RETRIES:-8}"
retry_delay_ms="${WIFISET_RETRY_DELAY_MS:-700}"
wait_ack="${WIFISET_WAIT_ACK:-1}"
ack_timeout_ms="${WIFISET_ACK_TIMEOUT_MS:-1500}"

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-510)" >&2
    exit 1
fi

if [[ -z "$ssid" ]]; then
    echo "Usage: ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/wifiset.sh <ssid> [password]" >&2
    echo "Note: SSID/password with spaces are not supported by current UART parser." >&2
    exit 1
fi

if [[ "$ssid" == *" "* ]] || [[ "$password" == *" "* ]]; then
    echo "SSID/password must not contain spaces for current WIFISET parser" >&2
    exit 1
fi

if (( ${#ssid} > 32 )); then
    echo "SSID too long: max 32 bytes" >&2
    exit 1
fi
if (( ${#password} > 64 )); then
    echo "Password too long: max 64 bytes" >&2
    exit 1
fi

if ! [[ "$settle_ms" =~ ^[0-9]+$ ]] || ! [[ "$retries" =~ ^[0-9]+$ ]] || ! [[ "$retry_delay_ms" =~ ^[0-9]+$ ]] || ! [[ "$wait_ack" =~ ^[01]$ ]] || ! [[ "$ack_timeout_ms" =~ ^[0-9]+$ ]]; then
    echo "WIFISET_* values must be valid integers (WIFISET_WAIT_ACK: 0|1)" >&2
    exit 1
fi
if (( retries == 0 )); then
    echo "WIFISET_RETRIES must be >= 1" >&2
    exit 1
fi

python3 - "$port" "$baud" "$ssid" "$password" "$settle_ms" "$retries" "$retry_delay_ms" "$wait_ack" "$ack_timeout_ms" <<'PY'
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
ssid = sys.argv[3]
password = sys.argv[4]
settle_ms = int(sys.argv[5])
retries = int(sys.argv[6])
retry_delay_ms = int(sys.argv[7])
wait_ack = int(sys.argv[8]) == 1
ack_timeout_ms = int(sys.argv[9])

speed = getattr(termios, f"B{baud}", None)
if speed is None:
    print(f"Unsupported baud rate for termios: {baud}", file=sys.stderr)
    sys.exit(2)

if password:
    payload = f"WIFISET {ssid} {password}\r\n".encode("ascii", errors="strict")
else:
    payload = f"WIFISET {ssid}\r\n".encode("ascii", errors="strict")

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

                if b"WIFISET OK" in rx:
                    print(
                        f"Sent ({attempt + 1}x) with ACK: WIFISET <ssid> <redacted> -> {port} @ {baud}"
                    )
                    sys.exit(0)
                if b"WIFISET BUSY" in rx:
                    break

        if attempt + 1 < retries and retry_delay_ms:
            time.sleep(retry_delay_ms / 1000.0)

    if wait_ack:
        print(
            f"No WIFISET OK ACK after {retries} attempts on {port} @ {baud}",
            file=sys.stderr,
        )
        sys.exit(3)

    print(f"Sent ({retries}x): WIFISET <ssid> <redacted> -> {port} @ {baud}")
finally:
    os.close(fd)
PY
