# Wi-Fi History Narrowing Followup 56 (2026-03-20)

## Goal

Reconfirm previously rejected diagnostics with known 2.4 GHz APs present (<test-ssid-guest> / <test-ssid-primary>).

## Run A: Boot-scan-only promiscuous sweep

Environment:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`

Results:

- Promisc windows logged zero packets on all channels (8/1/6/11):
  - `boot_scan_only_promisc_diag window ... total=0 mgmt=0 ctrl=0 data=0 misc=0`
- Scan done list still empty (`count=0`, `head_ptr=0x0`).
- IDF compare still reports `ap_num=0` / `records_returned=0`.

Log: `logs/flash_capture_20260320_boot_scan_promisc/capture.log`

## Run B: Country override

Environment:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_COUNTRY_US_OVERRIDE=1`

Results:

- Country override applied (driver state shows `cc=US` and `nchan=13`).
- Scan done list still empty (`count=0`, `head_ptr=0x0`).
- IDF compare still reports `ap_num=0` / `records_returned=0`.

Log: `logs/flash_capture_20260320_country_override/capture.log`

## Implication

Even with 2.4 GHz APs present, both the promisc sweep and US country override remain at zero packets and zero scan results, reinforcing a deeper RX ingress blackout rather than scan config or country gating.

## Next Step

Given repeated zero RX across promisc, scan, and IDF compare paths, the next high-value step is to instrument the lower RX event ingress path (e.g., MAC/PP RX) or validate RF/antenna/hardware state (when JTAG arrives) to confirm whether RX interrupts and packet buffers are firing at all.
