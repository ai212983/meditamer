# Wi-Fi history narrowing follow-up 58 (setup stage trace)

Date: 2026-03-20

## What we did
- Rebuilt with `MEDITAMER_WIFI_SETUP_STAGE_TRACE=1` and `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`.
- Captured boot log in `logs/flash_capture_20260320_112007/capture.log`.

## Observations
- Setup stage trace appears and completes `setup.begin`, `embassy_net_new.before`, `embassy_net_new.after`.
- No panic observed in this run.
- Scan still returns empty results: `scan_done_list count=0` for scan IDs 128/129 and `idf_compare=ok ... ap_num=0`.
- `hal_mac_get_event_wrap_diag_ext` continues to return `0x00000800`/`0x00000000` pattern with unchanged pre/post window snapshots.
- `pre_words[1]` in the HAL event window snapshots now shows `0x0004881c/0x0004801c` for later entries in the sequence.

## Interpretation
- The stack-guard panics seen in earlier captures appear to be tied to the flashed image rather than a persistent hardware fault.
- Wi-Fi boot scan still never surfaces any APs, despite the MAC ISR event window showing activity.

## Next steps (proposed)
- Expand the MAC event window snapshot outside ISR context to include words 6-11 so we can compare additional registers across stages.
