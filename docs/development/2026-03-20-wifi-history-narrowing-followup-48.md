# 2026-03-20 Wi-Fi History Narrowing Follow-up 48

## Objective

Continue the selective no-hardware patch strategy from follow-up 47 by testing the
next app-only gate in the early `wDev_ProcessRxSucData` prelude.

The target was the extra discard check on `a9 + 48` in the app-only `65 / 98`
special-case path:

- `400894f1: l8ui a4, a9, 48`
- `400894f4: beqz a4, discard`

The specific goal was to force that gate nonzero while leaving the rest of the
prelude intact.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is still the proven-green follow-up 43 control

Selective patch applied:

- function: `wDev_ProcessRxSucData`
- address: `0x400894f1`
- original instruction:
  - `400894f1: 300942     l8ui a4, a9, 48`
- patched instruction:
  - `400894f1: ffa042     movi a4, 255`

Practical effect:

- before:
  - the app-only special-case path discards when byte `a9 + 48` is zero
- after:
  - the following `beqz a4, discard` can no longer take that extra discard path

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_rxprelude_byte48_force_patch.elf`

Verification:

- `400894f1: ffa042     movi a4, 255`

Flash/capture artifact:

- `logs/flash_capture_20260320_rxprelude_byte48_force_real/capture.log`

## Result

This patch collapses the green branch back to the stable empty-list failure.

Observed in `logs/flash_capture_20260320_rxprelude_byte48_force_real/capture.log`:

- first and only `ScanDone` is empty:
  - `event scan_done_list status=0 count=0 scan_id=128 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffccbd4`
- explicit scan still completes:
  - `idf_explicit_compare_postcall=postcall scan_rc=0 ... scan_done_count=1 scan_done_ap_num=0`
- pre-retrieval list state is empty:
  - `scan_list_snapshot label=event_post_before_get_ap_num scannum=0x0000 head_ptr=0x00000000`

Important surviving signals:

- revived RX-delivery counters are still present:
  - `wdev_binary_patch_counts after=scan_done watchdog_count=8 lmac_rx_suc_count=8 pp_post_arg25_count=13`
- so this patch does not collapse the branch all the way back to a pre-RX state
- it specifically collapses list materialization back to the empty-list branch

## Interpretation

This is stronger than follow-up 47.

What is now proven:

1. The app-only `a9 + 48` discard gate is live.
2. Forcing that gate nonzero is not harmless.
3. It is strong enough to destroy the green result-list branch while preserving
   early RX-delivery activity (`lmac_rx_suc_count=8`, `pp_post_arg25_count=13`).

That means this app-only special-case subpath is not just correlated noise.
It materially controls whether the branch progresses from restored RX delivery
into actual AP-list materialization.

## Current Narrowed Boundary

The strongest remaining no-hardware target inside the app-only prelude is now
centered on the special-case path around:

- `0x400894e9..0x400894f4`
- especially the interaction between:
  - the `65 / 98` special-case classification
  - the `a9 + 48` extra gate
  - the later shared classification body

What is now closed:

- `wdevProcessRxSucDataAll` second-pass gate as primary
- the earlier `mov.n a4, a7` branch as sole cause

What remains strongest:

- the deeper app-only special-case gating in `wDev_ProcessRxSucData`

## Recommended Next Step

If continuing without JTAG:

1. keep the follow-up 43 green control intact
2. avoid blanket forcing of the `a9 + 48` gate, since it destroys the green
   branch
3. target the remaining special-case classifier instead, especially the
   `a12 == 65 / 98` selection path feeding this gate
