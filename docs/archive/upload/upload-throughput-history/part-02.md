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

