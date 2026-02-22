#!/usr/bin/env python3
"""Extract touch_trace rows from a raw serial monitor log.

Usage:
  import_touch_log.py <input_log> <output_trace_csv> [--keep-absolute-time]
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


def usage() -> str:
    return (
        "usage: tools/touch_replay/import_touch_log.py "
        "<input_log> <output_trace_csv> [--keep-absolute-time]"
    )


def parse_int(token: str, field: str, line_no: int) -> int:
    token = token.strip()
    try:
        return int(token, 10)
    except ValueError as exc:
        raise ValueError(f"line {line_no}: invalid {field} '{token}'") from exc


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(usage(), file=sys.stderr)
        return 2

    keep_absolute_time = False
    if len(argv) > 3:
        if len(argv) == 4 and argv[3] == "--keep-absolute-time":
            keep_absolute_time = True
        else:
            print(usage(), file=sys.stderr)
            return 2

    input_path = Path(argv[1]).expanduser().resolve()
    output_path = Path(argv[2]).expanduser().resolve()

    if not input_path.exists():
        print(f"input file not found: {input_path}", file=sys.stderr)
        return 2

    rows: list[list[str]] = []

    with input_path.open("r", encoding="utf-8", errors="replace") as handle:
        for line_no, line in enumerate(handle, start=1):
            cleaned = ANSI_RE.sub("", line)
            marker = cleaned.find("touch_trace,")
            if marker < 0:
                continue

            candidate = cleaned[marker:].strip()
            parts = [part.strip() for part in candidate.split(",")]
            if len(parts) < len(HEADER):
                continue

            parts = parts[: len(HEADER)]
            if parts[0] != "touch_trace":
                continue
            if parts[1] == "ms":
                continue

            try:
                parse_int(parts[1], "ms", line_no)
                parse_int(parts[2], "count", line_no)
                for idx, field in enumerate(("x0", "y0", "x1", "y1"), start=3):
                    parse_int(parts[idx], field, line_no)
            except ValueError as err:
                print(str(err), file=sys.stderr)
                return 2

            rows.append(parts)

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
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
