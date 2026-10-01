#!/usr/bin/env python3

"""Validate and summarize comparable reference/Rust refresh captures."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import statistics
import sys
import tomllib


ROOT = Path(__file__).resolve().parent
SPEC_PATH = ROOT / "common" / "spec.toml"


@dataclass(frozen=True)
class BenchmarkMode:
    cli_name: str
    benchmark_name: str
    phase: str
    display_mode: str
    kind: str
    power_state: str
    numbered_patterns: bool
    purpose: str
    samples_key: str
    phase_profile: bool


MODES = {
    "partial-1bit": BenchmarkMode(
        cli_name="partial-1bit",
        benchmark_name="partial_1bit",
        phase="partial_1bit_performance",
        display_mode="1bit",
        kind="partial",
        power_state="warm_held_on",
        numbered_patterns=True,
        purpose="performance",
        samples_key="partial_samples",
        phase_profile=False,
    ),
    "profile-partial-1bit": BenchmarkMode(
        cli_name="profile-partial-1bit",
        benchmark_name="partial_1bit",
        phase="partial_1bit_profile",
        display_mode="1bit",
        kind="partial",
        power_state="warm_held_on",
        numbered_patterns=True,
        purpose="phase_profile",
        samples_key="partial_profile_samples",
        phase_profile=True,
    ),
    "full-1bit": BenchmarkMode(
        cli_name="full-1bit",
        benchmark_name="full_1bit",
        phase="full_1bit_performance",
        display_mode="1bit",
        kind="full",
        power_state="cold_per_sample",
        numbered_patterns=True,
        purpose="performance",
        samples_key="full_performance_samples",
        phase_profile=False,
    ),
    "full-3bit": BenchmarkMode(
        cli_name="full-3bit",
        benchmark_name="full_3bit",
        phase="full_3bit_performance",
        display_mode="3bit",
        kind="full",
        power_state="cold_per_sample",
        numbered_patterns=True,
        purpose="performance",
        samples_key="full_performance_samples",
        phase_profile=False,
    ),
    "profile-full-1bit": BenchmarkMode(
        cli_name="profile-full-1bit",
        benchmark_name="full_1bit",
        phase="full_1bit_profile",
        display_mode="1bit",
        kind="full",
        power_state="cold_per_sample",
        numbered_patterns=True,
        purpose="phase_profile",
        samples_key="full_profile_samples",
        phase_profile=True,
    ),
    "profile-full-3bit": BenchmarkMode(
        cli_name="profile-full-3bit",
        benchmark_name="full_3bit",
        phase="full_3bit_profile",
        display_mode="3bit",
        kind="full",
        power_state="cold_per_sample",
        numbered_patterns=True,
        purpose="phase_profile",
        samples_key="full_profile_samples",
        phase_profile=True,
    ),
}


@dataclass(frozen=True)
class FullPhaseTiming:
    preparation_us: int
    power_on_us: int
    waveform_us: int
    initial_clean_us: int
    framebuffer_us: int
    settle_us: int
    final_clean_us: int
    terminal_vscan_us: int
    finalization_us: int
    previous_copy_us: int
    instrumented_total_us: int


@dataclass(frozen=True)
class PartialPhaseTiming:
    preparation_us: int
    row_discovery_us: int | None
    transition_prepare_us: int | None
    power_on_us: int
    transition_scan_us: int
    cleanup_us: int
    finalization_us: int
    previous_copy_us: int
    instrumented_total_us: int


@dataclass(frozen=True)
class Sample:
    index: int
    elapsed_us: int
    start_interval_us: int
    phases: FullPhaseTiming | PartialPhaseTiming | None


@dataclass(frozen=True)
class Summary:
    implementation: str
    samples: tuple[Sample, ...]

    @property
    def durations_ms(self) -> tuple[float, ...]:
        return tuple(sample.elapsed_us / 1_000 for sample in self.samples)

    @property
    def half_run_drift_ms(self) -> float:
        midpoint = len(self.samples) // 2
        first = self.durations_ms[:midpoint]
        second = self.durations_ms[midpoint:]
        return statistics.fmean(second) - statistics.fmean(first)

    def phase_mean_ms(self, field: str) -> float:
        values = [getattr(sample.phases, field) for sample in self.samples if sample.phases]
        if len(values) != len(self.samples):
            raise ValueError(f"{self.implementation}: phase data is incomplete")
        return statistics.fmean(values) / 1_000

    def optional_phase_mean_ms(self, field: str) -> float | None:
        values = [getattr(sample.phases, field) for sample in self.samples if sample.phases]
        present = [value for value in values if value is not None]
        if not present:
            return None
        if len(present) != len(values):
            raise ValueError(f"{self.implementation}: phase field {field} is incomplete")
        return statistics.fmean(present) / 1_000


def parse_fields(line: str, prefix: str) -> dict[str, str] | None:
    prefix_index = line.find(prefix)
    if prefix_index < 0:
        return None
    fields: dict[str, str] = {}
    for token in line[prefix_index + len(prefix) :].strip().split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def require_single(
    records: list[dict[str, str]], predicate: dict[str, str], label: str
) -> dict[str, str]:
    matches = [
        record
        for record in records
        if all(record.get(key) == value for key, value in predicate.items())
    ]
    if len(matches) != 1:
        raise ValueError(f"{label}: expected one record, found {len(matches)}")
    return matches[0]


def load_records(path: Path, prefix: str) -> list[dict[str, str]]:
    records = [
        fields
        for line in path.read_text(errors="replace").splitlines()
        if (fields := parse_fields(line, prefix)) is not None
    ]
    if not records:
        raise ValueError(f"{path}: no {prefix.strip()} records")
    return records


def fnv1a(bytes_: bytes | bytearray) -> int:
    hash_ = 0x811C9DC5
    for byte in bytes_:
        hash_ = ((hash_ ^ byte) * 0x01000193) & 0xFFFFFFFF
    return hash_


def gray_level(spec: dict[str, object], x: int, y: int) -> int:
    width = int(spec["width"])
    band_height = int(spec["gray_band_height"])
    if y < band_height:
        return min(x * 8 // width, 7) * 2
    if y < band_height * 2:
        return x * 15 // (width - 1)
    tile = int(spec["gray_checker_tile"])
    checker = ((x // tile) + ((y - band_height * 2) // tile)) & 1
    if x < width // 2:
        return 0 if checker == 0 else 15
    return 6 if checker == 0 else 8


def set_gray_pixel(framebuffer: bytearray, width: int, x: int, y: int, level: int) -> None:
    framebuffer_x = width - 1 - y
    framebuffer_y = x
    index = framebuffer_y * (width // 2) + framebuffer_x // 2
    if framebuffer_x & 1 == 0:
        framebuffer[index] = (framebuffer[index] & 0x0F) | (level << 4)
    else:
        framebuffer[index] = (framebuffer[index] & 0xF0) | level


def fill_gray_rect(
    framebuffer: bytearray,
    width: int,
    x: int,
    y: int,
    rect_width: int,
    rect_height: int,
    level: int,
) -> None:
    for row in range(y, y + rect_height):
        for column in range(x, x + rect_width):
            set_gray_pixel(framebuffer, width, column, row, level)


def numbered_gray_hashes(spec: dict[str, object], samples: int) -> dict[int, int]:
    width = int(spec["width"])
    height = int(spec["height"])
    framebuffer = bytearray(width * height // 2)
    row_bytes = width // 2
    for index in range(len(framebuffer)):
        y = index // row_bytes
        x = (index % row_bytes) * 2
        framebuffer[index] = (gray_level(spec, y, height - 1 - x) << 4) | gray_level(
            spec, y, height - 2 - x
        )
    base_hash = fnv1a(framebuffer)
    expected_base_hash = int(spec["gray_expected_fnv1a"])
    if base_hash != expected_base_hash:
        raise ValueError(
            f"shared grayscale fixture hash 0x{base_hash:08x}, expected 0x{expected_base_hash:08x}"
        )

    digit_width = int(spec["digit_width"])
    digit_height = int(spec["digit_height"])
    marker_scale = int(spec["marker_scale"])
    marker_digits = int(spec["marker_digits"])
    marker_padding = int(spec["marker_padding"])
    digit_advance = (digit_width + 1) * marker_scale
    content_width = marker_digits * digit_advance - marker_scale
    content_height = digit_height * marker_scale
    badge_width = content_width + marker_padding * 2
    badge_height = content_height + marker_padding * 2
    badge_x = (width - badge_width) // 2
    badge_y = (height - badge_height) // 2
    digit_rows = spec["digit_rows"]

    hashes: dict[int, int] = {}
    for sample in range(samples + 1):
        numbered = bytearray(framebuffer)
        fill_gray_rect(numbered, width, badge_x, badge_y, badge_width, badge_height, 15)
        fill_gray_rect(numbered, width, badge_x, badge_y, badge_width, 1, 0)
        fill_gray_rect(
            numbered, width, badge_x, badge_y + badge_height - 1, badge_width, 1, 0
        )
        fill_gray_rect(numbered, width, badge_x, badge_y, 1, badge_height, 0)
        fill_gray_rect(
            numbered, width, badge_x + badge_width - 1, badge_y, 1, badge_height, 0
        )
        marker = sample % 1_000
        digits = (marker // 100, (marker // 10) % 10, marker % 10)
        for digit_index, digit in enumerate(digits):
            origin_x = badge_x + marker_padding + digit_index * digit_advance
            origin_y = badge_y + marker_padding
            for row in range(digit_height):
                bits = int(digit_rows[digit][row])
                for column in range(digit_width):
                    if bits & (1 << (digit_width - 1 - column)):
                        fill_gray_rect(
                            numbered,
                            width,
                            origin_x + column * marker_scale,
                            origin_y + row * marker_scale,
                            marker_scale,
                            marker_scale,
                            0,
                        )
        hashes[sample] = fnv1a(numbered)
    return hashes


def validate_capture(
    path: Path,
    prefix: str,
    implementation: str,
    mode: BenchmarkMode,
    expected_samples: int,
    expected_interval_us: int,
    expected_cpu_hz: int,
    expected_spec_version: int,
    expected_gray_hashes: dict[int, int],
) -> Summary:
    records = load_records(path, prefix)
    if any(record.get("event") == "halt" for record in records):
        raise ValueError(f"{implementation}: capture contains a halt record")
    boot = require_single(records, {"event": "boot"}, f"{implementation} boot")
    if int(boot["spec_version"]) != expected_spec_version:
        raise ValueError(
            f"{implementation}: spec_version={boot['spec_version']}, expected {expected_spec_version}"
        )
    if int(boot["cpu_hz"]) != expected_cpu_hz:
        raise ValueError(
            f"{implementation}: cpu_hz={boot['cpu_hz']}, expected {expected_cpu_hz}"
        )

    start = require_single(
        records,
        {"event": "benchmark_start", "benchmark": mode.benchmark_name},
        f"{implementation} benchmark start",
    )
    expected_start = {
        "purpose": mode.purpose,
        "samples": str(expected_samples),
        "warmup_samples": "1",
        "power_state": mode.power_state,
        "refresh_end_to_start_interval_ms": str(expected_interval_us // 1_000),
        "draw_timed": "0",
    }
    for key, expected in expected_start.items():
        if start.get(key) != expected:
            raise ValueError(
                f"{implementation}: benchmark {key}={start.get(key)!r}, expected {expected!r}"
            )
    expected_instrumentation = "phase_timestamps" if mode.phase_profile else "none"
    if start.get("instrumentation", "none") != expected_instrumentation:
        raise ValueError(
            f"{implementation}: benchmark instrumentation={start.get('instrumentation')!r}, "
            f"expected {expected_instrumentation!r}"
        )
    if implementation == "Our driver" and mode.display_mode == "1bit":
        ready = require_single(
            records,
            {"event": "ready", "benchmark": mode.benchmark_name},
            f"{implementation} ready",
        )
        phase_timing_key = f"{mode.kind}_scan_phase_timing"
        expected_phase_timing = "true" if mode.phase_profile else "false"
        if ready.get(phase_timing_key) != expected_phase_timing:
            raise ValueError(
                f"{implementation}: ready {phase_timing_key}="
                f"{ready.get(phase_timing_key)!r}, expected {expected_phase_timing!r}"
            )

    complete = require_single(
        records,
        {
            "event": "complete",
            "benchmark": mode.benchmark_name,
            "purpose": mode.purpose,
            "protocol": "passed",
        },
        f"{implementation} completion",
    )
    if complete.get("samples", complete.get("cycles")) != str(expected_samples):
        raise ValueError(f"{implementation}: completion sample count does not match contract")

    refreshes = [
        record
        for record in records
        if record.get("event") == "refresh"
        and record.get("phase") == mode.phase
        and record.get("kind") == mode.kind
    ]
    if mode.phase_profile and implementation == "Reference":
        phase_records = [
            record
            for record in records
            if record.get("event") == "phases"
            and record.get("phase") == mode.phase
            and record.get("kind") == mode.kind
        ]
    elif mode.phase_profile:
        phase_records = refreshes
    else:
        phase_records = []

    phases_by_sample: dict[int, dict[str, str]] = {}
    for record in phase_records:
        index = int(record["sample"])
        if index in phases_by_sample:
            raise ValueError(f"{implementation}: duplicate phase sample {index}")
        phases_by_sample[index] = record

    samples: list[Sample] = []
    for expected_index, record in enumerate(refreshes, start=1):
        actual_index = int(record["sample"])
        if actual_index != expected_index:
            raise ValueError(
                f"{implementation}: sample sequence expected {expected_index}, got {actual_index}"
            )
        expected_leave_on = {"1", "true"} if mode.kind == "partial" else {"0", "false"}
        if record.get("leave_on") not in expected_leave_on:
            raise ValueError(
                f"{implementation}: sample {actual_index} leave_on={record.get('leave_on')!r}"
            )
        if mode.numbered_patterns:
            marker = int(record["marker"])
            if marker != actual_index % 1_000:
                raise ValueError(
                    f"{implementation}: sample {actual_index} marker={marker:03}, expected {actual_index % 1_000:03}"
                )
            if implementation == "Reference" and mode.display_mode == "1bit":
                expected_pattern = (
                    "vertical_50px" if actual_index % 2 == 0 else "horizontal_50px"
                )
                if record.get("pattern") != expected_pattern:
                    raise ValueError(
                        f"{implementation}: sample {actual_index} pattern={record.get('pattern')!r}, expected {expected_pattern!r}"
                    )
        if mode.display_mode == "3bit":
            if record.get("display_mode") != mode.display_mode:
                raise ValueError(
                    f"{implementation}: sample {actual_index} display mode mismatch"
                )
            if int(record["pattern_hash"], 16) != expected_gray_hashes[actual_index]:
                raise ValueError(f"{implementation}: sample {actual_index} pattern hash mismatch")

        start_interval_us = int(record["start_interval_us"])
        if start_interval_us < expected_interval_us:
            raise ValueError(
                f"{implementation}: sample {actual_index} cadence {start_interval_us} us is below {expected_interval_us} us"
            )
        elapsed_us = int(record["elapsed_us"])
        if elapsed_us <= 0:
            raise ValueError(
                f"{implementation}: sample {actual_index} has invalid elapsed_us={elapsed_us}"
            )
        phases = None
        if mode.phase_profile:
            phase_record = phases_by_sample.get(actual_index)
            if phase_record is None:
                raise ValueError(
                    f"{implementation}: sample {actual_index} has no phase record"
                )
            if mode.kind == "full":
                phase_values = {
                    key: int(phase_record[key])
                    for key in (
                        "preparation_us",
                        "power_on_us",
                        "waveform_us",
                        "initial_clean_us",
                        "framebuffer_us",
                        "settle_us",
                        "final_clean_us",
                        "terminal_vscan_us",
                        "finalization_us",
                        "previous_copy_us",
                        "instrumented_total_us",
                    )
                }
                component_waveform = sum(
                    phase_values[key]
                    for key in (
                        "initial_clean_us",
                        "framebuffer_us",
                        "settle_us",
                        "final_clean_us",
                        "terminal_vscan_us",
                    )
                )
                if phase_values["waveform_us"] != component_waveform:
                    raise ValueError(
                        f"{implementation}: sample {actual_index} waveform components do not sum"
                    )
                accounted = (
                    phase_values["preparation_us"]
                    + phase_values["power_on_us"]
                    + phase_values["waveform_us"]
                    + phase_values["finalization_us"]
                    + phase_values["previous_copy_us"]
                )
                phases = FullPhaseTiming(**phase_values)
            else:
                phase_values = {
                    key: int(phase_record[key])
                    for key in (
                        "preparation_us",
                        "power_on_us",
                        "transition_scan_us",
                        "cleanup_us",
                        "finalization_us",
                        "previous_copy_us",
                        "instrumented_total_us",
                    )
                }
                optional_values = {
                    key: int(phase_record[key]) if key in phase_record else None
                    for key in ("row_discovery_us", "transition_prepare_us")
                }
                accounted = (
                    phase_values["preparation_us"]
                    + phase_values["power_on_us"]
                    + phase_values["transition_scan_us"]
                    + phase_values["cleanup_us"]
                    + phase_values["finalization_us"]
                    + phase_values["previous_copy_us"]
                )
                phases = PartialPhaseTiming(**phase_values, **optional_values)
            if accounted > phase_values["instrumented_total_us"]:
                raise ValueError(
                    f"{implementation}: sample {actual_index} phase total exceeds instrumented total"
                )

        samples.append(
            Sample(
                index=actual_index,
                elapsed_us=elapsed_us,
                start_interval_us=start_interval_us,
                phases=phases,
            )
        )

    if len(samples) != expected_samples:
        raise ValueError(
            f"{implementation}: found {len(samples)} samples, expected {expected_samples}"
        )
    return Summary(implementation=implementation, samples=tuple(samples))


def format_row(summary: Summary) -> str:
    values = summary.durations_ms
    return (
        f"| {summary.implementation} | {len(values)} | {statistics.fmean(values):.3f} ms "
        f"| {statistics.stdev(values):.3f} ms | {statistics.median(values):.3f} ms "
        f"| {min(values):.3f} ms | {max(values):.3f} ms |"
    )


def print_full_phase_table(reference: Summary, panel: Summary) -> None:
    rows = (
        ("Preparation / previous-frame copy", "bookkeeping"),
        ("Power on", "power_on_us"),
        ("Initial cleaning", "initial_clean_us"),
        ("Framebuffer passes", "framebuffer_us"),
        ("Settle pass", "settle_us"),
        ("Final cleaning", "final_clean_us"),
        ("Terminal vertical scan", "terminal_vscan_us"),
        ("Waveform / scan subtotal", "waveform_us"),
        ("Power off / finalization", "finalization_us"),
        ("Timing residual", "residual"),
        ("Transaction total", "total"),
    )
    print("| Phase | Reference mean | Our driver mean | Difference |")
    print("|---|---:|---:|---:|")
    for label, field in rows:
        if field == "bookkeeping":
            reference_ms = reference.phase_mean_ms("preparation_us") + reference.phase_mean_ms(
                "previous_copy_us"
            )
            panel_ms = panel.phase_mean_ms("preparation_us") + panel.phase_mean_ms(
                "previous_copy_us"
            )
        elif field == "residual":
            reference_ms = statistics.fmean(
                (sample.elapsed_us - sample.phases.instrumented_total_us) / 1_000
                for sample in reference.samples
                if sample.phases
            )
            panel_ms = statistics.fmean(
                (sample.elapsed_us - sample.phases.instrumented_total_us) / 1_000
                for sample in panel.samples
                if sample.phases
            )
        elif field == "total":
            reference_ms = statistics.fmean(reference.durations_ms)
            panel_ms = statistics.fmean(panel.durations_ms)
        else:
            reference_ms = reference.phase_mean_ms(field)
            panel_ms = panel.phase_mean_ms(field)
        print(
            f"| {label} | {reference_ms:.3f} ms | {panel_ms:.3f} ms "
            f"| {panel_ms - reference_ms:+.3f} ms |"
        )


def format_optional_ms(value: float | None) -> str:
    return "not separated" if value is None else f"{value:.3f} ms"


def print_partial_phase_table(reference: Summary, panel: Summary) -> None:
    def residual(summary: Summary) -> float:
        return statistics.fmean(
            (sample.elapsed_us - sample.phases.instrumented_total_us) / 1_000
            for sample in summary.samples
            if sample.phases
        )

    rows = (
        ("Preparation", "preparation_us"),
        ("Power on / already-on check", "power_on_us"),
        ("Nine transition scans", "transition_scan_us"),
        ("Cleanup / gate drain", "cleanup_us"),
        ("Finalization", "finalization_us"),
        ("Previous-frame copy", "previous_copy_us"),
    )
    print("| Transaction phase | Reference mean | Our driver mean | Difference |")
    print("|---|---:|---:|---:|")
    for label, field in rows:
        reference_ms = reference.phase_mean_ms(field)
        panel_ms = panel.phase_mean_ms(field)
        print(
            f"| {label} | {reference_ms:.3f} ms | {panel_ms:.3f} ms "
            f"| {panel_ms - reference_ms:+.3f} ms |"
        )
    reference_waveform = reference.phase_mean_ms("transition_scan_us") + reference.phase_mean_ms(
        "cleanup_us"
    )
    panel_waveform = panel.phase_mean_ms("transition_scan_us") + panel.phase_mean_ms("cleanup_us")
    print(
        f"| Waveform / scan subtotal | {reference_waveform:.3f} ms | {panel_waveform:.3f} ms "
        f"| {panel_waveform - reference_waveform:+.3f} ms |"
    )
    reference_residual = residual(reference)
    panel_residual = residual(panel)
    print(
        f"| Timing residual | {reference_residual:.3f} ms | {panel_residual:.3f} ms "
        f"| {panel_residual - reference_residual:+.3f} ms |"
    )
    reference_total = statistics.fmean(reference.durations_ms)
    panel_total = statistics.fmean(panel.durations_ms)
    print(
        f"| Transaction total | {reference_total:.3f} ms | {panel_total:.3f} ms "
        f"| {panel_total - reference_total:+.3f} ms |"
    )

    print()
    print("Preparation breakdown:")
    print()
    print("| Preparation phase | Reference mean | Our driver mean |")
    print("|---|---:|---:|")
    for label, field in (
        ("Changed-row discovery", "row_discovery_us"),
        ("Transition preparation", "transition_prepare_us"),
    ):
        print(
            f"| {label} | {format_optional_ms(reference.optional_phase_mean_ms(field))} "
            f"| {format_optional_ms(panel.optional_phase_mean_ms(field))} |"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("reference_capture", type=Path)
    parser.add_argument("panel_capture", type=Path)
    args = parser.parse_args()

    with SPEC_PATH.open("rb") as source:
        spec = tomllib.load(source)
    expected_interval_us = int(spec["refresh_interval_ms"]) * 1_000
    expected_cpu_hz = int(spec["cpu_hz"])
    expected_spec_version = int(spec["version"])
    mode = MODES[args.mode]
    expected_samples = int(spec[mode.samples_key])
    expected_gray_hashes = numbered_gray_hashes(spec, expected_samples)

    try:
        reference = validate_capture(
            args.reference_capture,
            "REFERENCE_REFRESH ",
            "Reference",
            mode,
            expected_samples,
            expected_interval_us,
            expected_cpu_hz,
            expected_spec_version,
            expected_gray_hashes,
        )
        panel = validate_capture(
            args.panel_capture,
            "PANEL_SOAK ",
            "Our driver",
            mode,
            expected_samples,
            expected_interval_us,
            expected_cpu_hz,
            expected_spec_version,
            expected_gray_hashes,
        )
    except (KeyError, ValueError) as error:
        print(f"refresh benchmark: FAIL: {error}", file=sys.stderr)
        return 1

    reference_mean = statistics.fmean(reference.durations_ms)
    panel_mean = statistics.fmean(panel.durations_ms)
    delta_ms = panel_mean - reference_mean
    delta_percent = delta_ms / reference_mean * 100
    print(f"refresh {mode.purpose.replace('_', ' ')}: {args.mode} (validated)")
    print()
    print("| Implementation | Samples | Mean | Sample std dev | Median | Min | Max |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    print(format_row(reference))
    print(format_row(panel))
    print()
    print(f"Our driver minus reference: {delta_ms:+.3f} ms ({delta_percent:+.2f}%).")
    print(
        "Half-run drift (second half minus first): "
        f"reference {reference.half_run_drift_ms:+.3f} ms; "
        f"our driver {panel.half_run_drift_ms:+.3f} ms."
    )
    if mode.phase_profile:
        print()
        if mode.kind == "partial":
            print_partial_phase_table(reference, panel)
        else:
            print_full_phase_table(reference, panel)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
