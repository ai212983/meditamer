# Upload Throughput History Part 19

## 2026-03-09: legacy-style poll-and-yield waits in current `esp-rtos` stall the current standalone `esp-radio` probe before `wifi_new`

- Added a narrow runtime A/B in the vendored current scheduler/runtime:
  - `vendor/esp-rtos-0.2.0/src/semaphore.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/queue.rs`
- New diagnostic gate:
  - `MEDITAMER_WIFI_ESP_RTOS_LEGACY_POLL_WAIT_DIAG=1`
- When enabled:
  - blocking `Semaphore::take()` switches from `WaitQueue`-based blocking to legacy-style poll-and-yield
  - blocking `Queue::send_to_back`, `send_to_front`, and `receive` switch from `WaitQueue`-based blocking to legacy-style poll-and-yield
- This was tested in the isolated current-stack comparator:
  - `tools/esp_radio_nostd_wifi_control/Cargo.toml`
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
  - still using vendored `esp-radio` + vendored `esp-rtos`
- Rebuilt the tool with:
  - `MEDITAMER_WIFI_ESP_RTOS_LEGACY_POLL_WAIT_DIAG=1`
- Flashed app-only to `0x10000` and captured:
  - `logs/esp_radio_nostd_wifi_control_legacypoll_20260309_100205/monitor.log`

Key evidence from the log:
- Boot is clean and the explicit-yield markers still run:
  - `diag_yield label=after_rtos_start count=8`
  - `begin=true`
  - `esp_radio_init=ok`
  - `diag_yield label=after_esp_radio_init count=8`
- The probe does not progress to:
  - `wifi_new=ok`
  - `set_mode=sta`
  - `start=ok`
  - `scan=...`
- A follow-up bounded read from the same image produced no additional serial output:
  - silent follow-up after the initial `esp_radio_init=ok` checkpoint

Conclusion:
- legacy-style poll-and-yield waits are not a fix for the current stack
- they make the current `esp-radio` / `esp-rtos` runtime fail earlier, before `wifi_new`
- this is still useful causally:
  - it confirms the current stack depends on deeper `esp-rtos` wait-queue semantics during or immediately after `esp_radio::init`
  - it also shows the working legacy built-in scheduler is not equivalent to “just poll and yield everywhere”
- the next target should move below this broad wait-policy A/B and focus on the specific `esp-rtos` task/timer/queue state transitions immediately after `esp_radio_init`, before `wifi_new`

## 2026-03-09: current standalone `esp-radio` creates exactly one task and one queue during `wifi_new`, not during `esp_radio_init`

- Added narrow creation counters in the vendored current runtime:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/queue.rs`
  - `vendor/esp-rtos-0.2.0/src/lib.rs`
- Exported hidden diagnostics for:
  - total `task_create` count
  - total `queue_create` count
  - last created queue capacity
  - last created queue item size
- Updated the standalone current comparator to print those counters at:
  - after `esp_rtos::start`
  - after `esp_radio::init`
  - after `wifi_new`
  - after `wifi_start`
- Rebuilt the standalone current comparator, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_creatediag_20260309_101042/monitor.log`

Key evidence from the log:
- After scheduler start:
  - `task_create_count=0`
  - `queue_create_count=0`
- After `esp_radio_init=ok`:
  - still `task_create_count=0`
  - still `queue_create_count=0`
- After `wifi_new=ok`:
  - `task_create_count=1`
  - `queue_create_count=1`
  - `queue_last_capacity=200`
  - `queue_last_item_size=8`
- After `start=ok`:
  - counts remain unchanged
  - still one task and one queue total
- The scan still fails:
  - `scan=ok count=0`

Conclusion:
- the current standalone failing path does not stall in object creation itself
- `esp_radio_init` creates nothing in the scheduler/runtime layer
- `wifi_new` is the point where the current stack creates its first runtime objects:
  - one task
  - one queue of `capacity=200`, `item_size=8`
- `wifi_start` does not create additional runtime objects in this probe
- the next target should therefore move from create-time to run-time behavior:
  - what that first queue carries
  - how that first task processes it
  - whether the queue/task pair progresses normally before the zero-result scan completes

## 2026-03-09: the single `wifi_new` queue is live and heavily active, but the current standalone `esp-radio` scan still returns zero

- Updated the standalone current comparator:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Added stage-isolated use of the existing vendored Wi-Fi OS diagnostics:
  - reset Wi-Fi OS counters after `esp_radio_init`
  - print queue/semaphore/event counters and sampled queue identities after `wifi_new`
  - reset again and print after `wifi_start`
  - reset again and print after `scan`
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_queuetraffic_20260309_101640/monitor.log`

Key evidence from the log:
- The same single created queue remains the active queue throughout:
  - sampled queue pointer stays `0x3ffb33fc`
- The queue is already active by `wifi_new`:
  - early `after_wifi_new` lines are partially corrupted on serial capture, but the surviving receive samples already show:
    - `queue=0x3ffb33fc`
    - receiver task `0x3ffb4ea0`
- After `wifi_start`, the queue is clearly live and ordered:
  - `queue_send=3`
  - `queue_recv=3`
  - `send_task_changes=0`
  - sampled main-side control messages are all `item_word0=0x00000006`
  - sampled receive side stays on the same queue and receiver task
- During the failing scan, queue activity increases materially:
  - `queue_send=41`
  - `queue_send_isr=14`
  - `queue_recv=55`
  - `send_task_changes=27`
  - the same queue `0x3ffb33fc` remains dominant
  - sampled send traffic includes the previously known message families:
    - `item_word0=0x00000006`
    - `item_word0=0x00000010`
    - timer-side `item_word0=0x00000007 pointee_word0=0x00000008 pointee_word1=0x00000000`
- Despite that active queue traffic, the scan still ends at:
  - `scan=ok count=0`

Conclusion:
- the first `wifi_new` queue is not inert
- the zero-result current standalone scan is not explained by:
  - queue never created
  - queue never scheduled
  - queue never receiving traffic
- the remaining target is now narrower:
  - the semantics of what the current queue/task pair is doing during scan
  - especially why that active traffic still leads to `scan=ok count=0` on the current stack while the legacy standalone no-std `esp-wifi` control scans successfully on the same board/network

## 2026-03-09: the standalone current comparator reproduces the same `main -> wifi` and later `timer -> wifi` queue-role pattern as the main firmware diagnostics

- Extended the standalone current comparator task/queue output:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Added direct role resolution using vendored `esp-rtos` task-role tags:
  - queue first/last sender roles
  - queue first/last receiver roles
  - per-sample sender/receiver roles
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_taskroles_20260309_102156/monitor.log`

Key evidence from the log:
- The dominant queue remains a single queue:
  - `queue=0x3ffb3404`
- After `wifi_start`, the role split is already the expected control path:
  - first send role = `main`
  - last send role = `main`
  - first recv role = `wifi`
  - last recv role = `wifi`
  - sampled send items are `main -> wifi` control messages with `item_word0=0x00000006`
- During the failing scan:
  - first send role still = `main`
  - last send role still = `main`
  - first/last recv roles stay = `wifi`
  - sampled send traffic now includes:
    - `main -> wifi` with `item_word0=0x00000006`
    - `wifi -> wifi` with `item_word0=0x00000000`
    - `wifi -> wifi` with `item_word0=0x00000010`
    - `timer -> wifi` with `item_word0=0x00000007 pointee_word0=0x00000008 pointee_word1=0x00000000`
- The scan still ends at:
  - `scan=ok count=0`

Conclusion:
- the isolated current standalone comparator reproduces the same queue-role/message-family pattern already seen in the main firmware branch
- this makes it much less likely that the earlier role pattern was an artifact of the larger firmware or host harness
- the remaining target is now tighter:
  - semantic handling of the active `main/wifi/timer` queue traffic on the current stack
  - not queue creation
  - not role assignment
  - not lack of queue progression

## 2026-03-09: the standalone current comparator shows the `wifi` task actively consuming semantic queue items during the failing scan

- Extended the standalone current comparator again:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Extended vendored Wi-Fi OS diagnostics to snapshot dequeued item words:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
- The receive hook now records:
  - last received item size and words
  - first six receive-sample item/pointee words
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_recvitems_20260309_102546/monitor.log`

Key evidence from the log:
- `after_wifi_new` already shows the `wifi` task consuming control items:
  - all receive samples are `task_role=wifi`
  - all sampled dequeued items are `item_word0=0x00000006`
  - pointee handler tags vary across normal control families
- `after_wifi_start` shows direct consumer-side correspondence with sender-side control traffic:
  - send samples:
    - `0x00000006 / 0x0000030a`
    - `0x00000006 / 0x00000301`
    - `0x00000006 / 0x00000331`
  - receive samples match the same three control items in the same queue:
    - `0x00000006 / 0x0000030a`
    - `0x00000006 / 0x00000301`
    - `0x00000006 / 0x00000331`
- During the failing scan, the `wifi` task consumes a richer family than the sender-side samples alone showed:
  - send-side samples include:
    - `main -> wifi` `0x00000006`
    - `wifi -> wifi` `0x00000000`
    - `wifi -> wifi` `0x00000010`
    - `timer -> wifi` `0x00000007 / 0x00000008 / 0x00000000`
  - receive-side samples include:
    - `0x00000006 / 0x00000305`
    - `0x00000000`
    - `0x00000017`
    - `0x00000017`
    - `0x00000010`
    - `0x00000007 / 0x00000008 / 0x00000000`
- Despite active consumer-side dequeue and dispatch-like progression, the scan still ends at:
  - `scan=ok count=0`

Conclusion:
- the current standalone failing path is not blocked before queue consumption
- the `wifi` task is actively consuming and differentiating semantic queue items during the zero-result scan
- the newly visible `0x00000017` receive-side family is now a high-value target, because it appears on the consumer side during the failing scan even though the sender-side samples only exposed `0x6`, `0x10`, and `0x7`
