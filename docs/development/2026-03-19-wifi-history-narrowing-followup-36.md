## 2026-03-19: Top-half MAC interrupt reader/clear code is effectively identical old vs current; the live split is the runtime event bits, not the top-half code shape

### New artifacts

- `logs/flash_capture_20260319_104821/capture.log`
- `logs/flash_capture_20260319_105347/capture.log`
- `logs/flash_capture_20260319_legacy_irqmask/capture.log`
- `docs/development/2026-03-19-wifi-history-narrowing-followup-35.md`

### What changed

- Disassembled the current app and working comparator release images around:
  - `wDev_ProcessFiq`
  - `hal_mac_interrupt_get_event`
  - `hal_mac_interrupt_clr_event`
- Compared the early event-mask handling in the top-half ISR path.

### What is now proven

1. `hal_mac_interrupt_get_event()` is effectively identical old vs current.

Current app:
- `0x4008e058 <hal_mac_interrupt_get_event>`
- loads a word from `0x3ff73c48`
- returns it directly

Working comparator:
- `0x4008b47c <hal_mac_interrupt_get_event>`
- loads a word from `0x3ff73c48`
- returns it directly

2. `hal_mac_interrupt_clr_event()` is effectively identical old vs current.

Current app:
- `0x4008e06c <hal_mac_interrupt_clr_event>`
- stores the provided value to `0x3ff73c4c`

Working comparator:
- `0x4008b490 <hal_mac_interrupt_clr_event>`
- stores the provided value to `0x3ff73c4c`

So the event reader/clear path is not where the app/comparator split is introduced.

3. The first event-mask gates in `wDev_ProcessFiq` are also equivalent in meaning.

Current app `wDev_ProcessFiq`:
- reads event word
- exits if zero
- clears the event word
- checks watchdog bit 11
- checks the `0x00600000` family for the `pp_post(..., 14, 1)` path
- then checks the broader RX/scan family via `0x01000024`

Working comparator `wDev_ProcessFiq`:
- reads event word
- exits if zero
- clears the event word
- checks watchdog bit `0x00000800`
- checks the same `0x00600000` family for the `pp_post(..., 14, 1)` path
- then checks the broader RX/scan family via `0x01000024`

The instruction form differs slightly, but the early mask logic is the same.

4. That means the earlier live runtime split remains the right one.

Runtime logs still show:
- app top-half only sees:
  - `0x00000800`
  - `0x00000000`
- comparator top-half sees and clears:
  - `0x01000020`
  - `0x00800000`
  - `0x00000080`
  - plus `0x00000000`

Sources:
- app:
  - `logs/flash_capture_20260319_104821/capture.log`
  - `logs/flash_capture_20260319_105347/capture.log`
- comparator:
  - `logs/flash_capture_20260319_legacy_irqmask/capture.log`

### Current boundary

The surviving boundary is no longer in:
- OSI hook registration
- OSI interrupt setup helpers
- top-half event reader/clear helpers
- first-mask logic in `wDev_ProcessFiq`

It is now specifically in the runtime production/admission of the MAC interrupt event word itself.

In plain terms:
- app and comparator top-half code read the same place and branch on the same early masks
- but the app runtime only ever sees the watchdog-style `0x800` event family
- the comparator runtime receives the richer RX/scan event family that drives RX delivery

### What this closes

- “current top-half interrupt reader/clear code is broken relative to the comparator”
- “the first `wDev_ProcessFiq` mask gates explain the app/comparator split”
- “the live difference is still mainly in Rust/OSI glue”

### Best next step

The next useful step is no longer source comparison of top-half helpers.

It requires deeper runtime instrumentation below or beside the top half, aimed at:
- where the MAC event word at `0x3ff73c48` is produced
- what enables the RX/scan-related bits on the working comparator
- why those bits never appear on the current app path

That is a deeper blob/hardware-facing seam than the current wrapper method can reach cleanly.
