# 2026-03-19 Wi-Fi History Narrowing Follow-up 45

## Objective
Continue the no-hardware path with static comparison only around the first revived RX consumer seam:

- `wdevProcessRxSucDataAll`
- `wDev_ProcessRxSucData`

The goal is to isolate a concrete code-level delta that can justify a future selective binary patch without adding another trampoline layer.

## Baseline
Follow-up 43 proved that restoring the direct-local handoff
`lmacProcessRxSucData -> pp_post(25, ...)`
recovers green behavior.

Follow-up 44 proved that adding any direct-local trampoline one call deeper at
`wdevProcessRxSucDataAll -> wDev_ProcessRxSucData`
collapses the branch back to the empty-list failure.

So the static comparison target here is the code just above that seam.

## Symbol Sizes
Current app:
- `ppTask = 0x40085198` size `0x1fb`
- `wDev_ProcessRxSucData = 0x400894d0` size `0x38c`
- `wdevProcessRxSucDataAll = 0x40089874` size `0x0f3`
- `pp_post = 0x4008e100` size `0x138`
- `lmacProcessRxSucData = 0x4008e268` size `0x04b`

Comparator:
- `ppTask = 0x40083408` size `0x1d3`
- `wDev_ProcessRxSucData = 0x40086b54` size `0x2f5`
- `wdevProcessRxSucDataAll = 0x40086e64` size `0x0e7`
- `pp_post = 0x4008afe0` size `0x136`
- `lmacProcessRxSucData = 0x4008aefc` size `0x04b`

The main size delta remains in `wDev_ProcessRxSucData`, but the most actionable code split appears one level earlier in `wdevProcessRxSucDataAll`.

## Shared Structure in `wdevProcessRxSucDataAll`
App and comparator still share the broad shape:
1. call `hal_mac_rx_get_last_dscr`
2. gate on `g_wdev_last_desc_reset` and the returned descriptor pointer
3. enter a loop over RX descriptors
4. test descriptor bit 30
5. call `wDev_ProcessRxSucData`
6. reacquire `hal_mac_rx_get_last_dscr`
7. perform a second gate before either:
   - looping again
   - or returning to the caller

So the function family is still structurally aligned.

## Concrete Static Delta: Second-Pass Gate
The strongest code-level split is the second gate after the inner call to `wDev_ProcessRxSucData`.

### App
Relevant window in the current app ELF:

- `4008991b: call8 40089b44 <hal_mac_rx_get_last_dscr>`
- `4008991e: mov.n a7, a10`
- `40089920: movi.n a10, 1`
- `40089922: l32r a5, ... <g_wdev_last_desc_reset>`
- `40089925: l8ui a9, a5, 0`
- `40089928: xor a9, a9, a10`
- `4008992b: extui a9, a9, 0, 8`
- `40089930: beqz.n a9, 40089957`
- `40089934: bnez.n a7, 40089957`

Interpretation:
- the app uses the freshly reacquired descriptor only to update `a7`
- then gates on:
  - `g_wdev_last_desc_reset ^ 1`
  - and whether `a7 != 0`
- so the second-pass decision depends directly on the new descriptor pointer stored in `a7`

### Comparator
Relevant window in the comparator ELF:

- `40086f08: call8 40087028 <hal_mac_rx_get_last_dscr>`
- `40086f0b: movi.n a12, 1`
- `40086f0d: movi.n a9, 0`
- `40086f0f: mov.n a2, a10`
- `40086f11: moveqz a9, a12, a10`
- `40086f14: l8ui a10, a5, 0`
- `40086f19: bgeu a10, a9, 40086f3b`

Interpretation:
- the comparator derives a threshold from the fresh reacquired descriptor:
  - `a9 = 1` only when `a10 == 0`
  - otherwise `a9 = 0`
- then compares the `g_wdev_last_desc_reset` byte against that fresh-result-derived threshold

### Practical Difference
The app and comparator are not implementing the same second-pass condition.

The comparator’s second gate is explicitly driven by the fresh result of the second
`hal_mac_rx_get_last_dscr()` call.

The app’s second gate is shaped differently and uses a direct boolean pair:
- `g_wdev_last_desc_reset ^ 1`
- `a7 != 0`

This is the cleanest static delta found so far above the too-sensitive
`wdevProcessRxDataAll -> wDev_ProcessRxSucData` seam.

## Static Delta in `wDev_ProcessRxSucData`
`wDev_ProcessRxSucData` is still materially larger on the app side (`0x38c` vs `0x2f5`).

Both variants still route through the same broad outcomes:
- discard path
- frame indication path
- AMPDU indication path
- multiple `ic_interface_enabled` guards

But the function is too large and branch-heavy to claim a single decisive static split yet.
Without a stable runtime seam one call deeper, the strongest actionable delta remains the second-pass gate in `wdevProcessRxSucDataAll`.

## Current Best Interpretation
The highest-value no-hardware candidate is now:
- the second-pass rescan/retry gate in `wdevProcessRxSucDataAll`
- specifically how the fresh `hal_mac_rx_get_last_dscr()` result is consumed in app vs comparator

This is more precise than the previous broad statement “somewhere below `pp_post(25)`”.

## Recommended Next Step
If continuing without JTAG, prefer a single selective binary patch around the app second-pass gate in `wdevProcessRxSucDataAll` rather than another trampoline.

The right target is the small condition block after the second `hal_mac_rx_get_last_dscr()` call, not the deeper `wDev_ProcessRxSucData` call itself.
