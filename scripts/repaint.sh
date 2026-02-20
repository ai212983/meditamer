#!/usr/bin/env bash

set -euo pipefail

baud="${ESPFLASH_BAUD:-115200}"
port="${ESPFLASH_PORT:-}"
settle_ms="${REPAINT_SETTLE_MS:-200}"
retries="${REPAINT_RETRIES:-2}"
retry_delay_ms="${REPAINT_RETRY_DELAY_MS:-500}"
wait_ack="${REPAINT_WAIT_ACK:-1}"
ack_timeout_ms="${REPAINT_ACK_TIMEOUT_MS:-15000}"
cmd="${REPAINT_CMD:-REPAINT}"

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

if ! [[ "$settle_ms" =~ ^[0-9]+$ ]] || ! [[ "$retries" =~ ^[0-9]+$ ]] || ! [[ "$retry_delay_ms" =~ ^[0-9]+$ ]] || ! [[ "$wait_ack" =~ ^[01]$ ]] || ! [[ "$ack_timeout_ms" =~ ^[0-9]+$ ]]; then
    echo "REPAINT_* values must be valid integers (REPAINT_WAIT_ACK: 0|1)" >&2
    exit 1
fi
if (( retries == 0 )); then
    echo "REPAINT_RETRIES must be >= 1" >&2
    exit 1
fi

python3 - "$port" "$baud" "$settle_ms" "$retries" "$retry_delay_ms" "$wait_ack" "$ack_timeout_ms" "$cmd" <<'PY'
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
settle_ms = int(sys.argv[3])
retries = int(sys.argv[4])
retry_delay_ms = int(sys.argv[5])
wait_ack = int(sys.argv[6]) == 1
ack_timeout_ms = int(sys.argv[7])
cmd = sys.argv[8].strip()
if not cmd:
    cmd = "REPAINT"

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

    payload = (cmd + "\r\n").encode("ascii", "ignore")
    rx = bytearray()
    ack_ok = (cmd + " OK").encode("ascii", "ignore")
    ack_busy = (cmd + " BUSY").encode("ascii", "ignore")
    for attempt in range(retries):
        os.write(fd, payload)
        if wait_ack:
            deadline = time.monotonic() + (ack_timeout_ms / 1000.0)
            saw_busy = False
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
                if ack_ok in rx:
                    print(f"Sent ({attempt + 1}x) with ACK: {cmd} -> {port} @ {baud}")
                    sys.exit(0)
                if ack_busy in rx:
                    saw_busy = True
                    break
            if saw_busy and attempt + 1 < retries and retry_delay_ms:
                time.sleep(retry_delay_ms / 1000.0)
                continue
        if attempt + 1 < retries and retry_delay_ms:
            time.sleep(retry_delay_ms / 1000.0)

    if wait_ack:
        print(
            f"No {cmd} ACK after {retries} attempts: {cmd} -> {port} @ {baud}",
            file=sys.stderr,
        )
        sys.exit(3)

    print(f"Sent ({retries}x): {cmd} -> {port} @ {baud}")
finally:
    os.close(fd)
PY
