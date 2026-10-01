# Inkplate firmware fixtures

Diagnostic boot paths selected with Cargo features. Use the normal
[build/flash workflow](../development/build-and-flash.md) and restore a normal production image
after checking. These fixtures are disabled in production profiles.

## UI initialization

```bash
CARGO_FEATURES=ui-initialization-fixture scripts/build/build.sh release
```

Exercises the actual external display model and LVGL backend: rejects allocation
and initialization boundaries, checks cleanup, installed/stale callbacks and
reinitialization, then continues normal startup.

## SD runner allocation

```bash
CARGO_FEATURES=sd-runner-allocation-fixture scripts/build/build.sh release
```

Runs before normal SD startup. Rejects FAT/runner allocations and checks external
free bytes, unpolled-runner cleanup and absence of runner polling, then continues
normal startup.

## Panel waveform

```bash
CARGO_FEATURES=panel-waveform-fixture scripts/build/build.sh release minimal
```

Before product tasks start, allocates a 180,000-byte PSRAM framebuffer and renders
fixed level, ramp and boundary patterns. Logs separate power-on, waveform and
power-off durations, then parks for physical image inspection.

### Gray4 experiments

Gray4 keeps its own conservative baseline, independent of partial-refresh
timing. The [waveform source](../../../boards/inkplate-tempera/src/waveform.rs)
owns GPIO sequence and row-boundary semantics.

Run the [waveform placement gate](../../../scripts/ci/check_panel_waveform_placement.sh)
against the built ELF; it takes only an optional ELF path. Assembly shape is
manual qualification in
[check_scan_contract.sh](../../../tools/rendering/refresh-probe/check_scan_contract.sh).
Image quality remains separate evidence.

### Binary comparator

```bash
MEDITAMER_PANEL_BINARY_WAVEFORM_FIXTURE=1 CARGO_FEATURES=panel-waveform-fixture scripts/build/build.sh release minimal
```

Renders an independent fixed 1-bit comparator pattern. It uses the production
fixed-zero hardware source loop, reference hardware clean loop, immediate
reference clean/row-start/row-boundary edges and zero inter-pass delay. Reference
pass counts and two-level `LUTB`/`LUT2` translation remain intact.
Independent partial timing is not linked into this full-only image.

### Standalone refresh probe

The [refresh-probe workflow](../../../tools/rendering/refresh-probe/DEVELOPMENT.md)
owns build and measurement commands. Panel probe builds use production timing;
only the `profile-*` modes add phase attribution. The [board waveform source](../../../boards/inkplate-tempera/src/waveform.rs)
owns timing semantics. Assembly shape is manual qualification in
[check_scan_contract.sh](../../../tools/rendering/refresh-probe/check_scan_contract.sh);
no probe variant implies production qualification.

## CPU load bench

```bash
FIRMWARE_BIN=cpu-load-probe CARGO_FEATURES=cpu-load-probe \
  scripts/build/build.sh release minimal
```

The image exercises idle, each core busy, both half/full busy, and interrupt-only
work. Restore production afterward. When changing instrumentation, run IRAM/stack
guards and network regression; IRQ hooks/counters must stay in internal memory.
[CPU metrics](../../references/runtime/metrics.md#cpu-load-inkplate) defines readings.
