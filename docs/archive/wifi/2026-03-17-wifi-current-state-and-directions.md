# 2026-03-17 Wi-Fi Current State And Directions

## Goal

Recover stable on-device Wi-Fi discovery in the main app firmware on the ESP32
target without regressing the rest of the application.

## Current Defensible State

### Historical fact

- The main app historically worked on the newer no-std stack on 2026-03-05.
- The historical green window is recorded in:
  - `docs/development/upload-throughput-history/part-13.md`
- The first recorded zero-discovery evidence is on 2026-03-06, not a new green
  window:
  - `docs/development/upload-throughput-history/part-16.md`
- The historical main app dependency set at that time was already:
  - `esp-hal 1.0.0`
  - `esp-rtos 0.2.0`
  - `esp-radio 0.17.0`
  - `esp-wifi-sys 0.8.1`

### Current reproducible reality

- The current main app on the same broad stack generation is reproducibly dark:
  - no pre-scan promisc traffic
  - direct explicit scan fails with `scan_rc=12300`
  - wrapped scan returns zero or times out
- Rebuilding the historical main-app rollback anchor today does not restore the
  old working state.
- `meditamer_march_5` builds, flashes, and currently passes bounded discovery
  today.
- `meditamer_march_6` builds and flashes today, but its runtime state is no
  longer stable across repeated validation:
  - earlier on 2026-03-17 it reproduced zero-discovery
  - later on 2026-03-17 the pristine March 6 snapshot became green again under
    the same bounded discovery workflow
- The standalone legacy comparator still works today on the same device with:
  - `esp-wifi 0.15.1`
  - `esp-wifi-sys 0.7.1`

### Second-device control

- A second similar device was tested today.
- Earlier on that second device:
  - `meditamer_march_6` failed discovery
  - the standalone legacy comparator still worked
- Later on 2026-03-17, both boards were rechecked with pristine March 6 and
  both stayed green under repeated one-round reruns.
- This means the March 6 dark state is not currently reproducible enough for
  direct file-level reduction or erase-based board-state comparison.

## What Has Been Ruled Out

The records already reject these as primary causes:

- board / RF environment
- generic ESP-IDF Wi-Fi behavior
- PSRAM as the primary cause
- Rust scan-wrapper config alone
- direct-vs-wrapped scan-call selection
- simple legacy wrapper porting onto `esp-wifi-sys 0.8.1`
- repeated Rust-side legacy wrapper/runtime/event/global-table shims inside
  `esp-radio 0.17.0`

Primary references:

- `docs/development/wifi-upload-decision-ledger.md`
- `docs/development/upload-throughput-history/part-16.md`
- `docs/development/upload-throughput-history/part-22.md`
- `docs/development/wifi-legacy-old-stack-blob-compatibility-plan.md`
- `logs/march6_wifi_discovery_debug_20260317_102325.log`
- `logs/flash_capture_second_device_legacy_comparator_20260317_103004/capture.log`

## Latest Boundary

There are now two separate boundaries:

- the current main app is still dark at the explicit scan-start transition
- the March 5 vs March 6 historical snapshot boundary is currently unstable and
  cannot be treated as a deterministic dark-vs-green split

### Main-app scan-start boundary

Immediately before `esp_wifi_scan_start(..., true)`, the main app and working
legacy comparator match on the visible probed state:

- `blob_scan`
- `blob_scan_globals`
- `blob_chm` prestart state
- `blob_sta` prestart state
- obvious `connect_scan_flag` / pending/busy-adjacent fields

Immediately after the same explicit scan-start call:

- Working legacy comparator:
  - returns `scan_rc=0`
  - enters the expected completion-family queue path
  - keeps normal `g_chm` / `g_scan` progression
- Main app:
  - returns `scan_rc=12300`
  - mutates into a different `g_chm` / `g_scan` state immediately
  - does not enter the comparator's completion-family queue path

This places the live mismatch inside or below the scan-start implementation
path itself, not in the Rust wrapper above it.

### March 5 vs March 6 repro boundary

- `meditamer_march_5` is green today under bounded capture.
- `meditamer_march_6` was dark earlier today, but pristine March 6 later became
  green again under repeated bounded captures.
- additional repro-gate slices completed later on 2026-03-17:
  - March 6 after March 5 predecessor, TIMESET off: green
  - March 6 after legacy-comparator predecessor, TIMESET off: green
  - March 6 with TIMESET applied before capture: not dark
  - March 6 with delayed capture on the same device: green
  - March 6 on the other similar device: dark across the bounded gate
  - later recheck of that same formerly dark board on pristine March 6: green
    across three one-round reruns
- `prepare_scan.rs` and `prepare_start.rs` reductions did not isolate a stable
  regression because the dark March 6 baseline stopped reproducing.
- The next requirement is a repro gate, not more March 5 -> March 6 file
  reduction.

## What Today Strengthened

### `esp-wifi-sys` generation boundary

The history already pointed at the old-vs-new sys generation split:

- working legacy generation:
  - `esp-wifi 0.15.1` + `esp-wifi-sys 0.7.1`
- failing newer generation:
  - `esp-radio 0.16.x+` / `0.17.0` + `esp-wifi-sys 0.8.1`

Today strengthened that with direct archive-level evidence:

- `0.8.1` carries a different ESP32 Wi-Fi archive set than `0.7.1`
- `0.8.1` adds `libregulatory.a`
- `esp_wifi_scan_start` and related scan/channel objects differ materially
  across generations

### Config ABI difference is real, but not sufficient

`wifi_scan_config_t` differs between `0.7.1` and `0.8.1`, but probing the old
layout in the main app did not fix the scan-start failure.

Conclusion:

- the struct ABI difference is real
- it is not the root cause by itself

### March 6 snapshot is not a stable reduction control

`meditamer_march_6` is useful because it is buildable and flashable today, but
it is not currently a deterministic dark control:

- it reflects a date where recorded blackout evidence already exists
- it earlier reproduced zero-discovery on hardware today
- it later passed bounded discovery again on the same workflow and device

So the current March 6 problem is not "how to reduce a stable dark snapshot".
It is "why the March 6 runtime outcome drifted from dark to green under current
validation conditions".

## Practical Meaning

The remaining problem is no longer best described as:

- "wrong scan wrapper"
- "wrong explicit scan config"
- "missing legacy shim"

It is best described as:

- a deeper mismatch in the newer linked Wi-Fi/blob generation, especially
  around scan-start / channel-manager / regulatory behavior
- and/or some historical build/runtime/substrate state that is no longer being
  recreated by source snapshot alone
- plus an unresolved March 6 validation drift problem that must be understood
  before file-level reduction is trustworthy

## Viable Directions

### Direction 1: Recovery-first

Treat the standalone legacy comparator as the only reproducibly working Wi-Fi
baseline and move toward a substrate that preserves its behavior.

This means:

- prefer the old working substrate direction over more `0.8.1` wrapper shims
- if same-image remains mandatory, continue only if the app can be made to use
  a truly old-compatible Wi-Fi substrate rather than a partial compatibility
  layer on top of `0.8.1`

### Direction 2: Root-cause archaeology

If precise proof is needed before changing direction, compare the linked ESP32
Wi-Fi objects directly between `0.7.1` and `0.8.1`, focusing on:

- `ieee80211_api.o`
- `ieee80211_ioctl.o`
- `ieee80211_scan.o`
- `wl_chm.o`
- `libregulatory.a`

The target is the code path that drives:

- `g_chm.op_chan`
- `g_chm.current_chan`
- `g_scan.word_00`
- the immediate `scan_rc=12300` branch

### Direction 3: Historical snapshot validation

Use the historical snapshots only after a strict repro gate is established.

Current note:

- `../meditamer_march_5` is complete enough to build, flash, and validate
- `../meditamer_march_5` is currently green
- `../meditamer_march_6` is complete enough to build, flash, and validate
- `../meditamer_march_6` is currently green again on both boards, despite
  earlier dark runs
- so the immediate task is to reproduce the dark runtime state again, not to
  continue blind source reduction

## Recommendation

Do not spend more time on Rust scan-wrapper fields or narrow compatibility
shims on top of `esp-wifi-sys 0.8.1`.

The two credible next moves are now:

1. establish a strict March 6 repro gate before any more March 5 -> March 6
   reduction
2. continue deeper current-main-app scan-start investigation independently of
   the historical snapshot drift

Current repro-gate status:

- predecessor image does not currently explain the March 6 drift
- TIMESET does not currently explain the March 6 drift
- delayed capture does not currently explain the March 6 drift
- the earlier dark-board repro no longer holds on recheck
- March 6 is currently green on both boards under repeated one-round reruns
- March 6 cannot currently be used as a reduction baseline at all

The strongest hardware-backed matrix we now have is:

- current / newer-stack main-app path: dark
- `meditamer_march_5`: green
- `meditamer_march_6`: green on both available boards at the latest recheck
- standalone legacy comparator: working
- standalone legacy comparator working was reproduced on a second device

## Latest Shift

The strongest new result from March 17 is that a dark state can be induced on
board `08:3a:8d:82:0b:98` with:

- `meditamer_march_5` predecessor
- then pristine `meditamer_march_6`

Once active, that dark state currently survives:

- March 6 -> March 5 source rollback
- full `prepare_scan.rs` rollback inside March 6
- full flash erase followed by March 5 reflash

That means the active fault domain is broader than a March 6 source diff and
broader than ordinary flash persistence alone.

## Related Records

- `docs/development/wifi-upload-decision-ledger.md`
- `docs/development/upload-throughput-history/part-13.md`
- `docs/development/upload-throughput-history/part-16.md`
- `docs/development/upload-throughput-history/part-22.md`
- `docs/development/wifi-legacy-old-stack-blob-compatibility-plan.md`
- `docs/development/wifi-regression-isolation-plan.md`
- `docs/development/wifi-true-old-stack-backend-plan.md`
- `docs/development/2026-03-17-wifi-march5-march6-regression-investigation-plan.md`
- `docs/development/2026-03-17-wifi-march6-repro-gate-plan.md`
