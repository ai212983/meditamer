## 2026-03-03: bounded soak follow-up for `SD_UPLOAD_CHUNK_MAX=65_536`

Validation shape:

```bash
MEDITAMER_SD_UPLOAD_CHUNK_MAX=65536 \
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_SOAK_CYCLES=10 \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifact:

- `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052`

Result:

- discovery debug: passed
- acceptance 1-cycle: passed
- acceptance 3-cycle: passed
- acceptance soak (10 cycles): failed with runtime panic

Panic marker:

- `Detected a write to the stack guard value on ProCpu`
- captured in:
  - `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052/panic_excerpt.log`
  - `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052/report.json`

Decision impact:

- keep default upload chunk size at `49_152` for now.
- keep `65_536` as opt-in override until soak is stable.

## 2026-03-03: panic-focused mitigation and `65_536` soak rerun

Mitigation commits:

- `dd9eaf7`: `feat(telemetry): add stack headroom probes for upload panic triage`
- `912fb02`: `fix(touch): reduce trace channel buffers to reclaim stack headroom`

Validation shape:

```bash
MEDITAMER_SD_UPLOAD_CHUNK_MAX=65536 \
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_SOAK_CYCLES=10 \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_65536_postfix_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifacts:

- pass run: `logs/wifi_regression_gate_65536_postfix_20260303_141406`
- non-panic transport failure run: `logs/wifi_regression_gate_stackdiag_postfix_20260303_140801`

`65_536` rerun result:

- discovery debug: passed
- acceptance 1-cycle: passed
- acceptance 3-cycle: passed
- acceptance soak (10 cycles): passed
- panic markers: none

Stack headroom evidence (`stack_diag`, pass run):

- `http_upload_route_entry`: `headroom=36024`, `total=43492`
- `sd_upload_begin_entry`: `headroom=11160`, `total=43492` (minimum observed)

Additional note:

- One intermediate instrumentation run failed with upload body read `ConnectionReset` and subsequent health-check timeout, while `NET_STATUS` stayed `Ready` and no panic/reboot markers were emitted.

Decision impact:

- panic signature (`stack guard write`) did not reproduce in the post-mitigation `65_536` soak rerun.
- keep default chunk size at `49_152` until extended soak repeats this result.

## 2026-03-03: owner decision to skip 24h soak and proceed

Decision:

- skip extended 24h soak gate and proceed with default switch.

Applied defaults:

- firmware: `SD_UPLOAD_CHUNK_MAX_DEFAULT=65_536` (`src/firmware/types/base.rs`)
- host fallback uploader: `HOSTCTL_UPLOAD_CHUNK_SIZE` default `65536` (`tools/hostctl/src/workflows_storage/upload/transfer.rs`)

Post-switch sanity check (default build, no chunk-size override):

- acceptance 1-cycle: `cycle 1 ... upload_ms=4892 ... throughput_kib_s=104.66`
- decomposition confirms default chunking: `chunks=8`, `max_chunk=65536`

Rollback path:

- override firmware chunk size at build time with:
  - `MEDITAMER_SD_UPLOAD_CHUNK_MAX=49152`
  - (fallback env key: `SD_UPLOAD_CHUNK_MAX`)

## 2026-03-03: transport-reset hardening + 3x default regression reruns

Hardening commit:

- `54e952f`: `fix(upload): harden read-body reset recovery and add reset metrics`

Behavioral changes:

- immediate socket abort on upload `read body` / `incomplete body` request errors.
- bounded abort wait around read-body reset path (`1.5s`).
- new upload metrics key: `req_read_body_reset`.

Re-validation artifacts (default build, default chunking `65_536`, soak=10):

- `logs/wifi_regression_gate_default65536_connresetfix_r1_20260303_144611`
- `logs/wifi_regression_gate_default65536_connresetfix_r2_20260303_144943`
- `logs/wifi_regression_gate_default65536_connresetfix_r3_20260303_145315`

Results:

- all three runs: `final_status=passed`.
- all three runs passed every stage:
  - `discovery_debug`
  - `acceptance_1_cycle`
  - `acceptance_3_cycle`
  - `acceptance_soak`
- panic/reboot flags stayed false in all reports.
- no `ConnectionReset`, `request err=read body`, or `body read err` signature matches in stage logs.

Decision impact:

- promote `SD_UPLOAD_CHUNK_MAX_DEFAULT=65_536` from risk-accepted default to stable-for-bounded-soak default under current mitigation set.

## 2026-03-03: SD SPI variance A/B (`36` vs `40` MHz)

Validation shape (same gate for both variants):

```bash
# A: 36 MHz (default)
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_SOAK_CYCLES=10 \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_sdspi36b_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh

# B: 40 MHz
MEDITAMER_SD_SPI_DATA_MHZ=40 \
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_SOAK_CYCLES=10 \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_sdspi40_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifacts:

- `36 MHz`: `logs/wifi_regression_gate_sdspi36b_20260303_151750`
- `40 MHz`: `logs/wifi_regression_gate_sdspi40_20260303_152151`

Gate status:

- both runs passed all stages (`discovery_debug`, `acceptance_1_cycle`,
  `acceptance_3_cycle`, `acceptance_soak`)
- discovery invariants remained stable in both runs.

Soak decomposition comparison (`upload_http: upload stats`, `n=10`):

- `36 MHz`:
  - `req_ms avg=3162.2`, range `3100..3248`
  - `sd_ms avg=2864.3`, range `2808..2962`
  - `read_wait_ms avg=2475.9`, range `2418..2573`
- `40 MHz`:
  - `req_ms avg=3377.2`, range `3106..4816`
  - `sd_ms avg=3037.9`, range `2789..4419`
  - `read_wait_ms avg=2694.0`, range `2417..4127`

Decision:

- keep `36 MHz` as SD SPI data-clock default.
- do not promote `40 MHz`; it introduces a materially wider upper tail in soak.

Tooling note from this pass:

- `2b2a3b3`: `fix(tooling): avoid abort metric false-positives in panic detection`
  - root cause: panic detector matched telemetry key `sess_timeout_abort=...`
    as panic due to broad `abort` substring check.
  - fix: narrow abort matching to real abort signatures (`abort()`, `aborted`,
    `abort was called`).

## 2026-03-03: CMD25 burst diagnostics + 3x soak correlation

Instrumentation commit:

- `3bb91e0`: `feat(storage): add cmd25 burst wait diagnostics for uploads`
- added per-upload `sd_upload: write_metrics` fields:
  - `cmd25_success_burst_ms_total`, `cmd25_success_burst_ms_avg`
  - `cmd25_ready_wait_count`, `cmd25_ready_wait_ms_total`,
    `cmd25_ready_wait_ms_avg`
  - `cmd25_ready_wait_polls_total`, `cmd25_ready_wait_polls_avg`
  - `cmd25_ready_wait_over_1ms`, `cmd25_ready_wait_over_4ms`,
    `cmd25_ready_wait_over_8ms`

3x `36 MHz` bounded soak runs used for correlation:

- `logs/wifi_regression_gate_sdspi36_burstdiag_r1_20260303_161323`
- `logs/wifi_regression_gate_sdspi36_burstdiag_r2_20260303_161645`
- `logs/wifi_regression_gate_sdspi36_burstdiag_r3b_20260303_162537`
- excluded run: `logs/wifi_regression_gate_sdspi36_burstdiag_r3_20260303_162008`
  (host-side health send failures while `NET_STATUS state=Ready`)

Correlation results:

- selected runs passed full gate (including soak).
- no selected soak uploads exceeded `req_ms > 3400` (`0/30`).
- CMD25 wait signals stayed low per upload:
  - `cmd25_ready_wait_ms_total` averaged `3.2..4.2 ms`
  - `cmd25_ready_wait_over_8ms` remained rare (`0..1` events per run total)
- interpretation: current latency spread is not primarily explained by CMD25
  write-ready waiting.

