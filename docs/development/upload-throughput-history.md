# Upload Throughput History

Regression command shape used for comparison:

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_LOG_PATH=logs/wifi_acceptance_baseline.log \
scripts/tests/hw/test_wifi_acceptance.sh
```

## Baseline Before Persistent Append Session

- Firmware commit: `b1d42bf`
- Date: `2026-02-26`
- Log: `logs/wifi_acceptance_20260226_164205.log`
- Result:
  - `payload_bytes=65536`
  - `upload_ms=45212`
  - `throughput_kib_s=1.42`
  - `connect_ms=6165`
  - `listen_ms=6165`

## After Persistent Append Session

- Firmware commit (working tree): `session-based append in sdcard::fat + sd_task upload integration`
- Date: `2026-02-26`
- Log: `logs/wifi_acceptance_20260226_170758.log`
- Result:
  - `payload_bytes=65536`
  - `upload_ms=42499`
  - `throughput_kib_s=1.51`
  - `connect_ms=6176`
  - `listen_ms=6176`

Comparison vs baseline:

- `upload_ms`: `45212 -> 42499` (`-2713 ms`, `-6.0%`)
- `throughput_kib_s`: `1.42 -> 1.51` (`+0.09 KiB/s`, `+6.3%`)

## 2026-02-26: Commit `1139a62` (flush append metadata on commit only)

Comparison command shape (same harness, 1 cycle each payload):

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_LOG_PATH=logs/wifi_acceptance_compare.log \
scripts/tests/hw/test_wifi_acceptance.sh
```

Pre-change reference commit:

- `009c17c` (`fix(regression): harden health reachability gating`)

Post-change commit:

- `1139a62` (`perf(sd-fat): flush append metadata on commit only`)

### Matched Samples

| payload_bytes | pre commit | pre upload_ms | pre KiB/s | post commit | post upload_ms | post KiB/s | delta upload_ms | delta KiB/s |
|---:|---|---:|---:|---|---:|---:|---:|---:|
| 131072 | `009c17c` | 77478 | 1.65 | `1139a62` | 45259 | 2.83 | `-41.6%` | `+71.5%` |
| 65536 | `009c17c` | 38016 | 1.68 | `1139a62` | 41651 | 1.54 | `+9.6%` | `-8.3%` |

Aggregate across both rows:

- effective throughput: `1.66 -> 2.21 KiB/s` (`+32.9%`)
- total upload time: `115494 -> 86910 ms` (`-24.7%`)

Source runs:

- pre 128 KiB: `test_name=health_harden_smoke`
- pre 64 KiB: `test_name=health_harden_3cycle` (cycle 1)
- post 128 KiB: `test_name=sdperf_post_128k_bounded`
- post 64 KiB: `test_name=sdperf_post_64k_bounded`

## 2026-03-01: guarded CMD25 + discovery/acceptance gate hardening

Validation order used:

1. bounded discovery debug after boot (`rounds=1`)
2. acceptance 1-cycle
3. acceptance 3-cycle
4. bounded soak (10 cycles)

Representative command shape:

```bash
HOSTCTL_NET_CYCLES=1|3|10 \
scripts/tests/hw/test_wifi_acceptance.sh
```

Primary run artifacts:

- discovery: `/tmp/final2_discovery_1round`
- acceptance 1-cycle: `/tmp/final4_acceptance_1cycle`
- acceptance 3-cycle: `/tmp/final4_acceptance_3cycle`
- acceptance soak10: `/tmp/final4_acceptance_soak10`

Observed throughput:

- 1-cycle: `upload_ms=4002`, `throughput_kib_s=127.94`
- 3-cycle average: `avg_upload_s=4.35`, `avg_kib_s=117.78`
- soak10 average: `avg_upload_s=4.34`, `avg_kib_s=118.79`

Comparison vs earlier historical aggregate (`2.21 KiB/s` effective, 2026-02-26 section above):

- effective throughput: `2.21 -> 118.79 KiB/s` (`~53.8x`)

## 2026-03-01: wider HTTP RX window (PSRAM) + host TCP no-delay

Change set:

- firmware: increase upload listener TCP RX buffer target to `65536` (TX `4096`)
- hostctl: enable `tcp_nodelay(true)` for upload client requests

Validation command shape (upload-rate diagnostics mode):

```bash
HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0 \
HOSTCTL_NET_CYCLES=1|3|10 \
scripts/tests/hw/test_wifi_acceptance.sh
```

Artifacts:

- 1-cycle: `/tmp/final5_acceptance_1cycle_nogate`
- 3-cycle: `/tmp/final5_acceptance_3cycle_nogate`
- soak10: `/tmp/final5_acceptance_soak10b_nogate`

Observed throughput (524288-byte payload):

- 1-cycle: `upload_ms=2458`, `throughput_kib_s=208.30`
- 3-cycle average: `avg_upload_s=2.64`, `avg_kib_s=195.58`
- soak10 average: `avg_upload_s=2.68`, `avg_kib_s=192.56`

Comparison vs previous iteration (`final4`, guarded CMD25 section):

- 1-cycle throughput: `127.94 -> 208.30 KiB/s` (`+62.8%`)
- 3-cycle average throughput: `117.78 -> 195.58 KiB/s` (`+66.1%`)
- soak10 average throughput: `118.79 -> 192.56 KiB/s` (`+62.1%`)

Device-side upload telemetry moved from roughly:

- `body_ms=1763`, `sd_ms=1822`, `req_ms=3669`

to:

- `body_ms=855`, `sd_ms=1301`, `req_ms=2205` (1-cycle representative)

Next bottleneck indicated by telemetry:

- `sd_ms` is now the dominant component (`~1.2-1.3s` per 512 KiB upload),
  so further gains likely require SD write-path improvements (not HTTP buffering).

## 2026-03-01: SD write-path diagnostics (CMD24/CMD25 counters)

Instrumentation added in firmware:

- `sd_upload: write_metrics ...`
  - `cmd24_sectors`
  - `cmd25_attempt_bursts`
  - `cmd25_success_bursts`
  - `cmd25_fallback_bursts`
  - `cmd25_attempt_sectors`
  - `cmd25_success_sectors`

Representative run (after boot, no boot-gate for pure upload diagnostics):

- artifact: `/tmp/final6_acceptance_3cycle_nogate`
- per-upload metrics:
  - `cmd24_sectors=40`
  - `cmd25_attempt_bursts=21`
  - `cmd25_success_bursts=21`
  - `cmd25_fallback_bursts=0`
  - `cmd25_attempt_sectors=1024`
  - `cmd25_success_sectors=1024`

Interpretation:

- payload data path is already fully `CMD25` (no fallback),
- `CMD24` traffic is metadata/management overhead (likely FAT + directory updates),
- remaining speed ceiling is mostly:
  - SD-side write latency (`sd_ms`), and
  - host/network body-feed jitter (`body_ms`) variance.

## 2026-03-01: SD SPI data clock sweep (24/30/36/40 MHz, bounded 1-cycle)

Change set:

- SD probe now supports build-time data clock override:
  - `MEDITAMER_SD_SPI_DATA_MHZ` (preferred)
  - `SD_SPI_DATA_MHZ` (fallback alias)
- Accepted range: `12..40` MHz
- New default selected: `36` MHz

Benchmark shape used:

```bash
HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0 \
HOSTCTL_NET_CYCLES=1 \
HOSTCTL_NET_OPERATION_RETRIES=1 \
HOSTCTL_NET_UPLOAD_TIMEOUT_SEC=45 \
HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC=20 \
HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC=10 \
scripts/tests/hw/test_wifi_acceptance.sh
```

Results (524288-byte payload):

- `24 MHz`: `upload_ms=2867`, `throughput_kib_s=178.58`
- `30 MHz`: `upload_ms=2956`, `throughput_kib_s=173.21`
- `36 MHz`: `upload_ms=2626`, `throughput_kib_s=194.97`
- `40 MHz`: `upload_ms=2812`, `throughput_kib_s=182.08`

Device-side write metrics stayed stable in sampled runs:

- `cmd24_sectors=40`
- `cmd25_attempt_sectors=1024`
- `cmd25_success_sectors=1024`
- `cmd25_fallback_bursts=0`

Representative upload-phase stats:

- `30 MHz`: `body_ms=919`, `sd_ms=1302`, `req_ms=2275`
- `36 MHz`: `body_ms=1034`, `sd_ms=1228`, `req_ms=2307`
- `40 MHz`: `body_ms=782`, `sd_ms=1300`, `req_ms=2135`

Selection:

- `36 MHz` gave the best measured host throughput in this sweep
  without reintroducing CMD25 fallback or write errors.
- Keep env override available for board/SD-card-specific rollback:
  `MEDITAMER_SD_SPI_DATA_MHZ=24` (or another validated value).

## 2026-03-03: upload chunk pipeline A/B regression gate (`off` vs `on`)

Validation shape (full regression gate, no soak):

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_ab_off_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh

CARGO_FEATURES=asset-upload-http-pipeline \
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_ab_on_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifacts:

- pipeline off: `logs/wifi_regression_gate_ab_off_20260303_115711`
- pipeline on: `logs/wifi_regression_gate_ab_on_20260303_120041`

Gate status:

- both runs passed discovery debug + acceptance 1-cycle + acceptance 3-cycle
- no panic/reboot markers
- no zero-discovery regression (`zero_discovery_rounds=0` in both runs)

Observed throughput (524288-byte payload):

- pipeline off:
  - 1-cycle: `upload_ms=4412`, `throughput_kib_s=116.05`
  - 3-cycle average: `avg_upload_s=4.79`, `avg_kib_s=107.48`
- pipeline on:
  - 1-cycle: `upload_ms=4103`, `throughput_kib_s=124.79`
  - 3-cycle average: `avg_upload_s=4.14`, `avg_kib_s=123.86`

Delta (pipeline on vs off):

- 1-cycle upload time: `4412 -> 4103 ms` (`-7.0%`)
- 3-cycle average upload time: `4.79 -> 4.14 s` (`-13.6%`)
- 3-cycle average throughput: `107.48 -> 123.86 KiB/s` (`+15.2%`)

Notes:

- Device-side `upload_http: upload stats` line confirms feature mode:
  - off log shows `pipeline=off`
  - on log shows `pipeline=on`
- With overlap enabled, per-bucket decomposition is not directly comparable to
  stop-and-wait mode in isolation; end-to-end `upload_ms` and host KiB/s are the
  primary A/B decision signals.

## 2026-03-03: default-feature confirmation gate (pipeline on by default)

Validation shape:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_default_confirm_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifact:

- `logs/wifi_regression_gate_default_confirm_20260303_121014`

Gate status:

- passed discovery debug + acceptance 1-cycle + acceptance 3-cycle
- no panic/reboot markers
- no zero-discovery regression

Observed throughput (524288-byte payload):

- 1-cycle: `upload_ms=4169`, `throughput_kib_s=122.81`
- 3-cycle average: `avg_upload_s=4.54`, `avg_kib_s=113.11`

Representative decomposition signal (`pipeline=on`):

- `chunks=11`, `avg_chunk=47662`, `max_chunk=49152`
- `read_wait_ms=2714`, `sd_ms=3053`, `commit_ms=233`, `req_ms=3327`

Interpretation:

- default-on behavior stays within prior pipeline-on envelope
- commit/metadata remains secondary versus multi-second transfer path costs, so
  Phase C metadata tightening stays deferred.

## 2026-03-03: upload chunk-size A/B (`49_152` vs `65_536`)

Change set:

- firmware upload chunk size is now build-time tunable for PSRAM upload builds:
  - preferred env: `MEDITAMER_SD_UPLOAD_CHUNK_MAX`
  - fallback env: `SD_UPLOAD_CHUNK_MAX`
  - accepted range: `4096..65536`
- internal upload chunk command length widened to `u32` so `65536` is represented safely.

Validation shape:

```bash
# baseline (default 49_152)
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_chunk_ab_49152_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh

# variant (65_536)
MEDITAMER_SD_UPLOAD_CHUNK_MAX=65536 \
ESPFLASH_PORT=/dev/cu.usbserial-510 \
scripts/device/flash.sh debug

HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_POLICY_PATH=tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_REGRESSION_OUTPUT_DIR=logs/wifi_regression_gate_chunk_ab_65536_<timestamp> \
scripts/tests/hw/test_wifi_regression_gate.sh
```

Artifacts:

- `49_152`: `logs/wifi_regression_gate_chunk_ab_49152_final_20260303_123948`
- `65_536`: `logs/wifi_regression_gate_chunk_ab_65536_20260303_124328`

Gate status:

- both runs passed discovery debug + acceptance 1-cycle + acceptance 3-cycle
- no panic/reboot markers in the final A/B pair
- no zero-discovery regression in the final A/B pair

Observed throughput (524288-byte payload):

- `49_152`:
  - 1-cycle: `upload_ms=4828`, `throughput_kib_s=106.05`
  - 3-cycle average: `avg_upload_s=5.08`, `avg_kib_s=101.07`
- `65_536`:
  - 1-cycle: `upload_ms=4708`, `throughput_kib_s=108.75`
  - 3-cycle average: `avg_upload_s=4.36`, `avg_kib_s=117.86`

Delta (`65_536` vs `49_152`):

- 1-cycle upload time: `4828 -> 4708 ms` (`-2.5%`)
- 3-cycle average upload time: `5.08 -> 4.36 s` (`-14.2%`)
- 3-cycle average throughput: `101.07 -> 117.86 KiB/s` (`+16.6%`)

Representative decomposition:

- `49_152` path:
  - `chunks=11`, `max_chunk=49152`, `cmd25_attempt_bursts=21`, `cmd25_fallback_bursts=0`
- `65_536` path:
  - `chunks=8`, `max_chunk=65536`, `cmd25_attempt_bursts=16`, `cmd25_fallback_bursts=0`

Notes:

- one earlier baseline attempt (`logs/wifi_regression_gate_chunk_ab_49152_20260303_122009`)
  recorded a panic during acceptance (`runtime_panic_other`), but that panic did not reproduce
  in the final paired A/B runs above.

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

## 2026-03-03: FAT append diagnostics + 3x soak correlation

Instrumentation commit:

- `64f6da6`: `feat(upload): add fat append chunk timing diagnostics`
- adds `sd_upload: write_metrics` chunk-boundary fields:
  - `chunk_total_ms_*`, `chunk_ensure_ready_ms_*`, `chunk_payload_lock_ms_*`
  - `chunk_append_ms_*`, `chunk_append_capacity_ms_*`,
    `chunk_append_write_data_ms_*`
  - `chunk_overhead_ms_*`, plus `chunk_total_over_200ms/_over_400ms` and
    `chunk_append_over_200ms/_over_400ms`

3x `36 MHz` bounded soak artifacts:

- `logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`
- `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`
- `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`

Gate status:

- all three runs passed every stage (`discovery_debug`, `acceptance_1_cycle`,
  `acceptance_3_cycle`, `acceptance_soak`).

Correlation summary:

- `req_ms > 3400`: `0/30`.
- `chunk_max_ms > 400`: `5/30` (`420`, `629`, `477`, `449`, `407`).
- append-path timing stayed tight across all runs:
  - `chunk_append_ms_avg`: `126.7..127.4 ms`
  - `chunk_append_capacity_ms_avg`: `38.0..38.5 ms`
  - `chunk_append_write_data_ms_avg`: `87.6..87.9 ms`
  - observed `chunk_append_ms_max` ceiling: `145 ms`
- representative outlier pair:
  - upload with `chunk_max_ms=629` had `chunk_append_ms_max=134`.
- interpretation:
  - current `chunk_max_ms` upper tail is not primarily caused by
    `fat::append_session_write` execution time.
  - residual wait outside append remains material (`sd_task_ms/chunk -
    chunk_append_ms_avg` roughly `150..191 ms` in these runs).

## 2026-03-03: queue-boundary diagnostics + 3x soak correlation

Instrumentation commit:

- `e85f2a7`: `feat(upload): add chunk queue-boundary residual diagnostics`
- key additions:
  - `SdUploadRequest.enqueued_at_ms`
  - per-chunk response timings: `chunk_queue_wait_ms`, `chunk_handler_ms`
  - `upload_http: upload stats` fields:
    - `sd_task_queue_wait_ms`
    - `sd_task_handler_ms`
    - `sd_task_residual_ms`
  - `sd_upload: write_metrics` fields:
    - `chunk_queue_wait_ms_*`
    - `chunk_non_append_ms_*`
    - `chunk_residual_ms_*`

Runs:

- selected:
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`
- excluded:
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r2_20260303_170602`
    (`acceptance_1_cycle` failed with `net_wait_ready: listener timeout`)

Selected-set summary (`n=30` uploads):

- `req_ms avg=3060.8`, range `2933..3752`, `req_ms > 3400`: `1/30`
- `chunk_max_ms avg=364.7`, range `318..666`, `chunk_max_ms > 400`: `6/30`
- `chunk_append_ms_avg` stayed stable at `126.2 ms`
- queue/handler/residual decomposition (`upload_http: upload stats`):
  - `sd_task_queue_wait_ms avg=47.0`
  - `sd_task_handler_ms avg=1017.6`
  - `sd_task_residual_ms avg=1239.0`
- high-tail sample:
  - `chunk_max_ms=666` with `sd_task_handler_ms=1027`,
    `sd_task_residual_ms=1845`

Interpretation:

- queue wait is present but not dominant.
- handler time is stable and consistent with FAT append timings.
- the dominant unexplained component is post-handler residual wait
  (`sd_task_residual_ms`).

## 2026-03-03: post-handler residual split instrumentation

Implemented split-residual instrumentation in firmware upload path:

- SD task now stamps chunk handler completion and publish edge timing.
- SD bridge stamps receive edge and computes publish-to-receive delay.
- `upload_http: upload stats` now emits:
  - `sd_task_post_handler_ms`
  - `sd_task_publish_to_receive_ms`
  - `sd_task_residual_other_ms`
- existing `sd_task_residual_ms` is preserved for continuity and now decomposes
  into:
  - `sd_task_post_handler_ms`
  - `sd_task_publish_to_receive_ms`
  - `sd_task_residual_other_ms`

Smoke verification:

- run: `logs/wifi_acceptance_split_residual_smoke_20260303_173818.log`
- sample upload stats:
  - `sd_task_residual_ms=1291`
  - `sd_task_post_handler_ms=1`
  - `sd_task_publish_to_receive_ms=1290`
  - `sd_task_residual_other_ms=0`
- interpretation:
  - in this sample, residual is dominated by publish-to-receive delay rather
    than SD-task post-handler pre-publish delay.

Next step:

- run bounded 3x `36 MHz` regression gates with split-residual instrumentation.
- correlate `chunk_max_ms > 400` uploads against the new split fields to
  identify which post-handler leg dominates.

## 2026-03-03: split-residual correlation (3x bounded soak)

Runs used:

- `logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`
- `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`
- `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`

Notes:

- full regression-gate attempts in the same window hit non-upload
  acceptance-stage failures (listener/boot-discovery gating), so split-residual
  correlation used direct bounded soak acceptance runs.
- one `upload_http: upload stats` line in `r3` was serial-concatenated and was
  excluded from aggregate parsing.

Correlation summary (`upload_http: upload stats`, valid `n=29`):

- `req_ms avg=3135.1`
- `chunk_max_ms avg=401.4`
- `chunk_max_ms > 400`: `9/29`
- decomposition means:
  - `sd_task_queue_wait_ms avg=43.7`
  - `sd_task_handler_ms avg=1035.4`
  - `sd_task_residual_ms avg=1259.9`
- residual split means:
  - `sd_task_post_handler_ms avg=0.4`
  - `sd_task_publish_to_receive_ms avg=1259.5`
  - `sd_task_residual_other_ms avg=0.0`
- outlier (`chunk_max_ms > 400`) residual split:
  - `sd_task_residual_ms avg=1429.2`
  - `sd_task_publish_to_receive_ms avg=1429.0`
  - `sd_task_post_handler_ms avg=0.2`
  - `sd_task_residual_other_ms avg=0.0`

Interpretation:

- split confirms post-handler residual is not SD-task post-handler delay.
- residual is almost entirely bridge publish-to-receive delay in this campaign.

Next root-cause focus:

- investigate bridge receive cadence during pipelined body ingest to determine
  how much of publish-to-receive is expected overlap accounting versus avoidable
  receive lag affecting request tail behavior.

## 2026-03-03: bridge non-blocking inflight drain mitigation

Mitigation implemented:

- bridge now tries to drain completed inflight SD chunk results non-blockingly
  during body ingest (between socket reads), instead of waiting only at explicit
  queue-boundary flush points.

Validation runs:

- `logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`
- `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`

Aggregate comparison (pre-fix split-residual set vs post-fix trydrain set):

- pre-fix (`n=29`):
  - `req_ms avg=3135.1`
  - `chunk_max_ms avg=401.4`, `chunk_max_ms > 400`: `9`
  - `sd_task_residual_ms avg=1259.9`
  - `sd_task_publish_to_receive_ms avg=1259.5`
- post-fix (`n=20`):
  - `req_ms avg=3175.6`
  - `chunk_max_ms avg=172.4`, `chunk_max_ms > 400`: `0`
  - `sd_task_residual_ms avg=40.0`
  - `sd_task_publish_to_receive_ms avg=39.7`

Interpretation:

- dominant residual leg (`publish_to_receive`) drops by ~`96.8%` in this sample.
- chunk roundtrip tail (`chunk_max_ms`) collapses accordingly.
- request-time mean remained in the same multi-second band; requires further
  gate-scale validation before concluding net throughput benefit.

Next step:

- run full 1/3/soak regression gates on this mitigation and confirm reliability
  plus request-time behavior under bounded soak.

## 2026-03-03: host transport A/B for ingress isolation (`direct` vs `chunked`)

Host-tooling change:

- added `HOSTCTL_UPLOAD_MODE` in hostctl upload flow:
  - `auto` (default): try direct `PUT /upload`, fallback to chunked
  - `direct`: force direct `PUT /upload` only
  - `chunked`: force `/upload_begin` + `/upload_chunk` + `/upload_commit`

10-cycle acceptance runs:

- direct mode:
  - `logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`
  - summary: `avg_upload_s=6.27`, `avg_kib_s=82.64`
- chunked mode:
  - `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`
  - summary: `avg_upload_s=17.88`, `avg_kib_s=28.93`

Normalization method:

- used `METRICS UPLOAD_PHASE` delta (last-first) in each run to compare equal
  transferred bytes while avoiding serial-line sampling bias.
- both runs transferred `5.0 MiB` total (`10` acceptance uploads).

`METRICS UPLOAD_PHASE` delta comparison (per `512 KiB` payload):

- direct (`reqs_per_512KiB=1.0`):
  - `body_ms=2457.3`
  - `sd_ms=1563.8`
  - `req_ms=3156.3`
- chunked (`reqs_per_512KiB=8.0`):
  - `body_ms=1685.0`
  - `sd_ms=1056.6`
  - `req_ms=2923.6`

Interpretation:

- chunked transport reduces server-side per-byte timing inside individual
  requests, but total upload throughput is much worse because the multi-request
  orchestration dominates wall-clock time.
- optimization should remain on direct `PUT /upload` and target ingress pacing
  within that path.
- `HOSTCTL_UPLOAD_MODE` is retained as a deterministic A/B switch for future
  experiments.

## 2026-03-03: direct-path HTTP RX buffer A/B (`65_536` vs `131_072`)

Firmware change:

- added compile-time HTTP RX socket buffer tuning (PSRAM upload builds):
  - preferred env: `MEDITAMER_HTTP_RX_BUF_TARGET`
  - fallback env: `HTTP_RX_BUF_TARGET`
  - accepted range: `8192..262144` (default `65536`)

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_NET_CYCLES=10`):

- baseline (`65_536`, default build):
  - `logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`
  - runtime confirms: `upload_http: http_rx buffer placement=Psram bytes=65536`
- variant (`131_072`):
  - `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`
  - runtime confirms: `upload_http: http_rx buffer placement=Psram bytes=131072`

`upload_http: upload stats` aggregates (`n=10` each):

- baseline:
  - `read_wait_ms avg=2398.9`
  - `req_ms avg=3093.5`
  - `ingress_pre_read_q_total avg=36347.6` (`~413.0 bytes/read`)
  - `ingress_read_wait_over_50ms avg=7.8` (`8.9%` reads)
- variant:
  - `read_wait_ms avg=2802.6`
  - `req_ms avg=3491.6`
  - `ingress_pre_read_q_total avg=58205.2` (`~937.3 bytes/read`)
  - `ingress_read_wait_over_50ms avg=16.8` (`27.1%` reads)

`METRICS UPLOAD_PHASE` delta normalization (equal `5.0 MiB` transferred):

- baseline:
  - `body_ms=2398.9 ms/512KiB`
  - `sd_ms=1566.3 ms/512KiB`
  - `req_ms=3093.5 ms/512KiB`
- variant:
  - `body_ms=2802.6 ms/512KiB`
  - `sd_ms=1684.6 ms/512KiB`
  - `req_ms=3491.4 ms/512KiB`

Host throughput summary:

- baseline: `avg_kib_s=97.83`
- variant: `avg_kib_s=80.26`

Decision:

- retain default `HTTP_RX_BUF_TARGET=65_536`.
- do not promote `131_072`; this bounded direct-mode A/B worsens latency and
  throughput.

Next focus:

- ingress pacing root-cause in direct upload (host send cadence / burst-idle
  pattern correlation against firmware `read_wait_ms`).

## 2026-03-03: host send diagnostics + retry-class probe matrix (direct path)

Host-tooling changes:

- added host direct-upload diagnostics:
  - `host_upload_send_diag` (attempts, send/total ms, optional body cadence)
  - `host_upload_retry_diag` (retry class flags including `transport_reset`)
- added sidecar log persistence:
  - default `<HOSTCTL_NET_LOG_PATH>.hostdiag`
  - optional `HOSTCTL_UPLOAD_SEND_DIAG_PATH`
- added retry hardening knob:
  - `HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH`

Primary correlation run (`direct`, cycles=10):

- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log`
- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log.hostdiag`

Aggregate (`n=10`):

- firmware:
  - `read_wait_ms avg=2475.9`
  - `req_ms avg=3150.0`
- host:
  - `send_ms avg=3326.4`
  - `avg_attempts=2.00`
  - `corr(send_ms, read_wait_ms)=0.944`

Retry-class probes:

- pool A/B:
  - off: `logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`
  - on: `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`
  - `read_wait_ms avg`: `2439.6 -> 2395.0`
  - `req_ms avg`: `3116.2 -> 3084.8`
- connection-close A/B:
  - off: `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`
  - on: `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`
  - `read_wait_ms avg`: `2541.3 -> 2419.0`
  - `req_ms avg`: `3204.3 -> 3080.0`
  - retry count increased in this sample (`1 -> 3`)
- fresh-client A/B:
  - off: `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`
  - on: `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`
  - near-neutral latency deltas; retries unchanged (`3 -> 3`)

Interpretation:

- host send timing remains tightly coupled with firmware `read_wait_ms`.
- no host transport toggle consistently eliminates `transport_reset` first-attempt
  retries.

## 2026-03-03: `HOSTCTL_UPLOAD_TCP_NODELAY` A/B (`1` vs `0`)

Change:

- added host knob `HOSTCTL_UPLOAD_TCP_NODELAY` (`1` default).

Runs (`direct`, cycles=5):

- `TCP_NODELAY=1`:
  - `logs/wifi_acceptance_nodelayab_on_direct5_20260303_195857.log`
  - summary: `avg_upload_s=5.56`, `avg_kib_s=93.23`
- `TCP_NODELAY=0`:
  - `logs/wifi_acceptance_nodelayab_off_direct5_20260303_200015.log`
  - summary: `avg_upload_s=5.91`, `avg_kib_s=87.59`

`upload_http: upload stats` aggregates (`n=5`):

- `TCP_NODELAY=1`:
  - `read_wait_ms avg=2289.4`
  - `req_ms avg=2960.0`
  - `ingress_pre_read_q_total avg=31602.2`
- `TCP_NODELAY=0`:
  - `read_wait_ms avg=2397.6`
  - `req_ms avg=3060.4`
  - `ingress_pre_read_q_total avg=35916.2`

Decision:

- keep `HOSTCTL_UPLOAD_TCP_NODELAY=1` default; disabling it regressed bounded
  throughput and request timing.

## 2026-03-03: ingress wait split telemetry (`empty_q` vs `nonempty_q`)

Firmware change:

- added upload stats fields:
  - `ingress_read_wait_empty_q_ms`
  - `ingress_read_wait_nonempty_q_ms`

Validation run:

- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`
- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log.hostdiag`

Aggregate (`n=3`):

- `read_wait_ms avg=2355.7`
- `ingress_read_wait_empty_q_ms avg=2351.3`
- `ingress_read_wait_nonempty_q_ms avg=4.3`
- empty-queue share of `read_wait_ms`: `~99.8%`

Conclusion:

- current ingress bottleneck is almost entirely no-data wait at socket read
  points; this is not a buffered-data drain delay.

## 2026-03-04: Wi-Fi RSSI context added to ingress diagnostics

Implementation:

- connected watchdog now samples STA RSSI (`WifiController::rssi()`).
- telemetry now emits:
  - `METRICS WIFI_LINK rssi_last_dbm=... rssi_min_dbm=... rssi_max_dbm=... rssi_samples=... rssi_low_samples=...`
- upload request stats now include Wi-Fi RSSI context:
  - `wifi_rssi_last_dbm`
  - `wifi_rssi_min_dbm`
  - `wifi_rssi_max_dbm`
  - `wifi_rssi_samples`
  - `wifi_rssi_low_samples`

Validation runs:

- guarded post-flash acceptance attempt:
  - `logs/wifi_acceptance_ingress_rssi_direct3_20260304_082811.log`
  - failed at boot discovery gate (`ready=false`, zero discovery evidence)
- discovery recovery proof:
  - `logs/wifi_discovery_rssi_recover_20260304_083129.log`
  - summary: `ready_rounds=8`, `zero_discovery_rounds=0`,
    `total_scan_nonzero_events=1`
- bounded direct correlation run:
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log`
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log.hostdiag`

Direct 10-cycle aggregate (`n=10`):

- request timing:
  - `read_wait_ms avg=2532.8`
  - `req_ms avg=3227.5`
  - `ingress_read_wait_empty_q_ms avg=2528.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`
- Wi-Fi RSSI context:
  - `wifi_rssi_last_dbm avg=-62.5` (range `-68..-59`)
  - `wifi_rssi_min_dbm avg=-71.0`
  - `wifi_rssi_low_samples avg=1.0`
- host/firmware coupling:
  - `host retry_count=10`, `avg_attempts=2.00`
  - `corr(send_ms, read_wait_ms)=0.991`
  - `corr(rssi_last, read_wait_ms)=0.056`

Interpretation:

- ingress wait remains empty-queue dominated (~`99.8%` of read-wait).
- within this RSSI band, signal strength variation was not the primary driver of
  request-time variance.
- dominant coupling remains host send cadence and first-attempt transport-reset
  retry behavior.

## 2026-03-04: host retry cause-chain diagnostics + pre-PUT pacing A/B

Host tooling changes:

- expanded `host_upload_retry_diag` with typed reqwest/IO flags and compact
  full cause chain (`err_chain`).
- added `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` (`0` default), applied before each
  direct `PUT /upload` attempt.
- added host failure class `host_transport_connect_refused` for better
  post-run classification.

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_SEND_DIAG=1`, cycles=5):

- baseline (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log.hostdiag`
  - summary: `avg_upload_s=6.45`, `avg_kib_s=79.72`
- variant (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=120`):
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log.hostdiag`
  - summary: `avg_upload_s=5.11`, `avg_kib_s=103.96`

Hostdiag aggregate:

- baseline:
  - retries: `5/5`
  - `avg_attempts=2.00`
  - repeated first-attempt chain:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- variant:
  - retries: `0/5`
  - `avg_attempts=1.00`
  - no first-attempt retry lines observed.

Firmware upload-stats aggregate (`n=5`, last five upload requests per run):

- baseline:
  - `read_wait_ms avg=2546.8`
  - `req_ms avg=3224.6`
  - `ingress_read_wait_empty_q_ms avg=2541.4`
  - `ingress_read_wait_nonempty_q_ms avg=5.4`
- variant:
  - `read_wait_ms avg=2602.8`
  - `req_ms avg=3285.8`
  - `ingress_read_wait_empty_q_ms avg=2598.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`

Interpretation:

- primary retry signature is now explicit and stable: first-attempt connect
  refused on direct `PUT /upload`.
- short host pre-PUT delay suppresses this class in bounded A/B and improves
  end-to-end throughput by avoiding retries.
- per-success ingress wait remains empty-queue dominated; next optimization
  target stays listener/accept readiness around the `mkdir -> upload` boundary.

## 2026-03-04: `NET_ACCEPT` microsecond gap evidence + firmware keep-alive fix candidate

Instrumentation change:

- upgraded listener re-arm telemetry to microseconds:
  - `METRICS NET_ACCEPT arm_gap_n arm_gap_us arm_gap_us_max arm_gap_after_mkdir_n arm_gap_after_mkdir_us arm_gap_after_mkdir_us_max`

Validation run (pre-fix evidence, direct mode):

- `logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log`
- `logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log.hostdiag`
- host summary:
  - `avg_upload_s=5.72`, `avg_kib_s=90.68`
  - retries: `3/3` (`avg_attempts=2.00`)
  - each first-attempt failure chain includes:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- firmware (last `METRICS NET_ACCEPT` line in run):
  - `arm_gap_n=22`
  - `arm_gap_us=4832` (aggregate)
  - `arm_gap_us_max=233`
  - `arm_gap_after_mkdir_n=3`
  - `arm_gap_after_mkdir_us=672` (aggregate)
  - `arm_gap_after_mkdir_us_max=226`

Interpretation:

- the observed listener re-arm window is sub-millisecond and still sufficient to
  race immediate reconnects in this direct path.
- this supports a protocol/connection-lifecycle fix over coarse delay-only
  mitigation.

Firmware fix candidate implemented (awaiting live validation):

- switched HTTP responses to `HTTP/1.1` keep-alive semantics (removed forced
  `Connection: close` behavior).
- updated listener socket cycle to serve multiple requests per accepted socket.
- added keep-alive idle guard (`500 ms` header timeout after first request) to
  avoid one idle peer blocking accepts.

Validation status:

- on-device validation blocked after flash attempt failed with serial transport
  loss (`Broken pipe`, then `Serial port not found`); USB serial node was absent
  on host (`/dev/cu.*`).
- next action is to restore serial visibility and rerun bounded direct acceptance
  at `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`.

## 2026-03-04: keep-alive validation + host cross-cycle client reuse

Validation context:

- serial recovered on USB and firmware was flashed/validated in direct mode.
- acceptance runs used `HOSTCTL_UPLOAD_MODE=direct`,
  `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, `HOSTCTL_UPLOAD_SEND_DIAG=1`.
- acceptance profile currently pins `HOSTCTL_NET_LOG_PATH=acceptance_3_cycle`;
  evidence below is from appended run segments in that stream.

Firmware keep-alive validation:

- keep-alive ON (cycles=3): `avg_upload_s=3.62`, `avg_kib_s=141.86`
- forced close (cycles=3): `avg_upload_s=3.40`, `avg_kib_s=150.67`
- keep-alive ON repeat (cycles=3): `avg_upload_s=3.53`, `avg_kib_s=145.39`
- matched warmed pair (cycles=3):
  - forced close: `avg_kib_s=149.52`
  - keep-alive ON: `avg_kib_s=147.06`
- matched warmed pair (cycles=6):
  - keep-alive ON: `avg_upload_s=3.45`, `avg_kib_s=148.58`
  - forced close: `avg_upload_s=3.45`, `avg_kib_s=148.30`

Interpretation:

- keep-alive behavior is stable in bounded acceptance.
- throughput delta is small and inconsistent; practical outcome is near parity.

Host cross-cycle client reuse experiment:

- host tooling now supports reusable client path for wifi-acceptance:
  - `HOSTCTL_NET_REUSE_UPLOAD_CLIENT=1` (default `0`).
- strict-guard 6-cycle reuse run (`max_delta=0`) hit one first-attempt timeout:
  - `HOST_FAILURE class=host_transport_send_fail` on cycle 3,
  - retry recovered upload,
  - guard failed on `req_read_body_reset delta=1`.
- reuse run with relaxed guard (`max_delta=2`) completed:
  - keep-alive ON: `avg_upload_s=3.64`, `avg_kib_s=142.61`
  - forced close: `avg_upload_s=3.50`, `avg_kib_s=146.36`
- default mode (reuse off) sanity run (cycles=3) remained stable:
  - `avg_upload_s=3.49`, `avg_kib_s=146.90`

Decision:

- do not promote host cross-cycle client reuse as a default throughput
  optimization.
- retain it as opt-in for diagnostics while primary root-cause focus remains
  firmware ingress empty-queue `read_wait_ms`.

## 2026-03-04: firmware ingress empty-queue mitigation via cooperative read fairness

Firmware change:

- in `src/firmware/storage/upload/http/connection/body.rs`, added periodic
  cooperative yield while draining immediately-ready socket reads:
  - initial bounded-run thresholds: `12 KiB` / `24 reads`
  - later promoted to configurable firmware defaults via
    `HTTP_INGRESS_COOP_YIELD_{BYTES,READS}`.
- intent: give the net runner scheduling opportunities during long ready-read
  bursts so RX queue refill cadence improves and empty-queue stalls shrink.

Bounded validation (`direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, cycles=3):

- baseline (pre-change):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.27`
  - last three `upload_http: upload stats` `read_wait_ms`: `2368`, `2389`, `2493`
  - last three `ingress_read_wait_empty_q_ms`: `2362`, `2383`, `2483`
- post-change run A:
  - summary: `avg_upload_s=3.43`, `avg_kib_s=149.33`
  - `read_wait_ms`: `2304`, `2295`, `2293`
  - `ingress_read_wait_empty_q_ms`: `2302`, `2291`, `2285`
- post-change run B (immediate confirmation):
  - summary: `avg_upload_s=3.42`, `avg_kib_s=149.57`
  - `read_wait_ms`: `2280`, `2397`, `2255`
  - `ingress_read_wait_empty_q_ms`: `2276`, `2391`, `2247`

Observed bounded deltas:

- throughput improved by about `+2.8..+3.0%` in the two post-change runs.
- compared sample-average `read_wait_ms` moved:
  - pre: `2416.7`
  - post: `2304.0` (`-4.7%`)

Interpretation:

- cooperative fairness in the firmware read loop appears to reduce empty-queue
  ingress wait in short bounded runs, with corresponding throughput lift.
- next step is longer-cycle and soak confirmation to separate persistent effect
  from RF variance.

## 2026-03-04: ingress fairness threshold tuning and default promotion

Tuning setup:

- threshold knobs made build-time configurable:
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES` / fallback
    `HTTP_INGRESS_COOP_YIELD_BYTES`
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS` / fallback
    `HTTP_INGRESS_COOP_YIELD_READS`
- mode: direct upload (`HOSTCTL_UPLOAD_MODE=direct`)

Bounded matrix (cycles=6):

- `4096/16`: `avg_kib_s=148.05`, `stddev=3.24`
- `6144/20`: `avg_kib_s=147.81`, `stddev=4.01`
- `8192/24`: `avg_kib_s=149.05`, `stddev=3.51`

Extended confirmation (cycles=10):

- `4096/16`: `avg_kib_s=147.78`, `stddev=4.09`
- `8192/24`: `avg_kib_s=149.67`, `stddev=2.30`

Decision:

- promoted firmware defaults to `8192/24` in `src/firmware/types/base.rs`:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`
- reason: in longer A/B, `8192/24` produced both higher throughput and lower
  variance than `4096/16`.

## 2026-03-04: bounded soak validation for promoted ingress fairness defaults

Artifact:

- `logs/wifi_acceptance_ingressfairness_soak10_20260304_113208.log`

Run profile:

- direct upload mode (`HOSTCTL_UPLOAD_MODE=direct`)
- `HOSTCTL_NET_CYCLES=10`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
- `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`
- promoted defaults under test:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`

Observed:

- summary: `avg_upload_s=3.47`, `avg_kib_s=147.71`, `total_s=68.75`
- per-cycle throughput (`n=10`): mean/stddev `147.72 ± 3.89 KiB/s`
  (min/max `136.57 / 150.59`)
- warmed-cycle view (`cycles 2..10`): `148.95 ± 1.21 KiB/s`
- failure class / guard signal:
  - no `HOST_FAILURE` markers
  - no listener-not-ready or host health/send-failure markers
  - `req_read_body_reset delta=0` across cycles

Interpretation:

- promoted ingress fairness defaults stay stable through bounded soak with no
  new host/device failure-class regressions.
- the known first-cycle startup/listener outlier remains visible, while
  steady-state cycle variance is low.
