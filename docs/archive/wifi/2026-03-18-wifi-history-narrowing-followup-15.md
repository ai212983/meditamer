# 2026-03-18 Wi-Fi History Narrowing Follow-up 15

## Scope

This follow-up closes the app-side static `chm_init` callback-install question left open by follow-up 14.

It answers one question:

- is the app-side `g_chm` slot callback split only a runtime side effect, or is it already hard-coded in `chm_init` itself?

## Artifacts

- App binary:
  - `target/xtensa-esp32-none-elf/debug/meditamer`
- Comparator binary:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`
- Baseline app capture:
  - `logs/flash_capture_20260318_142006/capture.log`
- Comparator caller capture:
  - `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`

## Static Result

### App `chm_init`

Relevant app disassembly window:

- `target/xtensa-esp32-none-elf/debug/meditamer`
- `40122d8c .. 40122daa`

Key instructions in that block:

- `l32r a2, ... (4012279c <nan_dp_schedule_ndc_start+0x68>)`
- `mov.n a11, a2`
- `addi a10, a10, 36`
- `callx8 a4`
- `movi.n a12, 1`
- `mov.n a11, a2`
- `addi a10, a10, 56`
- `callx8 a3`

Meaning:

- app `chm_init` directly installs the same callback literal into both `g_chm` timer slots
- that callback literal is in the `nan_dp_schedule_ndc_start` symbol range
- slot 0 gets `arg=0`
- slot 1 gets `arg=1`

This is not just a later compat-layer side effect. The callback-family choice is already present in app `chm_init` itself.

### Comparator `chm_init`

Comparator runtime evidence from follow-up 14 shows:

- `ets_timer_setfn` on slot 0 / slot 1 happens from callers inside `chm_init`
- installed callback is inside `ieee80211_timer_process`

Concrete runtime evidence:

- `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`
  - lines `76-77`

Relevant resolution:

- callback `0x401097a8` is inside `ieee80211_timer_process`
- callers `0x80109c31` and `0x80109c42` are inside comparator `chm_init`

## Conclusion

The callback-family split is hard-coded at the `chm_init` producer boundary.

It is not merely:

- a later timer-queue artifact
- a generic compat-timer side effect
- or a post-install mutation of otherwise identical slot setup

The direct split is:

- app `chm_init` installs `nan_dp_schedule_ndc_start`
- comparator `chm_init` installs `ieee80211_timer_process`

That is the strongest code-level boundary established so far.

## What This Does And Does Not Prove

What it proves:

- the app/comparator divergence exists at the exact slot-callback producer
- the divergence is present before later explicit-scan failure handling

What it does not yet prove:

- that callback identity alone is sufficient for the final zero-result failure

We already tested the strongest available approximation of that idea:

- paired slot retarget to the `ieee80211_timer_process` family

and that changed the failure mode to `scan_rc=12300` rather than restoring the comparator path.

So callback identity is causally upstream, but it is coupled with additional state.

## Narrowed Next Step

The next useful target is the setup state adjacent to those two `chm_init` `ets_timer_setfn` calls.

Specifically:

1. compare the app and comparator `chm_init` blocks around the callback install and subsequent arm calls
2. identify the adjacent state coupled to the callback-family choice
   - timeout words at `a2+8` and `a2+12`
   - surrounding `g_chm` fields
   - any preceding branch that selects the callback literal
3. treat the live root-cause target as:
   - callback-family choice in `chm_init`
   - plus the state consumed immediately around that choice
