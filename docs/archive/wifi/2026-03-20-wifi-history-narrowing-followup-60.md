# Wi-Fi history narrowing follow-up 60 (MAC window extractor)

Date: 2026-03-20

## What we did
- Added `scripts/diag/extract_mac_event_window.sh` to extract the `mac_event_window` words into a table.
- Ran it on `logs/flash_capture_20260320_112234/capture.log` to confirm formatting.

## Output sample
```
stage                                w0         w1         w2         w3         w4         w5         w6         w7         w8         w9        w10        w11
before_diag_reset              00000000   02008000   00000000   00000000   090e2020   00000000   00000000   0fff0fff   00000000   00000000   00000000   a5000c24
after_set_mode                 00000000   06008000   00000000   00000000   090e2020   00000000   00000000   0fff0fff   00000000   00000000   00000000   a5000c24
after_start_pre_driver_state   01e839e0   0004801c   00000000   00000000   090e2020   00000404   00000000   ffff0fff   00000000   00000000   00000000   a5802d24
after_nan_timer_slot_retarget  01e839e0   0404801c   00000000   00000000   090e2020   00000404   00000000   ffff0fff   00000000   00000000   00000000   a5802d24
```

## Next step
- Use the extractor on a known-good capture and compare stage-by-stage output against the bad state.
