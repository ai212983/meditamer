#!/usr/bin/env python3
"""Convert Meditamer's line-oriented firmware traces to Perfetto JSON."""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable
from urllib.parse import unquote_to_bytes


class TraceFormatError(ValueError):
    """The input does not contain one complete, unambiguous capture."""


@dataclass(frozen=True)
class Event:
    seq: int
    ts: int
    core: int
    context_kind: str
    context_id: int
    target: str
    name: str
    fields: dict[str, int | float | bool]


@dataclass(frozen=True)
class Capture:
    generation: int
    counters: dict[str, int]
    events: tuple[Event, ...]
    incomplete_reasons: tuple[str, ...]


_REQUIRED_HEADER = {"version", "generation", "count", "overflow", "unsupported", "hook_failures"}
_REQUIRED_EVENT = {"seq", "ts", "core", "context_kind", "context_id", "target", "name"}
_UI_STAGES = {
    1: "acquired", 2: "pipeline received", 3: "suppressed", 4: "reset",
    5: "gesture generated", 6: "publication interrupted", 7: "UI dequeued",
    8: "render result", 9: "panel begin", 10: "panel end",
    11: "input discarded by reset", 12: "gesture-event discarded",
    13: "multitouch discarded", 14: "suppression cleared", 15: "suppression armed",
    20: "LVGL pressed", 21: "released", 22: "clicked", 23: "press lost",
    24: "scroll begin", 25: "scroll end", 26: "navigation callback",
    27: "settings callback", 28: "dismiss callback", 29: "ambient callback",
    30: "multi dequeue", 31: "multi contacts", 32: "multi render",
    33: "multi reset", 34: "render unknown", 35: "panel unknown",
    36: "navigation input closed", 37: "navigation presented",
    38: "transition input discarded",
}
_EXPLICIT_FAILURE_STAGES = {3, 6, 11, 12, 13, 38}
_CONTEXT_KINDS = {0: "thread", 1: "task", 2: "IRQ"}
_NON_LOSS_HEADER_FIELDS = {"version", "generation", "count"}


def _pairs(text: str, *, line: int) -> dict[str, str]:
    result: dict[str, str] = {}
    for token in text.split():
        if "=" not in token:
            raise TraceFormatError(f"line {line}: truncated record near {token!r}")
        key, value = token.split("=", 1)
        if not key or value == "" or key in result:
            raise TraceFormatError(f"line {line}: malformed or duplicate key {key!r}")
        result[key] = value
    return result


def _integer(value: str, label: str, line: int, *, signed: bool = False) -> int:
    pattern = r"-?[0-9]+" if signed else r"[0-9]+"
    if not re.fullmatch(pattern, value):
        raise TraceFormatError(f"line {line}: {label} is not a valid integer")
    return int(value)


def _decode(value: str, label: str, line: int) -> str:
    if re.search(r"%(?![0-9A-Fa-f]{2})", value):
        raise TraceFormatError(f"line {line}: invalid percent encoding in {label}")
    try:
        return unquote_to_bytes(value).decode("utf-8")
    except UnicodeDecodeError as exc:
        raise TraceFormatError(f"line {line}: invalid UTF-8 in {label}") from exc


def _float(value: str, label: str, line: int) -> float:
    if not re.fullmatch(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?", value):
        raise TraceFormatError(f"line {line}: {label} is not a valid finite float")
    result = float(value)
    if not math.isfinite(result):
        raise TraceFormatError(f"line {line}: {label} is not a valid finite float")
    return result


def _event(text: str, line: int) -> Event:
    values = _pairs(text, line=line)
    missing = _REQUIRED_EVENT - values.keys()
    unexpected = [key for key in values if key not in _REQUIRED_EVENT and not key.startswith("f.")]
    if missing or unexpected:
        detail = f"missing {', '.join(sorted(missing))}" if missing else f"unexpected {unexpected[0]}"
        raise TraceFormatError(f"line {line}: truncated or malformed event ({detail})")
    fields: dict[str, int | float | bool] = {}
    for wire_key, wire_value in values.items():
        if not wire_key.startswith("f."):
            continue
        field_name = _decode(wire_key[2:], "field name", line)
        if not field_name or field_name in fields or len(wire_value) < 2 or wire_value[1] != ":":
            raise TraceFormatError(f"line {line}: malformed field {wire_key!r}")
        prefix, payload = wire_value[0], wire_value[2:]
        if prefix == "u":
            fields[field_name] = _integer(payload, field_name, line)
        elif prefix == "i":
            fields[field_name] = _integer(payload, field_name, line, signed=True)
        elif prefix == "b" and payload in ("0", "1"):
            fields[field_name] = payload == "1"
        elif prefix == "f":
            fields[field_name] = _float(payload, field_name, line)
        else:
            raise TraceFormatError(f"line {line}: invalid typed value for {field_name!r}")
    return Event(
        seq=_integer(values["seq"], "seq", line),
        ts=_integer(values["ts"], "ts", line),
        core=_integer(values["core"], "core", line),
        context_kind=_decode(values["context_kind"], "context_kind", line),
        context_id=_integer(values["context_id"], "context_id", line),
        target=_decode(values["target"], "target", line),
        name=_decode(values["name"], "name", line),
        fields=fields,
    )


def parse_capture(lines: Iterable[str], generation: int | None = None) -> Capture:
    captures: list[tuple[dict[str, int], list[Event], bool, list[str]]] = []
    active: tuple[dict[str, int], list[Event], bool, list[str]] | None = None
    saw_trace = False
    status_counts: dict[int, int] = {}
    for line_number, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line.startswith("TRACE"):
            continue
        saw_trace = True
        if line.startswith("TRACE SELFTEST "):
            if not re.fullmatch(r"TRACE SELFTEST OK generation=[0-9]+", line):
                raise TraceFormatError(f"line {line_number}: malformed TRACE self-test acknowledgement")
            continue
        if line in {"TRACE START", "TRACE START OK", "TRACE STOP", "TRACE STOP OK", "TRACE ARM", "TRACE ARM OK", "TRACE STATUS"}:
            raise TraceFormatError(f"line {line_number}: truncated TRACE acknowledgement")
        if line.startswith("TRACE STATUS "):
            values = _pairs(line[len("TRACE STATUS "):], line=line_number)
            if set(values) != {"state", "generation", "count"} or values["state"] not in {"armed", "capturing", "frozen", "full"}:
                raise TraceFormatError(f"line {line_number}: malformed TRACE status")
            status_generation = _integer(values["generation"], "generation", line_number)
            status_count = _integer(values["count"], "count", line_number)
            if status_count < status_counts.get(status_generation, 0):
                raise TraceFormatError(f"line {line_number}: live record count decreased within generation {status_generation}")
            status_counts[status_generation] = status_count
            continue
        acknowledgement_match = re.fullmatch(r"TRACE (START|STOP|ARM) (.+)", line)
        if acknowledgement_match:
            operation, acknowledgement = acknowledgement_match.groups()
            if acknowledgement.startswith("OK "):
                acknowledgement = acknowledgement[3:]
            values = _pairs(acknowledgement, line=line_number)
            required_ack = {"generation", "capacity"} if operation in {"START", "ARM"} else {"generation", "count"}
            if set(values) != required_ack:
                raise TraceFormatError(f"line {line_number}: malformed TRACE acknowledgement")
            for key, value in values.items():
                _integer(value, key, line_number)
            continue
        if line.startswith("TRACE BEGIN "):
            if active is not None:
                raise TraceFormatError(f"line {line_number}: new capture begins before prior capture ends")
            raw_header = _pairs(line[len("TRACE BEGIN "):], line=line_number)
            missing = _REQUIRED_HEADER - raw_header.keys()
            if missing:
                raise TraceFormatError(f"line {line_number}: incomplete header; missing {', '.join(sorted(missing))}")
            header = {key: _integer(value, key, line_number) for key, value in raw_header.items()}
            if header["version"] != 1:
                raise TraceFormatError(f"line {line_number}: unsupported trace version {header['version']}")
            active = (header, [], False, [])
            captures.append(active)
        elif line.startswith("TRACE EVENT "):
            if active is None:
                raise TraceFormatError(f"line {line_number}: event before TRACE BEGIN")
            active[1].append(_event(line[len("TRACE EVENT "):], line_number))
        elif line.startswith("TRACE END "):
            if active is None:
                raise TraceFormatError(f"line {line_number}: TRACE END without TRACE BEGIN")
            end = _pairs(line[len("TRACE END "):], line=line_number)
            if set(end) != {"generation"}:
                raise TraceFormatError(f"line {line_number}: malformed TRACE END")
            end_generation = _integer(end["generation"], "generation", line_number)
            if end_generation != active[0]["generation"]:
                raise TraceFormatError(f"line {line_number}: generation mismatch at TRACE END")
            active = (active[0], active[1], True, active[3])
            captures[-1] = active
            active = None
        elif line == "TRACE ERROR" or line.startswith("TRACE ERROR "):
            if active is not None:
                active[3].append(f"firmware reported {line}")
            else:
                raise TraceFormatError(f"line {line_number}: {line} without an active capture")
        else:
            raise TraceFormatError(f"line {line_number}: unrecognized or truncated TRACE record")
    if not saw_trace or not captures:
        raise TraceFormatError("missing TRACE BEGIN")
    matching = [capture for capture in captures if generation is None or capture[0]["generation"] == generation]
    if not matching:
        raise TraceFormatError(f"generation {generation} not found")
    if len(matching) != 1:
        raise TraceFormatError("multiple captures found; select one with --generation")
    header, events, ended, errors = matching[0]
    if not ended:
        raise TraceFormatError(f"generation {header['generation']}: missing TRACE END")
    if len(events) != header["count"]:
        raise TraceFormatError(
            f"generation {header['generation']}: header count {header['count']} does not match {len(events)} events"
        )
    sequences = [event.seq for event in events]
    if sequences:
        expected = list(range(sequences[0], sequences[0] + len(sequences)))
        if sequences != expected:
            raise TraceFormatError("event sequence contains a gap, duplicate, or reordering")
    incomplete = list(errors)
    labels = {"overflow": "overflow", "unsupported": "unsupported events",
              "hook_failures": "hook failures", "capacity": "capacity field losses",
              "parented": "parented events"}
    for key, value in header.items():
        if key not in _NON_LOSS_HEADER_FIELDS and value:
            incomplete.append(f"{labels.get(key, key.replace('_', ' '))}: {value}")
    return Capture(header["generation"], header, tuple(events), tuple(incomplete))


def _track_ids(events: tuple[Event, ...]) -> dict[tuple[int, str, int], int]:
    keys = sorted({(event.core, event.context_kind, event.context_id) for event in events})
    return {key: index + 1 for index, key in enumerate(keys)}


def export_capture(capture: Capture) -> dict[str, object]:
    tracks = _track_ids(capture.events)
    output: list[dict[str, object]] = []
    for core in sorted({event.core for event in capture.events}):
        output.append({"ph": "M", "name": "process_name", "pid": core + 1, "tid": 0,
                       "args": {"name": f"core {core}"}})
    for (core, kind, ident), tid in tracks.items():
        kind_name = _CONTEXT_KINDS.get(int(kind), kind) if kind.isdigit() else kind
        output.append({"ph": "M", "name": "thread_name", "pid": core + 1, "tid": tid,
                       "args": {"name": f"{kind_name} {ident}"}})
    flows: dict[int, list[tuple[Event, dict[str, object]]]] = {}
    for event in capture.events:
        name = event.name
        args: dict[str, int | float | bool] = {"seq": event.seq, **event.fields}
        if event.target == "interaction" and event.name == "stage" and isinstance(event.fields.get("stage"), int):
            name = _UI_STAGES.get(int(event.fields["stage"]), f"stage {event.fields['stage']}")
        item: dict[str, object] = {
            "name": name, "cat": event.target, "ph": "i", "s": "t", "ts": event.ts,
            "pid": event.core + 1, "tid": tracks[(event.core, event.context_kind, event.context_id)],
            "args": args,
        }
        output.append(item)
        flow_id = event.fields.get("flow_id")
        if isinstance(flow_id, int) and not isinstance(flow_id, bool) and flow_id > 0:
            flows.setdefault(flow_id, []).append((event, item))
    for flow_id, points in flows.items():
        points.sort(key=lambda point: (point[0].ts, point[0].seq))
        if len(points) < 2:
            continue
        for index, (event, item) in enumerate(points):
            phase = "s" if index == 0 else "f" if index == len(points) - 1 else "t"
            output.append({"name": "flow", "cat": "interaction", "ph": phase, "id": flow_id,
                           "ts": event.ts, "pid": item["pid"], "tid": item["tid"]})
    metadata = {"generation": capture.generation, **capture.counters,
                "evidence_complete": not capture.incomplete_reasons,
                "incomplete_reasons": list(capture.incomplete_reasons)}
    output.append({"ph": "M", "name": "trace_metadata", "pid": 0, "tid": 0, "args": metadata})
    return {"traceEvents": output, "displayTimeUnit": "us", "metadata": metadata}


def _fmt_ms(value: float) -> str:
    return f"{value:.3f} ms"


def render_report(capture: Capture) -> str:
    status = "incomplete" if capture.incomplete_reasons else "complete"
    lines = [f"# Trace generation {capture.generation}", "", f"Evidence status: **{status}**.", ""]
    if capture.incomplete_reasons:
        lines.extend(["Incomplete evidence:", ""] + [f"- {reason}" for reason in capture.incomplete_reasons] + [""])
    flows: dict[int, list[Event]] = {}
    for event in capture.events:
        flow_id = event.fields.get("flow_id")
        if isinstance(flow_id, int) and not isinstance(flow_id, bool) and flow_id > 0:
            flows.setdefault(flow_id, []).append(event)
    lines.extend(["## Flows", ""])
    if not flows:
        lines.extend(["No attributed flows were recorded.", ""])
    for flow_id, events in sorted(flows.items()):
        events.sort(key=lambda event: (event.ts, event.seq))
        stage_events = [(event, int(event.fields["stage"])) for event in events
                        if event.target == "interaction" and event.name == "stage"
                        and isinstance(event.fields.get("stage"), int)]
        stage_names = [_UI_STAGES.get(stage, f"stage {stage}") for _, stage in stage_events]
        lines.append(f"### Flow {flow_id}")
        lines.append("")
        lines.append(
            f"- First/last: {events[0].ts} us (seq {events[0].seq}) / "
            f"{events[-1].ts} us (seq {events[-1].seq})"
        )
        lines.append(f"- Stages: {' -> '.join(stage_names) if stage_names else '(generic events only)'}")
        for event, stage in stage_events:
            if stage == 7 and isinstance(event.fields.get("source_ms"), int):
                lines.append(f"- UI queue latency: {_fmt_ms(event.ts / 1000 - int(event.fields['source_ms']))}")
        begins = [event for event, stage in stage_events if stage == 9]
        ends = [event for event, stage in stage_events if stage == 10]
        if begins and ends:
            end = next((candidate for candidate in ends if candidate.ts >= begins[0].ts), None)
            if end:
                lines.append(f"- Panel duration: {_fmt_ms((end.ts - begins[0].ts) / 1000)}")
        failures = [_UI_STAGES[stage] for _, stage in stage_events if stage in _EXPLICIT_FAILURE_STAGES]
        if failures:
            lines.append(f"- Explicit recorded outcome: {', '.join(failures)}")
        stages = {stage for _, stage in stage_events}
        if stage_events and (1 not in stages or not stages.intersection({3, 5, 6, 8, 10, 11, 12, 13, 38})):
            lines.append("- Missing acquisition or terminal stages do not establish a failure.")
        lines.append("")
    lines.extend([
        "## Interpretation limits", "",
        "- Panel completion is a software acknowledgement, not proof of an optical update.",
        "- A flow missing acquisition or terminal records is not, by itself, an established failure.",
        "- Missing records are not interpreted as input drops.", "",
    ])
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="captured console log")
    parser.add_argument("--output", type=Path, required=True, help="Perfetto-compatible JSON output")
    parser.add_argument("--report", type=Path, required=True, help="Markdown evidence report")
    parser.add_argument("--generation", type=int, help="select one generation from a multi-capture log")
    args = parser.parse_args(argv)
    try:
        with args.input.open(encoding="utf-8") as source:
            capture = parse_capture(source, args.generation)
        args.output.write_text(json.dumps(export_capture(capture), indent=2) + "\n", encoding="utf-8")
        args.report.write_text(render_report(capture), encoding="utf-8")
    except (OSError, TraceFormatError) as exc:
        parser.exit(1, f"trace_export: {exc}\n")
    if capture.incomplete_reasons:
        print("trace_export: exported incomplete evidence: " + "; ".join(capture.incomplete_reasons), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
