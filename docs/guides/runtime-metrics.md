# Runtime Metrics and Serial Diagnostics

## Baseline Runtime Periods

- LVGL service period: 8 ms, with panel refreshes driven by accumulated dirty areas.
- Battery task: independent Embassy task every 5 minutes (`300s`).
- Battery percentage source: BQ27441 fuel-gauge `SoC` register.

## Runtime Metrics

Runtime metrics are available over `UART0` (`115200` baud):

```text
METRICS
```

Response lines:

```text
METRICS WIFI attempt=<n> success=<n> failure=<n> no_ap=<n> scan_runs=<n> scan_empty=<n> scan_hits=<n>
METRICS WIFI_LINK rssi_last_dbm=<n> rssi_min_dbm=<n> rssi_max_dbm=<n> rssi_samples=<n> rssi_low_samples=<n>
METRICS UPLOAD accept_ok=<n> accept_err=<n> request_err=<n> req_hdr_to=<n> req_read_body=<n> req_read_body_reset=<n> req_sd_busy=<n> sd_errors=<n> sd_busy=<n> sd_timeouts=<n> sd_power_on_fail=<n> sd_init_fail=<n> sess_timeout_abort=<n> sess_mode_off_abort=<n>
METRICS UPLOAD_PHASE req=<n> bytes=<n> body_ms=<n> body_max=<n> sd_ms=<n> sd_max=<n> req_ms=<n> req_max=<n>
METRICS UPLOAD_DECOMP copy_ms=<n> copy_max=<n> sdq_ms=<n> sdq_max=<n> sdtask_ms=<n> sdtask_max=<n> commit_ms=<n> commit_max=<n> chunk_p50_max=<n> chunk_p95_max=<n> chunk_max=<n> chunk_samples=<n> chunk_drop=<n>
METRICS UPLOAD_RTT begin_n=<n> begin_ms=<n> begin_max=<n> chunk_n=<n> chunk_ms=<n> chunk_max=<n> commit_n=<n> commit_ms=<n> commit_max=<n> abort_n=<n> abort_ms=<n> abort_max=<n> mkdir_n=<n> mkdir_ms=<n> mkdir_max=<n> rm_n=<n> rm_ms=<n> rm_max=<n>
METRICS NET wifi_connected=<0|1> http_listening=<0|1> ip=<a.b.c.d>
METRICS NET_PIPELINE dhcp_wait_n=<n> dhcp_wait_ms=<n> dhcp_wait_ms_max=<n> dhcp_ready=<n> gate_wifi_down=<n> gate_link_down=<n> gate_no_ipv4=<n> listener_on=<n> listener_off=<n> accept_wait_n=<n> accept_wait_ms=<n> accept_wait_ms_max=<n>
METRICS NET_ACCEPT arm_gap_n=<n> arm_gap_us=<n> arm_gap_us_max=<n> arm_gap_after_mkdir_n=<n> arm_gap_after_mkdir_us=<n> arm_gap_after_mkdir_us_max=<n>
```

`UPLOAD_PHASE` reports end-to-end per-request timing buckets for upload body handling.
`UPLOAD_DECOMP` reports phase decomposition (payload copy, SD queue/send, SD task wait, commit) plus bounded per-request chunk latency summary maxima.
`UPLOAD_RTT` reports SD roundtrip counts and timing totals/maxima by command phase.

### Runtime Scheduling Profiles

Embassy task priorities are selected from a centralized behavior profile. In automatic mode the
runtime chooses `interactive`, `upload`, or `diagnostics` from the current app state. Diagnostics
has precedence over upload.

Use the serial control to inspect or temporarily override the automatic choice:

```text
SCHEDPROFILE
SCHEDPROFILE AUTO
SCHEDPROFILE INTERACTIVE
SCHEDPROFILE UPLOAD
SCHEDPROFILE DIAGNOSTICS
```

The response reports the active and automatically selected profiles, the volatile override, and
runtime-readiness state. `AUTO` removes the override; overrides are intentionally not persisted
across reboot.

Profile policy is defined in `src/firmware/runtime/scheduling.rs`. Touch acquisition runs alone on
the core-1 Embassy executor; its core assignment is fixed across profiles. Touch processing remains
the highest-priority input task on core 0. Upload balances Wi-Fi, network, HTTP, and SD workers at
the same priority so no pipeline stage can starve another. Diagnostics prioritizes serial control
and diagnostics work. Core-0 priorities control executor polling; they do not interrupt an
already-running synchronous section or provide priority inheritance for a shared peripheral lock.

### Runtime Telemetry Domain Control

Use runtime telemetry domain toggles to reduce log pressure without reflashing.

```text
TELEM
TELEMSET NONE
TELEMSET WIFI ON
TELEMSET NET ON
TELEMSET REASSOC ON
```

- `TELEM` returns current domain mask/status.
- `TELEMSET` updates enabled domains (`WIFI`, `REASSOC`, `NET`, `HTTP`, `SD`, `ALL`, `DEFAULT`, `NONE`).
- `METRICS` / `METRICSNET` remain available regardless of telemetry domain settings.

Agent-oriented contract and runbook:

- `docs/guides/agents/telemetry-control.md`

## SD Card Hardware Test

Automated UART-driven SD/FAT end-to-end validation:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_hw.sh
```

Defaults:

- uses current flashed firmware (does **not** flash by default)
- captures monitor log under `logs/`
- default suite (`HOSTCTL_SDCARD_SUITE=all`) verifies:
  - baseline flow: `SDPROBE`, FAT mkdir/write/read/append/stat/truncate/rename/remove, and `SDRWVERIFY`
  - burst/backpressure flow: burst command sequence without host pacing
  - failure-path flow: non-empty-dir remove rejection, rename collision rejection, not-found read, `SDRWVERIFY 0` refusal, parser `CMD ERR` for oversized payload
  - command completion via `SDREQ id=...` + `SDWAIT <id>` with status/code checks

Optional env vars:

- `HOSTCTL_SDCARD_FLASH_FIRST=1` to flash first (mode arg defaults to `debug`)
- `HOSTCTL_SDCARD_VERIFY_LBA` (default `2048`)
- `HOSTCTL_SDCARD_BASE_PATH` to override test directory path on SD card
- `HOSTCTL_SDCARD_SUITE` (`all` default, `baseline`, `burst`, `failures`, `cutover`, or `no-card`)
- `no-card` verifies 20 bounded `init_failed`/`NoResponse` absent-card probes plus stack, memory,
  touch scheduling, panic, reset, and timeout gates; it does not provide SD/FAT correctness or
  throughput evidence
- `HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS` (default `300000`)

Burst/backpressure regression only:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_burst_regression.sh
```
