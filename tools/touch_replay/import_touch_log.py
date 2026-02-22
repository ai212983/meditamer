#!/usr/bin/env python3
"""Extract touch_trace rows from a raw serial monitor log.

Usage:
  import_touch_log.py <input_log> <output_trace_csv> [--keep-absolute-time]
                      [--events-output <expected_kinds.txt>]
"""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

HEADER = [
    "touch_trace",
    "ms",
    "count",
    "x0",
    "y0",
    "x1",
    "y1",
    "raw0",
    "raw1",
    "raw2",
    "raw3",
    "raw4",
    "raw5",
    "raw6",
    "raw7",
]

ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
EVENT_KINDS = {
    "down",
    "move",
    "up",
    "tap",
    "long_press",
    "swipe_left",
    "swipe_right",
    "swipe_up",
    "swipe_down",
    "cancel",
}


def usage() -> str:
    return (
        "usage: tools/touch_replay/import_touch_log.py "
        "<input_log> <output_trace_csv> [--keep-absolute-time] "
        "[--events-output <expected_kinds.txt>]"
    )


def parse_int(token: str, field: str, line_no: int) -> int:
    token = token.strip()
    try:
        return int(token, 10)
    except ValueError as exc:
        raise ValueError(f"line {line_no}: invalid {field} '{token}'") from exc


def parse_args(argv: list[str]) -> tuple[Path, Path, bool, Path | None]:
    if len(argv) < 3:
        raise ValueError(usage())

    input_path = Path(argv[1]).expanduser().resolve()
    output_path = Path(argv[2]).expanduser().resolve()
    keep_absolute_time = False
    events_output: Path | None = None

    i = 3
    while i < len(argv):
        arg = argv[i]
        if arg == "--keep-absolute-time":
            keep_absolute_time = True
            i += 1
            continue
        if arg == "--events-output":
            i += 1
            if i >= len(argv):
                raise ValueError(usage())
            events_output = Path(argv[i]).expanduser().resolve()
            i += 1
            continue
        raise ValueError(usage())

    return input_path, output_path, keep_absolute_time, events_output


def main(argv: list[str]) -> int:
    try:
        input_path, output_path, keep_absolute_time, events_output = parse_args(argv)
    except ValueError as err:
        print(str(err), file=sys.stderr)
        return 2

    if not input_path.exists():
        print(f"input file not found: {input_path}", file=sys.stderr)
        return 2

    rows: list[list[str]] = []
    event_kinds: list[str] = []

    with input_path.open("r", encoding="utf-8", errors="replace") as handle:
        for line_no, line in enumerate(handle, start=1):
            cleaned = ANSI_RE.sub("", line)

            trace_marker = cleaned.find("touch_trace,")
            if trace_marker >= 0:
                candidate = cleaned[trace_marker:].strip()
                parts = [part.strip() for part in candidate.split(",")]
                if len(parts) >= len(HEADER):
                    parts = parts[: len(HEADER)]
                    if parts[0] == "touch_trace" and parts[1] != "ms":
                        try:
                            parse_int(parts[1], "ms", line_no)
                            parse_int(parts[2], "count", line_no)
                            for idx, field in enumerate(("x0", "y0", "x1", "y1"), start=3):
                                parse_int(parts[idx], field, line_no)
                        except ValueError as err:
                            print(str(err), file=sys.stderr)
                            return 2
                        rows.append(parts)

            event_marker = cleaned.find("touch_event,")
            if event_marker >= 0:
                candidate = cleaned[event_marker:].strip()
                parts = [part.strip() for part in candidate.split(",")]
                if len(parts) < 3 or parts[0] != "touch_event":
                    continue
                if parts[1] == "ms" or parts[2] == "kind":
                    continue

                try:
                    parse_int(parts[1], "event_ms", line_no)
                except ValueError as err:
                    print(str(err), file=sys.stderr)
                    return 2

                kind = parts[2].strip().lower()
                if kind not in EVENT_KINDS:
                    print(f"line {line_no}: invalid event kind '{parts[2]}'", file=sys.stderr)
                    return 2
                event_kinds.append(kind)

    if not rows:
        print(f"no touch_trace rows found in: {input_path}", file=sys.stderr)
        return 1

    if not keep_absolute_time:
        base_ms = int(rows[0][1], 10)
        for row in rows:
            row[1] = str(int(row[1], 10) - base_ms)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(HEADER)
        writer.writerows(rows)

    print(f"wrote {len(rows)} touch rows -> {output_path}")

    if events_output is not None:
        events_output.parent.mkdir(parents=True, exist_ok=True)
        with events_output.open("w", encoding="utf-8") as handle:
            for kind in event_kinds:
                handle.write(kind)
                handle.write("\n")
        print(f"wrote {len(event_kinds)} touch events -> {events_output}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
