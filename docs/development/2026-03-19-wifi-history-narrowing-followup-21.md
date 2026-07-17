# Wi-Fi History Narrowing Followup 21

## Scope

This follow-up records the next timer-recovery narrowing step after followup 20.

Goal:
- determine what the baseline legacy callback family actually does during scan recovery
- compare that behavior to the pair-retarget branch
- test whether the earlier `arg=0`-only hypothesis is actually causal

## New Instrumentation

Added callback-side-effect tracing inside legacy timer compat.

While a legacy callback is executing, the trace now records callback-driven:
- `compat_timer_disarm`
- `compat_timer_arm_us`
- `compat_timer_setfn`

Each side-effect line includes:
- current callback pointer and arg
- target timer pointer
- target callback pointer and arg
- arm timeout/repeat when applicable

Also added a diagnostics-only skip gate:
- `MEDITAMER_WIFI_LEGACY_TIMER_SKIP_RECOVERY_ARG0_EXEC_DIAG=1`

That gate skips only callbacks that match all of:
- `arg=0`
- `pre_scan_word00=0x0000010f`
- `pre_scan_word114=0x00000000`

## Runs

1. Legacy timer compat baseline with callback-side-effect trace
- log: [flash_capture_20260319_legacycompat_sideeffects_baseline/capture.log](../../logs/flash_capture_20260319_legacycompat_sideeffects_baseline/capture.log)

2. Legacy timer compat plus pair retarget with callback-side-effect trace
- log: [flash_capture_20260319_legacycompat_sideeffects_pair_retarget/capture.log](../../logs/flash_capture_20260319_legacycompat_sideeffects_pair_retarget/capture.log)

3. Legacy timer compat with recovery `arg=0` callbacks skipped, extended boot window
- log: [flash_capture_20260319_legacycompat_skip_recovery_arg0_long/capture.log](../../logs/flash_capture_20260319_legacycompat_skip_recovery_arg0_long/capture.log)

## Proven Facts

### 1. Baseline recovery callback execution has real side effects

In the baseline legacy-compat run, the recovery callback family repeatedly emits callback-side-effect lines immediately after each due execution.

Representative sequence from the baseline log:
- `legacy_due_trace ... callback_ptr=0x40123b44 arg_ptr=0x0 pre_op_chan=0x01`
- then repeated callback-driven timer operations:
  - `kind=disarm ... target_timer_ptr=0x3ffcb4c4 target_callback_ptr=0x40123b44 target_arg_ptr=0x1`
  - `kind=disarm ... target_timer_ptr=0x3ffcb8bc target_callback_ptr=0x40125634 target_arg_ptr=0x0`
  - `kind=disarm ... target_timer_ptr=0x3ffcd5ac target_callback_ptr=0x40145604 target_arg_ptr=0x0`
  - `kind=disarm ... target_timer_ptr=0x3ffcb4b0 target_callback_ptr=0x40123b44 target_arg_ptr=0x0`
  - `kind=arm ... target_timer_ptr=0x3ffcb4b0 target_callback_ptr=0x40123b44 target_arg_ptr=0x0 us=10000`
  - `kind=arm ... target_timer_ptr=0x3ffcb4c4 target_callback_ptr=0x40123b44 target_arg_ptr=0x1 us=20000`

So the escaping baseline path is not just passively waiting.
It actively reconfigures timer state on each recovery-step execution.

### 2. Pair-retarget executes due callbacks but emits no callback-side effects

In the pair-retarget run:
- `legacy_due_trace ordinal=1 found=1 executed=1 callback_ptr=0x40134fc4 arg_ptr=0x0`
- `legacy_due_trace ordinal=2 found=1 executed=1 callback_ptr=0x40134fc4 arg_ptr=0x1`

But there are no `legacy_callback_sideeffect` lines in that failing window.

This is the strongest live discriminator in this follow-up.

Interpretation:
- the pair-retarget branch does not fail because no due callback executes
- it fails because the retargeted callback family does not perform the same timer reconfiguration work that the baseline recovery family performs

### 3. The earlier `arg=0`-only hypothesis is false

The long-window skip run changes the earlier interpretation.

In that run:
- every recovery-phase `arg=0` callback is skipped with:
  - `reason=recovery_arg0`
- the `arg=1` callback still executes on every channel step
- the `arg=1` callback still emits the same timer side effects:
  - disarm of the ancillary timers
  - disarm/rearm of both `g_chm` slot timers

And the run still reaches the recovered scan state:
- `event scan_done_list status=0 count=0 scan_id=128`
- `blob_chm after=rust_scan op_chan=0xff`
- `blob_scan after=rust_scan word_00=0x00000000`
- `blob_scan after=rust_scan word_114=0x00000080`
- `scan_done_eventpost after=rust_scan count=1`
- `scan_get_scan_id ret=0x00000080`

This closes the `arg=0`-is-necessary theory.

### 4. The surviving causal boundary is callback family behavior, not callback slot number

What now survives:
- baseline recovery family can use either `arg=0` or `arg=1` and still drive the same slot-timer maintenance loop
- pair-retarget executes both `arg=0` and `arg=1` callbacks, but does not emit the maintenance loop at all

So the live split is now:
- baseline callback family behavior versus retargeted callback family behavior
- not `arg=0` versus `arg=1`

## Current Narrowed Boundary

The live root-cause range is now much tighter:
- both branches enter the same bad scan state after `scan_start`
- baseline recovery happens because its callback family actively disarms and rearms the `g_chm` slot timers and related timers while scan progression is in flight
- pair-retarget executes due callbacks but does not perform that maintenance loop
- therefore pair-retarget never reaches the later recovery state that baseline reaches

## What This Closes

Closed:
- no-due-callback-execution as the reason pair-retarget fails
- `arg=0` callback execution as a necessary component of baseline recovery
- the earlier `arg=0`-only narrowing from followup 20

Still live:
- the exact semantic work performed by the baseline callback family during that maintenance loop
- why the retargeted callback family does not reproduce it even though the timer slots and due-dispatch path are active

## Exact Next Step

Move one level deeper into callback semantics.

Highest-value target:
- compare the baseline recovery callback family against the retarget target at the first state mutation they make to:
  - `g_chm` slot timers
  - the ancillary timers they disarm
  - or the channel/scan state that follows those rearm operations

The best current model is:
- the decisive split is no longer generic timer infrastructure
- it is callback-family-specific timer maintenance behavior during scan recovery
