# Upload Throughput History

Regression command shape used for comparison:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-510 \
WIFI_UPLOAD_CYCLES=1 \
WIFI_UPLOAD_PAYLOAD_BYTES=65536 \
WIFI_UPLOAD_SSID='<wifi-ssid>' \
WIFI_UPLOAD_PASSWORD='<wifi-password>' \
scripts/test_wifi_upload_regression.sh
```

## Baseline Before Persistent Append Session

- Firmware commit: `b1d42bf`
- Date: `2026-02-26`
- Log: `logs/wifi_upload_regression_20260226_164205.log`
- Result:
  - `payload_bytes=65536`
  - `upload_ms=45212`
  - `throughput_kib_s=1.42`
  - `connect_ms=6165`
  - `listen_ms=6165`

## After Persistent Append Session

- Firmware commit (working tree): `session-based append in sdcard::fat + sd_task upload integration`
- Date: `2026-02-26`
- Log: `logs/wifi_upload_regression_20260226_170758.log`
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
ESPFLASH_PORT=/dev/cu.usbserial-510 \
WIFI_UPLOAD_CYCLES=1 \
WIFI_UPLOAD_SSID='<wifi-ssid>' \
WIFI_UPLOAD_PASSWORD='<wifi-password>' \
scripts/test_wifi_upload_regression.sh
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
