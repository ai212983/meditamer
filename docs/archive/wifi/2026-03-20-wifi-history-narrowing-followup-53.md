# 2026-03-20 Wi-Fi History Narrowing Follow-up 53

## Objective

Attempt minimal live-metadata capture by wrapping `wDev_SnifferRxData` and logging
its arguments plus nearby metadata bytes.

This was the lowest-impact runtime capture option after closing all selector and
gate patches in the `wDev_ProcessRxSucData` special-case body.

## Setup

Change set:

- added `wdev_sniffer_wrap_diag` module
- enabled `--wrap=wDev_SnifferRxData` behind
  `MEDITAMER_WIFI_WDEV_SNIFFER_WRAP_DIAG=1`
- log hook added to `boot_scan_only_diag` counters

Flash/capture artifact:

- `logs/flash_capture_20260320_sniffer_wrap_diag/capture.log`

## Result

This attempt failed due to a runtime panic:

- `Detected a write to the stack guard value on ProCpu`

Backtrace excerpt (from the capture):

- `0x401781ba`
- `0x40171ce5`
- `0x4008195c`
- `0x40082e28`
- `0x40040003`
- `0x4000c2e1`
- `0x400e1760`
- `0x400e132d`
- `0x4011c6ab`
- `0x400d9c96`

## Interpretation

The `--wrap=wDev_SnifferRxData` attempt is not safe in this build shape.
It trips the stack guard before any useful diagnostic output is produced.

This means:

- wrapper-based metadata capture is not viable at this seam
- the next no-hardware step should avoid `--wrap` around this function

## Recommended Next Step

If continuing without JTAG:

1. do not reuse `MEDITAMER_WIFI_WDEV_SNIFFER_WRAP_DIAG=1` until the root cause of
   the stack-guard panic is understood
2. return to binary-patch-only approaches for metadata capture, or pause until
   JTAG is available
