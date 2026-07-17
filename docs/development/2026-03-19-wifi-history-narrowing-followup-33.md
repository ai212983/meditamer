## 2026-03-19: Comparator top-half sees RX/scan event masks that the app never sees

### New artifacts

- `logs/flash_capture_20260319_legacy_irqmask/capture.log`
- `logs/flash_capture_20260319_104821/capture.log`
- `logs/flash_capture_20260319_105347/capture.log`

### What changed

- Added the same top-half interrupt/RX wrappers to the working legacy comparator that were already running on the app build.
- Persisted the comparator wrapper rings through `after_idf_explicit_compare`, `after_scan`, and `steady` so late monitor attach still captures the IRQ-path evidence.

### What is now proven

1. The working legacy comparator sees a materially richer MAC-interrupt event family than the app.

- App top-half only observed:
  - `0x00000800`
  - `0x00000000`
- Comparator top-half observed and cleared:
  - `0x01000020`
  - `0x00800000`
  - `0x00000080`
  - plus `0x00000000`

This is visible in `logs/flash_capture_20260319_legacy_irqmask/capture.log` under:
- `hal_mac_get_event_wrap_diag_entry`
- `hal_mac_clr_event_wrap_diag_entry`

2. The working comparator reaches the RX path that the app never reaches.

In the comparator capture:
- `lmac_rx_suc_wrap_diag ... count=8`
- `lmac_rx_done_wrap_diag ... count=8`
- `ppenq_wrap_diag ... count=8`

The app-side captures still showed:
- `hal_mac_rx_end_wrap_diag ... count=0`
- `lmac_rx_suc_wrap_diag ... count=0`
- `lmac_rx_done_wrap_diag ... count=0`
- `ppenq_wrap_diag ... count=0`

3. The comparator also posts a richer `pp_post` family than the app.

Comparator `pp_post_wrap_diag_entry` includes:
- `arg0=0x19`
- `arg0=0x11`
- `arg0=0x17`
- `arg0=0x10`
- `arg0=0x06`

The app-side failing window only showed:
- `arg0=0x06`

4. This is upstream of result-list materialization.

The comparator-side IRQ path differences appear before the app/comparator split in list linking and AP visibility. That means the current live boundary is no longer the scan-result list itself.

### Current boundary

The surviving boundary is now:
- after ISR entry into `wDev_ProcessFiq`
- before RX-end handling and `ppEnqueueRxq`
- specifically in the MAC interrupt event family delivered to the top half

More concretely:
- app receives only the watchdog-style `0x00000800` event family
- working comparator receives RX/scan-related event masks that drive `lmacProcessRxSucData`, `lmacRxDone`, and `ppEnqueueRxq`

### What this closes

- “`wDev_ProcessFiq` is not running on the app”
- “top-half IRQ path is equivalent and the split is later in scan result retrieval”
- “result-list linking is the first live app/comparator split”

### Best next step

Stop probing scan-result consumers.

The next useful step is to compare the MAC interrupt source/mask setup that feeds `hal_mac_interrupt_get_event()` on app vs comparator, and determine why the app never receives the comparator’s RX/scan event family.
