# Wi-Fi History Narrowing Followup 24

## Scope

This follow-up records two current-timer findings after followup 23:
- whether `timer_queue::process()` is actually parking the timer task itself
- whether the retargeted current-substrate timer callbacks perform any timer-maintenance side effects

Goal:
- retire the broad scheduler-wake mismatch hypothesis if the timer task parks itself correctly
- determine whether callback execution alone is enough on the modern timer substrate

## Runs

1. Current-timer sleep-pointer probe with boot scan diag
- log: [flash_capture_20260319_timerqueue_sleep_probe_long_bootdiag/capture.log](../../logs/flash_capture_20260319_timerqueue_sleep_probe_long_bootdiag/capture.log)

2. Current-timer callback-side-effect probe with boot scan diag
- log: [flash_capture_20260319_currenttimer_sideeffects_bootdiag/capture.log](../../logs/flash_capture_20260319_currenttimer_sideeffects_bootdiag/capture.log)

## Proven Facts

### 1. `timer_queue::process()` parks the timer task itself, not some other task

At `idf_explicit_compare_postcall` in the sleep-pointer probe:
- `task_ptr=0x3ffc2980`
- `sleep_count=3`
- `sleep_true_count=3`
- `sleep_false_count=0`
- `sleep_last_task_ptr=0x3ffc2980`
- `sleep_task_mismatch_count=0`

That proves the timer queue is calling `SCHEDULER.sleep_until(...)` with the timer task pointer itself.

So the earlier broad theory that the timer queue might be parking the wrong task is closed.

### 2. The modern timer task does resume, but not through `scheduler.resume_task()`

The same postcall checkpoint shows:
- `entry_count=1`
- `loop_count=3`
- `selected_count=3`
- `process_skip_not_due_count=3`
- `timer_exec_recent` contains two callback executions

But it also shows:
- `mark_ready_count=0`

That is now explained by code shape, not by missing wakeups.

`timer_tick_handler()` wakes timer-queue sleepers through:
- `timer::handle_alarm(...)`
- then direct `run_queue.mark_task_ready(...)`

That path bypasses the earlier `note_timer_task_mark_ready()` counter, which only observes the `scheduler.resume_task()` path.

So the previous scheduler-resume mismatch hypothesis is retired.

### 3. The retargeted current-substrate callbacks do execute

At `idf_explicit_compare_postcall` in both probes:
- `timer_exec_recent idx=1 ordinal=1 callback_ptr=0x40135d08 arg_ptr=0x0`
- `timer_exec_recent idx=2 ordinal=2 callback_ptr=0x40135d08 arg_ptr=0x1`

So the modern timer runtime is not blocked before callback execution.

### 4. Callback execution alone is still not enough on the modern substrate

Even with those two callback executions, the branch remains:
- `idf_explicit_compare=scan_start_err scan_rc=12300`

The postcall state still shows the earlier failing form rather than recovery.

### 5. The executed current-substrate callbacks perform no observed timer-maintenance side effects

At `idf_explicit_compare_postcall` in the side-effect probe:
- `sideeffect_arm_count=0`
- `sideeffect_disarm_count=0`
- `sideeffect_last_kind=0`

This is the strongest new discriminator in the current-timer branch.

The callbacks do fire, but they do not re-arm or disarm any timers through the modern timer implementation while they are executing.

### 6. That differs materially from the successful legacy-compat recovery branch

Earlier legacy-compat recovery traces already showed repeated maintenance-loop side effects from the callback family:
- disarming the two `g_chm` slot timers
- rearming the two `g_chm` slot timers
- disarming ancillary timers

Those side effects were part of the successful recovery path that reached:
- `scan_done count=1`
- `scan_get_scan_id -> 0x80`

The current-substrate branch now proves a different outcome:
- the same high-level callback family can execute
- but no corresponding timer-maintenance loop is visible
- and recovery still does not happen

## Current Narrowed Boundary

The surviving boundary is no longer:
- scheduler wake delivery in general
- wrong task parked by `timer_queue::process()`
- or lack of callback execution

It is now specifically:
- the retargeted current-substrate callback family executes
- but it does not produce the legacy-style timer-maintenance loop
- so the branch remains in the `scan_rc=12300` failure form

## What This Closes

Closed:
- broad scheduler-wake mismatch as the main explanation
- “timer task never sleeps itself correctly”
- “timer callbacks never execute on the current substrate”

Still live:
- why the executed current-substrate callback path does not perform the timer-maintenance operations seen in the successful legacy branch
- whether the blob callback body is taking a different branch on the modern substrate
- whether the callback-family contract still depends on legacy timer object semantics that the modern substrate does not reproduce

## Exact Next Step

Stop probing outer scheduler/timer entry points.

The next useful step is deeper callback-family analysis:
- compare the successful legacy-compat recovery callback path against the current-substrate executed callback path
- focus on why the modern path emits zero timer side effects even though the callback itself fires
- target the callback-body contract or the timer-object semantics it consumes, not more task-wake counters
