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
