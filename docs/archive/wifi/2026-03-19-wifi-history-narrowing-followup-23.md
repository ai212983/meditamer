# Wi-Fi History Narrowing Followup 23

## Scope

This follow-up records the current-timer runtime-task and current-timer `process()` findings after followup 22.

Goal:
- determine whether the current timer substrate failure is caused by the timer task never being created or started
- if the timer task does run, determine why no timer callbacks execute in the failing branch

## New Instrumentation

Added current-timer runtime diagnostics in `esp-rtos` and surfaced them through the boot-scan logs.

New counters added to `timer_runtime_diag`:
- timer-task create count
- create source: `ensure` / `wake` / `enqueue`
- create mode and created task ptr
- current `process()` skip reasons:
  - inactive/drop
  - not-due
- last skipped callback ptr / arg ptr / `now_us` / `due_us`

Also added a lightweight delayed runtime-only snapshot after the explicit compare path, but that delayed line did not appear in the capture.

## Runs

1. Current timer task-create probe
- log: [flash_capture_20260319_currenttimer_taskcreate_probe/capture.log](../../logs/flash_capture_20260319_currenttimer_taskcreate_probe/capture.log)

2. Current timer task-create long capture
- log: [flash_capture_20260319_currenttimer_taskcreate_probe_long/capture.log](../../logs/flash_capture_20260319_currenttimer_taskcreate_probe_long/capture.log)

3. Current timer `process()` skip-reason long capture
- log: [flash_capture_20260319_currenttimer_processskip_probe_long/capture.log](../../logs/flash_capture_20260319_currenttimer_processskip_probe_long/capture.log)

4. Current timer delayed-runtime probe
- log: [flash_capture_20260319_currenttimer_runtime_delayed_probe/capture.log](../../logs/flash_capture_20260319_currenttimer_runtime_delayed_probe/capture.log)

## Proven Facts

### 1. The current timer task is not permanently absent

The short task-create probe initially showed zero creation/start counters through the pre-compare stages:
- `create_count=0`
- `entry_count=0`
- `loop_count=0`

That was not the final state of the failing branch. The longer capture proved the task does get created and run after explicit postcall.

At `idf_explicit_compare_postcall` in the long capture:
- `create_count=1`
- `create_from_enqueue_count=1`
- `create_last_mode=1`
- `entry_count=1`
- `loop_count=3`
- `default_branch_count=3`
- `pop_count=3`
- `selected_count=3`
- `task_ptr=0x3ffc2980`

So the surviving problem is not “timer task never exists.”

### 2. The current timer task is running, but it is not executing callbacks

At `idf_explicit_compare_postcall` in the same run:
- `timer_compat_diag ... exec_count=0`
- `scan_process_wrap_scan_get_id_state ... exec_count=0`
- repeated `scan_get_scan_id ret=0x00000000`

That means:
- the task runs
- the task loops
- the task selects work
- but it still executes zero timer callbacks in the failing branch

### 3. The current timer task is skipping selected timers as not-yet-due

The new `process()` skip counters resolved the reason.

At `idf_explicit_compare_postcall` in the `processskip` capture:
- `process_skip_inactive_count=0`
- `process_skip_not_due_count=3`
- `process_last_skip_callback_ptr=0x40134d08`
- `process_last_skip_arg_ptr=0x1`
- `process_last_skip_now_us=6807595`
- `process_last_skip_due_us=6817616`

So the timer task is not dropping inactive timers.
It is selecting live timers and rejecting them because they are still slightly before their due time.

### 4. The two retargeted slot timers are armed on the current substrate

At `idf_explicit_compare_postcall`:
- `timer_live_arm_diag ... count=2`
- `timer_compat_arm_recent ...`
  - slot 0: `callback_ptr=0x40134b10 arg_ptr=0x0 us=10000`
  - slot 1: `callback_ptr=0x40134b10 arg_ptr=0x1 us=20000`

So the failing branch is not blocked before timer arming.
The slot timers do get armed.

### 5. The current-timer failure boundary is now after arm, inside the wake/sleep contract

Putting the results together:
- timers are setfn-installed
- timers are armed
- timer task is created
- timer task enters and loops
- timer task selects those timers
- timer task skips them as not-yet-due
- timer callbacks still never execute in the failing window

That moves the live boundary to the modern scheduler/timer wake path after `sleep_until`, not to Wi-Fi scan configuration or slot callback-family identity.

### 6. The delayed runtime-only snapshot did not appear

The added `idf_explicit_compare_prefirst_runtime_delayed` line did not appear in the delayed-runtime capture.

That is consistent with the branch remaining stuck between:
- the postcall snapshot
- and later recovery/log points

It does not change the main conclusion above.

## Current Narrowed Boundary

The surviving boundary is now:
- not timer-task creation
- not timer-task entry
- not slot timer arming
- specifically the modern scheduler/timer wake contract after the task calls `SCHEDULER.sleep_until(...)`

Within the current timer queue path, the last proven state is:
- task loops in `process()`
- finds the armed timers
- judges them not yet due
- sleeps
- never reaches a later callback execution in the failing branch

## What This Closes

Closed:
- “current timer task never gets created”
- “current timer task never enters its loop”
- “slot timers never get armed on the current substrate”
- “callbacks are skipped because timers are inactive”

Still live:
- why the modern timer task does not wake back up and execute those timers once they become due
- whether the bug is in:
  - `scheduler::sleep_until`
  - scheduler timer-queue wakeup scheduling
  - timer interrupt wake delivery
  - or a coupled scheduler-state issue after the timer task goes to sleep

## Exact Next Step

Move out of the Wi-Fi module and instrument the scheduler timer wake path:
- `scheduler.rs::sleep_until`
- `timer/mod.rs::schedule_wakeup`
- `timer_tick_handler`
- the timer-queue wake path that marks the timer task ready again

The current evidence says the root cause is no longer primarily inside Wi-Fi scan logic.
