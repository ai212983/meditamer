# 2026-03-18 Wi-Fi History Narrowing Follow-up 12

## Scope

This follow-up closes the timer-arm handoff ambiguity that remained after follow-up 11.

It answers two questions:

1. Do the paired-retarget slot timers still arm through the normal compat and esp-rtos timer path?
2. How does that path compare to the baseline app that stays in the `scan_rc=0` plus empty-results failure family?

## Artifacts

- Paired-retarget app capture with direct `esp_rtos_timer_arm` wrap:
  - `logs/flash_capture_20260318_141757/capture.log`
- Baseline app capture with the same wrapper stack:
  - `logs/flash_capture_20260318_142006/capture.log`
- Prior paired-retarget live-slot confirmation:
  - `logs/flash_capture_20260318_135309/capture.log`

## Code Changes In This Slice

- Added `esp_rtos_timer_arm` to the link-wrap list in `build.rs`.
- Added a dedicated wrapper ring in:
  - `src/firmware/storage/upload/wifi/connect/timer_arm_wrap_diag.rs`
- Wired the new wrapper ring into boot-scan diagnostics in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag/mod.rs`
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
- Enabled Xtensa asm for the app crate in `src/lib.rs`.

## Symbol Resolution

Resolved from the current app image:

- `0x4010610e` / `0x40106112` -> `esp_radio::compat::timer_compat::compat_timer_arm_us`
- `0x400fadbe` -> `esp_radio::common_adapter::ets_timer_arm`
- `0x40122e71` -> `chm_init`
- `0x40122ea1` -> `chm_init`
- baseline slot callback `0x4012279c` -> `nan_dp_schedule_ndc_start`
- paired-retarget slot callback `0x401335dc` -> `ieee80211_timer_process`

## Runtime Result

### Paired retarget

At `idf_explicit_compare_postcall` in `logs/flash_capture_20260318_141757/capture.log`:

- `scan_rc=12300`
- `blob_chm op_chan=0x01 ptr_08=0xa ptr_0c=0x14`
- `blob_scan word_00=0x0000010f word_30=0x14 word_34=0x0a`
- live slot state:
  - slot 0 callback `0x401335dc`, arg `0`, period `10000`
  - slot 1 callback `0x401335dc`, arg `1`, period `20000`
- direct esp-rtos arm ring:
  - `timer_live_arm_diag ... count=2`
  - both entries are the retargeted callback pair
  - both entries report `caller_ptr=0x800f574d`
- direct `esp_rtos_timer_arm` wrapper ring:
  - `timer_arm_wrap_diag ... count=2`
  - both entries report `caller_ptr=0x8010610e`

Interpretation:

- The paired-retarget slot timers are still armed through the normal lower timer path.
- The arm stack is:
  - `compat_timer_arm_us -> esp_rtos_timer_arm -> Timer::arm`
- The paired-retarget branch flip to `scan_rc=12300` still happens before any `ScanDone`.

### Baseline app

At `idf_explicit_compare_postcall` in `logs/flash_capture_20260318_142006/capture.log`:

- `scan_rc=0`
- `blob_chm op_chan=0xff ptr_08=0x0 ptr_0c=0x0`
- `blob_scan word_00=0x00000000 word_30=0x78 word_114=0x00000080`
- `scan_done_eventpost count=1 status=0 ap_num=0`
- live slot state:
  - slot 0 callback `0x4012279c`, arg `0`, period `10000`
  - slot 1 callback `0x4012279c`, arg `1`, period `20000`
- direct esp-rtos arm ring:
  - `timer_live_arm_diag ... count=26`
  - repeated arms of the original NAN callback pair
  - all recent entries report `caller_ptr=0x800f5751`
- direct `esp_rtos_timer_arm` wrapper ring:
  - `timer_arm_wrap_diag ... count=26`
  - all recent entries report `caller_ptr=0x80106112`
- compat wrapper arm ring:
  - `timer_compat_wrapper_arm_recent ... caller_ptr=0x80122e71` for the `10000 us` slot
  - `timer_compat_wrapper_arm_recent ... caller_ptr=0x80122ea1` for the `20000 us` slot
- compat arm ring:
  - `timer_compat_arm_recent ... caller_ptr=0x800fadbe`

Interpretation:

- Baseline repeated slot rearming is not generic background timer noise.
- It comes specifically from `chm_init` through the compat wrapper path:
  - `chm_init -> ets_timer_arm -> compat_timer_arm_us -> esp_rtos_timer_arm -> Timer::arm`

## Narrowed Boundary

This closes another ambiguity.

What is now proven:

1. The paired-retarget branch does not bypass the real timer driver.
2. The paired-retarget branch still uses the normal lower timer arm stack.
3. The baseline app repeatedly rearms the original NAN slot callbacks from two concrete `chm_init` callsites.
4. The paired-retarget case does not show that repeated `ets_timer_arm` / `chm_init` rearm pattern in the explicit-compare window.

Current strongest statement:

- the meaningful split is no longer “does the timer arm machinery work?”
- the split is:
  - baseline keeps the original `chm_init`-driven NAN slot rearm pattern alive
  - paired retarget replaces callback identity and loses that repeated baseline `chm_init` wrapper-arm pattern
  - that change is sufficient to push the app into the earlier `scan_rc=12300` branch

## Practical Next Step

The next useful target is the exact `chm_init` timer-arm callsites.

1. Disassemble the two baseline `chm_init` arm callsites:
   - `0x40122e71`
   - `0x40122ea1`
2. Compare them against the comparator's `chm_init` timer setup.
3. Determine whether the live difference is:
   - callback identity only,
   - handle lifetime / recreation timing,
   - or another `chm_init`-owned state coupled to those two slot arms.
