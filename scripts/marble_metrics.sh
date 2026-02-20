#!/usr/bin/env bash

set -euo pipefail

baud="${ESPFLASH_BAUD:-115200}"
port="${ESPFLASH_PORT:-}"
settle_ms="${METRICS_SETTLE_MS:-500}"
retries="${METRICS_RETRIES:-1}"
retry_delay_ms="${METRICS_RETRY_DELAY_MS:-300}"
timeout_ms="${METRICS_TIMEOUT_MS:-60000}"

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

python3 - "$port" "$baud" "$settle_ms" "$retries" "$retry_delay_ms" "$timeout_ms" <<'PY'
import os
import select
import struct
import sys
import termios
import time
import tty
import fcntl
import re

port = sys.argv[1]
baud = int(sys.argv[2])
settle_ms = int(sys.argv[3])
retries = int(sys.argv[4])
retry_delay_ms = int(sys.argv[5])
timeout_ms = int(sys.argv[6])

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

    payload = b"METRICS\r\n"
    rx = bytearray()
    for attempt in range(retries):
        os.write(fd, payload)
        deadline = time.monotonic() + (timeout_ms / 1000.0)
        while time.monotonic() < deadline:
            r, _, _ = select.select([fd], [], [], 0.05)
            if not r:
                continue
            try:
                data = os.read(fd, 512)
            except BlockingIOError:
                continue
            if not data:
                continue
            rx.extend(data)
            if len(rx) > 8192:
                del rx[:-2048]
            text = rx.decode("utf-8", "replace")
            for line in text.splitlines():
                match = re.match(
                    r"^METRICS\s+MARBLE_REDRAW_MS=(\d+)(?:\s+MAX_MS=(\d+))?$", line
                )
                if match:
                    if match.group(2) is not None:
                        print(
                            f"METRICS MARBLE_REDRAW_MS={match.group(1)} MAX_MS={match.group(2)}"
                        )
                    else:
                        print(f"METRICS MARBLE_REDRAW_MS={match.group(1)}")
                    sys.exit(0)
        if attempt + 1 < retries and retry_delay_ms:
            time.sleep(retry_delay_ms / 1000.0)

    print(
        f"No METRICS response after {retries} attempts -> {port} @ {baud}",
        file=sys.stderr,
    )
    sys.exit(3)
finally:
    os.close(fd)
PY
