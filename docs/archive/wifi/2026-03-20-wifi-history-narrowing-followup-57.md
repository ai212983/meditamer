# Wi-Fi history narrowing follow-up 57 (deep event window)

Date: 2026-03-20

## What we did
- Parsed `logs/flash_capture_20260320_deep_event_window/capture.log` for the new MAC event window instrumentation.
- Mapped `wifi_mac_isr_target` pointer via `rust-objdump` to identify the ISR target symbol.

## Observations
- `scan_done_list` still reports `count=0` for both scan IDs; `idf_compare=ok` shows `ap_num=0`.
- `wifi_mac_isr_target` is set to `0x400db5dc`, which resolves to `__wrap_wDev_ProcessFiq` in the current debug ELF.
- `hal_mac_interrupt_get_event` returns a repeating pattern (`0x00000800` then `0x00000000`) and the MAC event window words remain identical pre/post for every captured entry.
- `wdev_fiq_wrap_diag` entries show `arg0=0` and `pre_mac_isr=0` across all snapshots.

## Interpretation
- The MAC ISR target is installed, but the only observable event bit is `0x00000800` with no RX callbacks; the MAC event window appears static across interrupts.
- This supports the hypothesis that the RX path is not receiving frames at all, rather than the scan results being dropped later in software.

## Next steps (proposed)
- Go deeper on the MAC event window by expanding the captured word range to see if any adjacent registers change during the interrupts.
