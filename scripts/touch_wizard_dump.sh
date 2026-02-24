#!/usr/bin/env bash

set -euo pipefail

port="${ESPFLASH_PORT:-}"
baud="${ESPFLASH_BAUD:-115200}"
timeout_ms="${TOUCH_WIZARD_DUMP_TIMEOUT_MS:-8000}"
retries="${TOUCH_WIZARD_DUMP_RETRIES:-3}"
settle_ms="${TOUCH_WIZARD_DUMP_SETTLE_MS:-200}"
output_path="${1:-logs/touch_wizard_dump_$(date +%Y%m%d_%H%M%S).log}"

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

if ! [[ "$baud" =~ ^[0-9]+$ ]] || ! [[ "$timeout_ms" =~ ^[0-9]+$ ]] || ! [[ "$retries" =~ ^[0-9]+$ ]] || ! [[ "$settle_ms" =~ ^[0-9]+$ ]]; then
    echo "ESPFLASH_BAUD / TOUCH_WIZARD_DUMP_* must be unsigned integers" >&2
    exit 1
fi
if (( retries == 0 )); then
    echo "TOUCH_WIZARD_DUMP_RETRIES must be >= 1" >&2
    exit 1
fi

mkdir -p "$(dirname "$output_path")"

python3 - "$port" "$baud" "$timeout_ms" "$retries" "$settle_ms" "$output_path" <<'PY'
import os
import re
import select
import sys
import termios
import time
import tty

port = sys.argv[1]
baud = int(sys.argv[2])
timeout_ms = int(sys.argv[3])
retries = int(sys.argv[4])
settle_ms = int(sys.argv[5])
output_path = sys.argv[6]

speed = getattr(termios, f"B{baud}", None)
if speed is None:
    print(f"Unsupported baud for termios: {baud}", file=sys.stderr)
    sys.exit(2)

line_re = re.compile(
    r"^touch_wizard_swipe,\d+,\d+,\d+,[a-z_]+,[a-z_]+,[a-z_]+,[a-z_]+,\d+,\d+,\d+,\d+,\d+,\d+,\d+,\d+,\d+$"
)

def latest_complete_dump_segment(text: str):
    end_idx = text.rfind("TOUCH_WIZARD_DUMP END")
    if end_idx == -1:
        return None
    begin_idx = text.rfind("TOUCH_WIZARD_DUMP BEGIN", 0, end_idx)
    if begin_idx == -1:
        return None
    line_end = text.find("\n", end_idx)
    if line_end == -1:
        line_end = len(text)
    else:
        line_end += 1
    return text[begin_idx:line_end]

def parse_validation(text: str):
    segment = latest_complete_dump_segment(text)
    if segment is None:
        return (False, None, 0, "missing complete BEGIN..END segment", None)
    m = re.search(r"samples=(\d+)", segment)
    if not m:
        return (False, None, 0, "missing samples header field", segment)
    expected = int(m.group(1))
    lines = 0
    for raw in segment.splitlines():
        line = raw.strip()
        if line_re.match(line):
            lines += 1
    return (lines == expected, expected, lines, None, segment)

def capture_once():
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        tty.setraw(fd)
        attrs = termios.tcgetattr(fd)
        attrs[4] = speed
        attrs[5] = speed
        attrs[6][termios.VMIN] = 0
        attrs[6][termios.VTIME] = 0
        termios.tcsetattr(fd, termios.TCSANOW, attrs)
        if settle_ms:
            time.sleep(settle_ms / 1000.0)

        os.write(fd, b"TOUCH_WIZARD_DUMP\r\n")
        deadline = time.monotonic() + (timeout_ms / 1000.0)
        buf = bytearray()
        saw_end = False
        while time.monotonic() < deadline:
            r, _, _ = select.select([fd], [], [], 0.05)
            if not r:
                continue
            try:
                data = os.read(fd, 4096)
            except BlockingIOError:
                continue
            if not data:
                continue
            buf.extend(data)
            if b"TOUCH_WIZARD_DUMP END" in buf:
                saw_end = True
                tail = time.monotonic() + 0.1
                while time.monotonic() < tail:
                    r2, _, _ = select.select([fd], [], [], 0.02)
                    if not r2:
                        continue
                    try:
                        d2 = os.read(fd, 4096)
                    except BlockingIOError:
                        continue
                    if d2:
                        buf.extend(d2)
                break
        return bytes(buf), saw_end
    finally:
        os.close(fd)

last_blob = b""
last_reason = "no data"
for attempt in range(1, retries + 1):
    blob, saw_end = capture_once()
    text = blob.decode("utf-8", "ignore")
    ok, expected, actual, reason, segment = parse_validation(text)
    if ok:
        output_text = segment if segment is not None else text
        with open(output_path, "wb") as f:
            f.write(output_text.encode("utf-8", "ignore"))
        print(
            f"TOUCH_WIZARD_DUMP OK: attempts={attempt} samples={actual} file={output_path}"
        )
        sys.exit(0)
    last_blob = blob
    if reason is not None:
        last_reason = reason
    elif not saw_end:
        last_reason = "timeout waiting for END marker"
    else:
        last_reason = f"samples mismatch expected={expected} actual={actual}"

with open(output_path, "wb") as f:
    f.write(last_blob)
print(
    f"TOUCH_WIZARD_DUMP FAILED after {retries} attempts: {last_reason}; captured={output_path}",
    file=sys.stderr,
)
sys.exit(3)
PY
