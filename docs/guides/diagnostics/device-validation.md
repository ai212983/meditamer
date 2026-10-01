# Device validation

Use an identified flashed image, the intended `$DEVICE_PORT`, and one serial
owner. [Build/flash](../development/build-and-flash.md) owns installation;
[serial control](../../references/runtime/serial-control.md) and
[metrics](../../references/runtime/metrics.md) own commands and interpretation.
Record the command/overrides, board, artifact identity, outcome and raw evidence.

## Service and telemetry smoke

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test runtime-modes-smoke --suite full
```

This toggles service modes and takes PSRAM snapshots without an extra settle delay.
For manual checks, query PSRAM before upload-on, after upload-on, and after
off. Use `STATE GET`/`DIAG GET` to verify application and diagnostic state.

Before a logging-sensitive run, query TELEM and save the mask. Clear with
`TELEMSET NONE`, enable needed domains, and query again. Wi-Fi bring-up uses
WIFI/NET; reassociation adds REASSOC; HTTP investigations use HTTP/NET/SD.
Capture METRICSNET/METRICS independently of logging. On success or failure,
clear the mask, enable each originally enabled domain, then verify restoration.

To launch Analog Clock, send plain UISTEP from Home to reach Launcher, then:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh repaint --command 'UISTEP ANALOG_CLOCK'
```

For a flash followed by launch, use
`HOSTCTL_FLASH_CAPTURE_POST_COMMAND='UISTEP ANALOG_CLOCK'` and
`HOSTCTL_FLASH_CAPTURE_POST_PATTERN='^UISTEP OK'` on the same connection.

## SD hardware test

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test sdcard-hw --suite all
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test sdcard-burst-regression
```

Runs current firmware without flashing. `all` covers FAT operations/readback,
burst backpressure and failure paths, using `SDREQ` plus correlated `SDWAIT`
completion. `--suite` also accepts `baseline`, `burst`, `failures`, `cutover`, and
`no-card`; no-card performs 20 bounded absent-card probes plus resource/liveness
gates and does not establish FAT correctness or throughput.

`--output` selects the summary path. Flash the candidate separately; this workflow
does not flash. The recipe fixes verify LBA 2048, derives `/sd<run-tag>` paths,
and uses a 300-second SDWAIT timeout.

## Boot and refresh soaks

```bash
ESPFLASH_PORT="$DEVICE_PORT" scripts/device/soak_boot.sh 10
ESPFLASH_PORT="$DEVICE_PORT" scripts/device/cold_boot_matrix.sh 20
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh panel-soak --cycles 20 --output logs/<new-panel-run>
```

Panel soak requires completed repaint acknowledgements for each requested cycle;
use `--wait-partial` when the run must also observe partial-refresh completion.
Inspect the physical panel separately for visual defects.

The matrix is a manual reset-button helper; it does not disconnect battery-backed
power and cannot prove true power-rail cold boot. Preserve complete boot captures
for startup I²C failures; recovered samples do not establish their cause.

Boot soak controls: `SOAK_WINDOW_SEC` (`8`), `SOAK_LOG_DIR`,
`SOAK_MONITOR_BEFORE`/`SOAK_MONITOR_AFTER`, and `SOAK_REQUIRE_UPTIME=1` to require
an uptime-screen marker. Matrix controls: `COLD_BOOT_WINDOW_SEC` (`45`),
`COLD_BOOT_CONNECT_TIMEOUT_SEC` (`40`), and `COLD_BOOT_LOG_DIR`.

## UI lifecycle

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test ui-lifecycle \
  --cycles 2 --output logs/<new-ui-run>.log
```

Attach to a ready board with upload off. The workflow completes a correlated
repaint and cycles Home → Launcher → Diagnostics → Home; the initial screen may
be Ambient. It requires panel acknowledgements, route/count coverage, settled
LVGL/heap integrity, plateaued high-water, and no refresh/runtime faults.
Timeouts are never retried because navigation outcome is ambiguous.

Baseline tolerance defaults to zero. Set `--max-baseline-drift-bytes` only to a
bound predeclared by an identified characterization run; do not copy a historical
bound onto a changed image. UART and sibling JSON are resource evidence.
Observe the physical panel and touch before closing a UI qualification gate.

## Scheduling and physical input

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh multicore-qualify \
  --output logs/<new-scheduling-run>
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh multicore-qualify \
  --physical-input --output logs/<new-physical-run>
```

The workflow retains one connection, captures startup, settles for 65 seconds,
and checks uptime before exercising interactive/upload/diagnostics. It checks
PING during a metrics dump and restores AUTO on success or failure. Display
updates remain enabled. Complete fresh metrics are required: BUSY/ERR or a
missing terminal record fails capture. Control has a 2-second deadline; bulk
dumps have a separate 10-second completion timeout.

Physical mode creates READY, then waits up to ten minutes for START in the output
directory; STOP cancels the wait. Keep the process/connection open, coordinate
with the operator, then create START. Each profile receives a one-minute window
and requires nonzero touch acquisition/delivery plus minimum active IMU coverage.
Alternate touch with hands-off pauses so suppression permits IMU sampling.

Budget failures are recorded while remaining profiles continue; the final result
still fails. Transport/control/liveness failures stop capture and preserve the
post-failure reset watch. Zero-touch runs are hands-off evidence, not physical
qualification; counters also require a functional observation.
