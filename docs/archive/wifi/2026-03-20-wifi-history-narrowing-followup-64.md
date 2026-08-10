# 2026-03-20 Wi-Fi History Narrowing Follow-up 64

## Objective

Reapply the follow-up 43 binary-patch seam on the current instrumented build by keeping the trampoline symbols in the ELF and locating the equivalent direct local call sites.

## Changes

- `src/firmware/storage/upload/wifi/connect/wdev_branch_wrap_diag.rs`
  - moved trampolines into `.wifi0iram` (so the linker script collects them)
  - made the four trampolines public and exported
  - exported `keep_wdev_branch_trampolines` so it can be forced by the linker
- `build.rs`
  - added `--undefined=` link args for the trampoline symbols and keep function

## Verification

Trampoline symbols now exist in the final ELF:

- `keep_wdev_branch_trampolines`: `0x400d8378`
- `wdev_process_panic_watchdog_trampoline`: `0x400835dc`
- `lmac_process_rx_suc_data_trampoline`: `0x40083620`
- `pp_post_trampoline`: `0x40083664`
- `wdev_process_rx_suc_data_trampoline`: `0x400836cc`

`wDev_ProcessFiq` direct local call to `wdev_process_panic_watchdog` still exists:

- call site `0x4008e4aa: call8 0x4008e3c8 <wdev_process_panic_watchdog>`

## Current Blocker

The follow-up 43 patch target no longer exists in this build:

- `wDev_ProcessFiq` now calls `__wrap_lmacProcessRxSucData` via `callx8` (indirect), not a direct local `call8`.
- `lmacProcessRxSucData` no longer contains a direct local `pp_post(...)` call.
- All remaining direct `call8` sites to `pp_post` are in unrelated TX/queue helpers (e.g. `ppTxPkt`, `ppMapWaitTxq`, `ppRegressAmpdu`) and do not show a constant `a10=25` setup.

Because of that, the exact follow-up 43 binary patch (three direct local call sites) cannot be reapplied on the current ELF.

## Next Step

Decide on the new patch seam:

- either patch the direct local `wdev_process_panic_watchdog` call only (still present), or
- identify the new RX-side consumer that emits `pp_post(25, ...)` in this build and patch there instead.
