# Wi-Fi History Narrowing Follow-up 18

## Scope

This follow-up records the legacy timer-compat instrumentation added after follow-up 17 and the first clean exercise of the combined hypothesis:

- old timer substrate semantics
- comparator-style `g_chm` slot callback family

The work stayed within the novelty gate by rerunning only the already-active timer hypothesis with new instrumentation.

## New Artifacts

- Legacy compat only:
  - `logs/flash_capture_20260318_legacycompat_recentsetfn_only/capture.log`
- Legacy compat plus initial direct callback rewrite attempt:
  - `logs/flash_capture_20260318_legacycompat_recentsetfn_retarget/capture.log`
- Legacy compat plus direct retarget trace:
  - `logs/flash_capture_20260318_legacycompat_retarget_trace/capture.log`
- Legacy compat plus arg0/arg1 pair retarget:
  - `logs/flash_capture_20260318_legacycompat_pair_retarget/capture.log`

## Code Changes In This Slice

- `vendor/esp-radio-0.17.0/src/compat/timer_compat_legacy_diag.rs`
- `vendor/esp-radio-0.17.0/src/compat/timer_compat_legacy.rs`
- `vendor/esp-radio-0.17.0/src/compat/timer_compat.rs`

## What Is Now Proven

### 1. Under legacy timer compat, the `g_chm` slot pair still installs the app callback family.

The new legacy recent-`setfn` ring shows the first two arg0/arg1 slot installs under legacy compat only:

- `timer_compat_setfn_recent ... ets_timer_ptr=0x3ffcad70 callback_ptr=0x401223dc arg_ptr=0x0`
- `timer_compat_setfn_recent ... ets_timer_ptr=0x3ffcad84 callback_ptr=0x401223dc arg_ptr=0x1`

From the matching legacy-only image map:

- `nan_dp_schedule_ndc_start = 0x40122374`
- therefore `0x401223dc = nan_dp_schedule_ndc_start + 0x68`

So old timer substrate alone does not switch the slot callback family to the comparator path.

### 2. The first direct rewrite attempt missed because the source pointer comparison was wrong.

In the traced legacy-retarget run:

- `incoming=0x4012295c`
- `source=0x400d6d30`
- `target=0x401337a0`
- `effective=0x4012295c`

And from the matching retarget image map:

- `nan_dp_schedule_ndc_start = 0x401228f4`
- therefore `0x4012295c = nan_dp_schedule_ndc_start + 0x68`
- `ieee80211_timer_process = 0x4013382c`
- therefore `0x401338cc = ieee80211_timer_process + 0xa0`

So the legacy retarget logic was comparing against a broken extern-derived source address instead of the actual incoming callback pointer.

### 3. The combined hypothesis has now been exercised cleanly.

A new diagnostics-only pair retarget was added in legacy compat:

- detect the arg0/arg1 pair that shares the same callback pointer
- rewrite both timer entries to `ieee80211_timer_process + 0xa0`

The trace confirms the rewrite:

- `legacy_timer_retarget ... incoming=0x40122a88 ... effective=0x40122a88`
- `legacy_timer_pair_retarget source=0x40122a88 target=0x401338cc`

From the current image map:

- `nan_dp_schedule_ndc_start = 0x40122a20`
- so `0x40122a88 = nan_dp_schedule_ndc_start + 0x68`
- `ieee80211_timer_process = 0x4013382c`
- so `0x401338cc = ieee80211_timer_process + 0xa0`

This means the app was actually forced onto:

- old timer substrate semantics
- comparator-style `g_chm` slot callback family

## Behavioral Result

That combined retarget does not restore the working comparator path.

It deterministically flips the app into the earlier failure branch:

- `scan_rc=12300`
- `scan_done_count=0`
- `blob_chm ... op_chan=0x01 ptr_08=0xa ptr_0c=0x14`
- `blob_scan ... word_00=0x0000010f word_30=0x14 word_34=0x0a`

This matches the earlier paired-slot retarget failure form seen on the new timer substrate.

## Negative Result That Matters

Legacy timer exec diagnostics still do not show a useful execution split:

- `timer_exec_diag ... current_callback_ptr=0x0 last_callback_ptr=0x0`

for both:

- legacy compat only
- legacy compat plus pair retarget

So the branch flip is still not explained by the currently visible timer execution history.

## Updated Narrowing

These branches are now closed:

- old timer substrate alone fixes full firmware
- old timer substrate prevents the callback-family branch split
- callback rewrite failure was caused by the overall hypothesis being wrong

What remains live:

1. the exact `g_chm` slot callback family is causally upstream of the branch split
2. timer substrate semantics do not change that causal outcome once the callback family is forced
3. comparator success still requires additional coupled state beyond:
   - old timer substrate
   - comparator-style `g_chm` slot callbacks

## Best Next Step

Do not spend more time on timer substrate permutations.

The next high-value target is the consumer path that reacts to the `g_chm` slot callback identity before result materialization.

Specifically:

1. compare baseline app versus pair-retarget app at the first scan/channel-manager consumer after `chm_init`
2. focus on the path that turns:
   - baseline into `op_chan=0xff`, `scan_rc=0`, empty result list
   - pair-retarget into `op_chan=0x01`, `scan_rc=12300`
3. treat timer callback identity as an upstream selector, not as the full root cause
