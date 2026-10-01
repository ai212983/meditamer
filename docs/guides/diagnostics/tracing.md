# Firmware tracing

The opt-in `firmware-trace` feature captures bounded Rust `tracing` events for
offline analysis. `ui-interaction-trace` adds input and presentation hooks. These
diagnostic features do not establish scheduling qualification or fix missed input.

Build the input diagnostic image with:

```sh
FIRMWARE_BIN=meditamer CARGO_FEATURES=ui-interaction-trace scripts/build/build.sh release
```

Use the normal [build and flash workflow](../development/build-and-flash.md). `TRACE ARM`
prepares an empty recording that starts on the next physical touch contact or
WAKE press. The triggering input is included; background events do not consume
storage while armed. A contact already held when arming waits for a new contact.
`TRACE STATUS` reports `armed`, `capturing`, `frozen`, or `full`.

`TRACE STOP` freezes or disarms the recorder, and `TRACE DUMP` exports the frozen
records using reliable UART writes. `TRACE START` remains available for explicit
immediate recordings. Keep one serial connection open: attachment can reset this
board. When storage fills it freezes without overwriting earlier events;
`buffer_full=1` marks the boundary as incomplete evidence, not an input failure.

For an action-triggered capture:

```sh
scripts/hostctl.sh trace-capture --output logs/trace-check
```

The workflow retains one connection, settles it for 65 seconds, checks readiness,
and arms the recorder. `READY` means the device is armed; there is no countdown
or `START` file to coordinate. The next touch or WAKE press starts recording.
Create `STOP` in the output directory when finished; the workflow then freezes
and exports the recording. It also exports automatically if the buffer fills.
The action-mode polling loop has no elapsed-time or iteration expiry; transport
errors still end capture and preserve available evidence.
`RECORDING` marks an observed trigger and `FROZEN` marks a full buffer.
The output directory must be new so prior evidence is retained. The [capture YAML](../../../tools/hostctl/scenarios/trace-capture.sw.yaml) owns orchestration.

Before requesting physical reproduction after recorder changes, run:

```sh
scripts/hostctl.sh trace-capture --self-test --output logs/trace-self-test
```

This emits and verifies 16 known records from each CPU, including their core,
index, value and complement, after the connection settles. It requires the
initial capture marker and zero loss counters. It checks recorder integrity;
it does not exercise touch or establish UI responsiveness. `--self-test` and
the optional timed recording flag `--seconds` are mutually exclusive.

Convert the raw console log with:

```sh
python3 tools/trace_export/trace_export.py capture.log \
  --output trace.json --report trace.md
```

Open `trace.json` in Perfetto. The report includes explicit loss counters,
per-contact stages and measured queue/panel times. Exit status 0 means a complete
transport capture, 2 means exported but incomplete evidence, and 1 means malformed
or ambiguous input. It does not certify interaction coverage or panel appearance.

## Adding an event

Use constant names and targets, an explicit root, and numeric or boolean fields:

```rust
#[cfg(feature = "firmware-trace")]
tracing::event!(name: "work_done", target: "application", parent: None,
    tracing::Level::TRACE, flow_id = operation_id, elapsed_us = elapsed_us);
```

The collector retains at most six fields. Formatted messages, debug values and
excess fields are reported as losses. Use an explicit `flow_id` for causal work;
IDs must be unique across all instrumented subsystems within a capture. Reuse an
ID only when carrying the same operation across a queue or execution context.
Implicit span stacks cannot safely represent multiplexed async tasks and IRQs.
Span callsites are disabled. Names and field metadata must be static.

The [collector](../../../platform/diagnostics/firmware-trace/src/lib.rs) performs no
formatting, UART writes or allocation on the event path. It uses bounded PSRAM
and cross-core queues; overflow/integrity failures make capture incomplete.
Do not call tracing from cache-disabled code, waveform routines or CPU-accounting
hooks: upstream code/metadata are not an IRAM facility.

## Interpreting input evidence

Contact IDs survive release timers and reset cancellation. Acquisition frame IDs
also distinguish multitouch samples. Suppression, queue discards and native LVGL
press/click callbacks are separate stages. A callback entry does not prove that
its intent was accepted. A recorded pressed state proves software state only.

Direct input rendering can retain its contact ID. Deferred or combined work may
have no unique origin and must remain unattributed rather than inheriting the
last tap. A missing downstream stage alone cannot distinguish a capture boundary,
deferred work, suppression, or an actual failure. Panel completion is a software
acknowledgement, not an optical measurement; visual soaks remain separate.

Navigation closes input admission until the destination (or restored origin)
has been presented. Stages 36/37 mark that boundary; stage 38 records intentional
rejection of queued or transition-time input. A held contact remains rejected
until release. The [UI shell](../../../platform/ui/shell/src/lib.rs) owns admission;
physical panel/touch observation remains required.
