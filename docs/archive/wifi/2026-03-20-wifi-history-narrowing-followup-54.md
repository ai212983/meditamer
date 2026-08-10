# Wi-Fi History Narrowing Followup 54 (2026-03-20)

## Goal

Validate the binary-patch sniffer trampoline (no wrap) and compare scan-done list state against IDF compare on the same boot-scan run.

## Build + Patch

- Rebuilt with `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1` after adding a hard keep for `wdev_sniffer_probe_trampoline`.
- ELF symbol for trampoline after rebuild: `wdev_sniffer_probe_trampoline` at `0x400d7f18`.
- `.rwtext.wifi` section base: `VA 0x400835a4`, file offset `0x0425a4`.
- Patched literal pool word at `0x40089434` (file offset `0x048434`) to `0x400d7f18`.
  - Before: `02 99 71 d9`
  - After: `18 7f 0d 40`
- Patched image: `logs/flash_capture_20260320_sniffer_probe_patch/meditamer-sniffer-probe.elf`.

## Capture

- Flash-capture directory: `logs/flash_capture_20260320_sniffer_probe_patch/`.
- Note: hostctl treated `--log capture_long.log` as an artifact directory and still wrote to `capture.log`.

## Results

- `scan_done_list` events present, but `count=0`:
  - `upload_http: event scan_done_list status=0 count=0 ...`
- IDF compare ran and returned zero APs:
  - `upload_http: boot_scan_only_diag idf_compare=ok ... ap_num=0 records_returned=0`
- Sniffer trampoline did not record any entries:
  - `upload_http: boot_scan_only_diag wdev_sniffer_probe ... count=0`

## Implication

- Either the environment had zero visible APs during the boot scan, or the `wDev_SnifferRxData` call path is not exercised by this scan flow (so the literal patch never fires).

## Next Step

- Confirm whether the scan environment should have APs. If yes, move instrumentation to a guaranteed scan path (for example `scan_process`/`scan_done` handling or other RX-success hooks) rather than `wDev_SnifferRxData`.
- If no APs are expected, bring up a known AP (phone hotspot) and re-run the same patched image to validate that scan results populate and the sniffer probe can trigger.
