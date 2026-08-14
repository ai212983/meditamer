## 2026-03-19: Static references to the MAC event latch are limited to read/clear helpers and init reset; there is no remaining cheap source-level producer to inspect

### New artifacts

- `docs/development/2026-03-19-wifi-history-narrowing-followup-36.md`
- `logs/flash_capture_20260319_104821/capture.log`
- `logs/flash_capture_20260319_105347/capture.log`
- `logs/flash_capture_20260319_legacy_irqmask/capture.log`

### What changed

- Enumerated all static references to the MAC interrupt event latch addresses:
  - `0x3ff73c48`
  - `0x3ff73c4c`
- Disassembled the nearby non-top-half references in both the current app and working comparator.

### What is now proven

1. Both images only reference `0x3ff73c48` in the top-half event reader.

Current app:
- `0x4008e058 <hal_mac_interrupt_get_event>` reads `0x3ff73c48`

Working comparator:
- `0x4008b47c <hal_mac_interrupt_get_event>` reads `0x3ff73c48`

No additional static references to `0x3ff73c48` were found in either image.

2. Both images reference `0x3ff73c4c` only in clear/init paths.

Current app references:
- `0x4008e06c <hal_mac_interrupt_clr_event>` writes `0x3ff73c4c`
- `0x4013f77b <hal_deinit>` writes `-1` to `0x3ff73c4c`
- `0x4013f9b9 <hal_init>` writes `-1` to `0x3ff73c4c`

Working comparator references:
- `0x4008b490 <hal_mac_interrupt_clr_event>` writes `0x3ff73c4c`
- `0x40114719 <hal_deinit>` writes `-1` to `0x3ff73c4c`
- `0x4011495c <hal_init>` writes `-1` to `0x3ff73c4c`

3. Those init/deinit paths are also effectively identical old vs current.

The nearby `hal_deinit` / `hal_init` sequences are the same in meaning:
- reset related MAC state
- write `-1` to the clear latch at `0x3ff73c4c`
- continue through normal HAL init

So they do not explain why runtime event bits diverge later.

4. This leaves no cheap source-level producer to inspect.

At this point, the static code evidence says:
- the top-half reader is the same
- the top-half clearer is the same
- init/deinit reset of the clear latch is the same
- the early mask logic in `wDev_ProcessFiq` is the same

But runtime still differs:
- app only sees `0x00000800`
- comparator sees the richer RX/scan event family

So the live split is not exposed by remaining source-level references to the event latch itself.

### Current boundary

The surviving boundary is now below everything we can cheaply inspect in source/disassembly around the event latch.

It is specifically in:
- runtime production of the event word
- or runtime admission/masking of RX/scan bits before they ever become visible at `0x3ff73c48`

### What this closes

- “there is another obvious static writer of `0x3ff73c48` or `0x3ff73c4c` we have not inspected yet”
- “the remaining difference is likely in the top-half reader/clear helper neighborhood”
- “another easy objdump pass around the latch addresses will probably expose the cause”

### Best next step

This branch is now blocked on deeper runtime instrumentation, not more source comparison.

The next useful work must be one of:
- binary-patch or breakpoint-style instrumentation at the hardware/blob-facing producer of the event word
- hardware register/state capture around the MAC interrupt source before `wDev_ProcessFiq`
- or a deliberate experimental pivot back to ISR wake semantics with full acknowledgment that the novelty gate already says those easy wrapper experiments were not sufficient in main firmware
