# Telemetry Feasibility (ESP32 + Embassy + `no_std`)

## Scope
Evaluate whether we should introduce metrics/telemetry in firmware, and which libraries fit our constraints (RAM pressure, `no_std`, UART-first debugging, optional Wi-Fi upload mode).

## Current Baseline
- We already have ad-hoc telemetry over UART:
  - `METRICS` / `PERF` command in `src/firmware/runtime/serial_task.rs` (currently marble redraw timings only).
  - Structured CSV-like traces (`tap_trace`, `touch_trace`, `touch_event`, SD task lines, upload logs).
- This is useful but fragmented. There is no centralized schema/counter registry.

## Feasibility Verdict
Feasible, with a staged approach.

- On-device metrics and event telemetry are feasible with very low overhead.
- Full OpenTelemetry/Prometheus exporters on-device are not a good fit for this firmware profile.
- Best path: lightweight in-firmware counters + compact event records, exported over existing channels (UART now, optional HTTP endpoint later).

## Library Fit

### Good fit
- `defmt` + `defmt-serial`
  - Compact logging format for constrained targets.
  - Works over serial transport, which matches current workflow.
  - Crates:
    - https://crates.io/crates/defmt
    - https://crates.io/crates/defmt-serial

- `postcard`
  - `no_std` binary serialization for compact telemetry/event packets.
  - Useful if we want machine-readable telemetry records instead of plain text.
  - Crate: https://crates.io/crates/postcard

- `serde-json-core` (optional)
  - `no_std` JSON encode/decode if human-readable structured JSON is needed.
  - Higher overhead than binary (`postcard`), but easier for ad-hoc host tooling.
  - Crate: https://crates.io/crates/serde-json-core

### Possible but lower priority
- `tracing`
  - Core crate supports `no_std`, but practical subscriber/export pipeline is usually `std`-heavy.
  - Better suited for host services than this firmware runtime.
  - Crate: https://crates.io/crates/tracing

### Poor fit for on-device runtime (for now)
- `opentelemetry`
- `metrics` (`metrics-rs` ecosystem)
- `prometheus-client`

These are strong for servers/desktop targets, but for this firmware they add complexity and memory/runtime cost without clear immediate payoff.

## Recommended Architecture

### Phase 1 (recommended now)
1. Add a tiny `telemetry` module in firmware:
   - Atomic counters for key reliability points (wifi connect attempts/failures, `sd_busy`, `power_on_failed`, upload begin/chunk/commit failures, mode transitions).
   - Small fixed histograms/buckets for latency (`connect_ms`, `listen_ms`, upload durations).
2. Expose a single UART command response (`METRICS`) with stable key-value lines.
3. Keep event traces as-is, but normalize labels and field names.

### Phase 2
1. Add optional structured export format:
   - `postcard` packets over UART for machine parsing.
   - Keep text output for human debugging.
2. Add resettable metrics windows (`METRICS RESET`) and boot/session identifiers.

### Phase 3 (optional)
1. Add HTTP read-only endpoint in upload mode (for example `/metrics`) that dumps current counters.
2. Keep this endpoint strictly pull-based and lightweight.

## Why this approach
- Maximizes observability per byte of RAM.
- Avoids hard dependency on Wi-Fi connectivity for diagnostics.
- Fits current runtime mode architecture and existing serial workflow.
- Keeps the door open for richer host-side telemetry processing later.
