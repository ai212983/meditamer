# Wi-Fi History Narrowing Followup 55 (2026-03-20)

## Goal

Re-run boot-scan diagnostics with known APs present (<test-ssid-guest> / <test-ssid-primary>) and confirm whether APs populate in scan-done list or IDF compare.

## Runs

### Run A: APs present, default boot scan timing

Environment:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG=1`

Results:

- `scan_done_list` still reports `count=0` and `head_ptr=0x0`.
- `boot_scan_only_diag idf_compare=ok ... ap_num=0 records_returned=0`.
- `wdev_sniffer_probe` remains `count=0` across stages.

### Run B: APs present, compat071 scan config

Environment:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPAT071=1`

Results:

- Same outcome: `scan_done_list count=0`, `idf_compare ap_num=0`, `wdev_sniffer_probe count=0`.

### Run C: APs present, IDF log bump

Environment:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG=1`
- `MEDITAMER_WIFI_FIRST_START_IDF_LOG_DIAG=1`

Results:

- Same outcome as Run A (no APs in scan-done or IDF compare; sniffer probe still zero).

## Implication

Even with known APs present and with default scan timing and compat071 config, the scan result list remains empty and RX callbacks stay at zero. That points to a deeper RX-ingress blackout rather than a scan-wrapper or config-struct issue.

## Next Step

- Confirm the APs are 2.4 GHz (ESP32 cannot see 5 GHz) and not hidden. If they are 5 GHz only, we need a 2.4 GHz AP/phone hotspot for a valid check.
- If APs are confirmed 2.4 GHz and in range, decide whether to explicitly re-run previously rejected diagnostics (for example `MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG` or `MEDITAMER_WIFI_COUNTRY_US_OVERRIDE`) with user reconfirmation.
