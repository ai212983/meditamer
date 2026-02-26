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
