# Multicore scheduling implementation

- Status: Done — implementation closed; residual qualification tracked separately
- Last-reviewed: 2026-09-08
- Decision: [ADR-0027](../../architecture/0027-multicore-scheduling-policy.md)
- Execution: [baseline and gate evidence](multicore-scheduling-evidence.md)

## Closeout — 2026-09-08

Closed at the user's request after reset restored operation and the user confirmed
that everything was responsive. CPU1 I²C/acquisition ownership, CPU0 application
pipelines, bounded transport/control, serial readiness and scheduling instrumentation
are implemented. This closes implementation work; it does **not** assert that every
original qualification gate below passed.

Navigation defects (Clock returning after Launcher → Ambient Home) and intermittent
panel corruption/visual soaks are explicitly deferred. The freeze preceding the
reset remains unexplained. Numeric scheduling, remaining lifecycle/runtime checks,
Wi-Fi regression and quality-gate gaps are retained in
[REL-009](../../reference/reliability-issues.md#rel-009-multicore-integration-residual-qualification).
No deadlines, memory floors or acceptance thresholds were relaxed for closeout.
The evidence ledger records the final installed image and individual results.
The sequence below is the original contract, retained as history.

## Outcome and scope

CPU1 owns shared I²C and acquisition; CPU0 owns product state, input interpretation,
rendering, storage, and application networking. Implement the ADR's bounded
communication, dependency-aware priorities, event-driven waits, and latency checks.
Completion means verified ownership and service behavior, not equal CPU percentages.

Keep sensor cadence, calibration, bus rates, panel waveforms, and radio affinity
unchanged during placement work. The factory updater and Medinote placement are
outside scope; shared changes must still build for their supported configurations.
No firmware changes or device experiments are performed by creating this plan.

## Code boundaries

| Area | Responsibility |
| --- | --- |
| `targets/meditamer-inkplate/src/system.rs`, `system/tasks.rs` | Core startup, resource transfer, readiness, task placement |
| `products/meditamer/src/firmware/types/{i2c,touch_bus,base}.rs` | Owner-local HAL handles, CPU0 request handles, display dependencies |
| `boards/inkplate-tempera/src/touch_bus_proxy.rs` | Existing bounded transport to adapt after auditing all clients |
| `products/meditamer/src/firmware/{touch,imu,environment,battery}` and RTC consumers | Acquisition, publication, control and recovery boundaries |
| `products/meditamer/src/firmware/panel_bus.rs`, display and UI backend | Panel arbitration and synchronous rendering integration |
| `products/meditamer/src/firmware/{scheduling.rs,serial.rs,serial}` | Priority policy, bounded control and bulk output, event-driven waits |
| `platform/cpu-load`, `tools/hostctl` | Bounded timing observations and reproducible capture |

## Execution sequence

Each step produces a reviewable change and passes its gate before the next step.
Keep placement, wakeup reduction, and priority changes independently measurable.

### 1. Establish the contract and baseline

- Inventory every runtime and startup I²C client: touch, IMU, environmental sensors,
  battery, RTC, panel/PMIC, expanders, frontlight, diagnostics and serial commands.
  Record transaction shapes, largest payloads, rates, blocking APIs, lifecycle and
  control dependencies. Do not assume the touch proxy's 32-byte request limit or
  single reply slot covers all clients.
- Record task/executor/core and interrupt affinity, including platform-owned radio
  work. Identify acquisition-local processing versus application interpretation;
  retain touch/IMU application pipelines on CPU0 unless a specific operation belongs
  to acquisition. Trace output backpressure through suspend/resume acknowledgements.
- Capture baseline CPU time, wakeups, maximum poll and masked intervals, wake-to-run
  latency, bus queue/transfer duration, sample gaps, and both core stack high-water
  marks. Reuse existing profiling and passive capture; add only bounded counters or
  histograms needed to observe missing quantities, with tracing overhead identified.
- Before migration, record numeric budgets for touch delivery, acquisition cadence,
  control acknowledgement, queue waiting and masked windows in the execution evidence.
  Derive them from input/sensor contracts and measured panel windows; do not turn an
  observed stall into an acceptable budget. Keep the 40 ms transfer deadline and
  100/400 kHz device rates. Queue waiting gets a separate finite deadline.

**Gate:** every client and dependency has an owner and bounded wait contract;
baseline evidence, budgets and an exact restorable firmware artifact are recorded.

### 2. Prove owner startup and the request boundary

- Prepare the HAL in blocking mode for transfer to CPU1; enter async mode and bind
  its interrupts there. Create owner-local handles on CPU1. Do not add unsafe `Send`
  or transfer an initialized async driver across cores.
- Define bounded startup readiness/failure reporting so CPU0 can await required
  initialization without a circular dependency. Distinguish bus readiness from
  individual sensor availability; preserve optional-device and retry behavior.
  Preserve BME688 initialization before ELAN and exclusive ELAN reset/hello before
  other clients run. Start the owner in panel-waveform-fixture mode too, even when
  acquisition tasks are omitted.
- Adapt the transport for all required CPU0 transactions. Requests own their data,
  preserve repeated-start/transaction semantics and device configuration, and have
  correlated replies. Cancellation, timeout, late replies, recovery and caller
  reuse cannot corrupt the next request or leave the peripheral wedged.
  Expired or cancelled queued requests must not start. An interrupted in-flight
  write can have uncertain side effects; reconcile device state before retrying or
  advancing a dependent panel/power transition.
- Keep control progress independent of full sample queues. Bound queued work and
  non-preemptible transaction time; urgent control cannot bypass an active transfer.
  Specify fairness between local acquisition and CPU0 requests without creating a
  general-purpose scheduling framework.
- First build a focused startup/transport probe proving CPU1 interrupt affinity,
  transaction completion and error recovery. Account for task pools, channel storage
  and CPU1 stack before integrating the production acquisition tasks.

**Gate:** host tests cover transaction/configuration forwarding, concurrent callers,
cancellation, stale replies and late write effects; hardware proves startup,
deadlines and recovery.
The [DRAM budget](../../reference/dram/dram-budget.md) and ELF stack/IRAM gates pass.

### 3. Integrate acquisition and panel coordination

- Move touch, IMU, environment and battery acquisition with their drivers onto CPU1.
  Keep timestamps, demand subscriptions, fault handling and sample continuity intact.
  Keep RTC physical access on the owner core through requests; wall-clock policy,
  command handling, gesture recognition and UI state stay on CPU0.
- Route every remaining CPU0 I²C client through the request boundary, including
  initialization, panel control, frontlight, diagnostics and recovery paths. Remove
  the reversed touch-to-CPU0 proxy once touch is local to the owner.
- Preserve acknowledged panel quiescence, the permitted touch waveform window,
  pipeline reset semantics, and resume ordering. The owner must continue processing
  control while producers wait for CPU0 consumers. Exercise full queues while CPU0
  renders or masks interrupts; no scan holds a cross-core lock or waits on CPU1.
- Audit cache-disabled scan overlap with CPU1 code/data access and shared expander
  state. Preserve register update atomicity across clients, not just individual I²C
  transfer serialization. Remove CPU0 bus-idle rendering gates only after their
  replacement ownership/arbitration contract is demonstrated.

**Gate:** concurrent acquisition and full/partial/clean refresh meet the budgets;
panel, touch/WAKE, IMU and sensor behavior survive faults and suspend/resume.
Validate waveform timing and visual output on hardware before accepting this step.

### 4. Reduce wakeups and align priorities

- Replace serial's periodic receive timeout with readiness, outbound-work signals and
  the next actual session deadline. Bound parsing, command execution and output
  drains; separate responsive control handling from background bulk formatting and
  transmission without competing UART owners or interleaved protocol records.
- Audit acquisition and service loops for avoidable polling and redundant wakes.
  Batch only where transaction semantics, demand and latency budgets permit; retain
  necessary sensor timers and sampling rates. Measure each change separately.
- Set priorities per executor from the dependency graph: short acquisition/control
  first, ordinary service next, bulk diagnostics last. Verify progress of consumers
  needed by higher-priority producers and continuously ready network work in every
  service profile. Preserve platform-required radio scheduling.
- Retain thread-mode executors by default. Add a separate preempting context only
  if a measured budget violation survives bounded polls and correct placement;
  record its stack/interrupt cost and prove masked-window behavior before adoption.

**Gate:** latency budgets hold under sustained traffic and serial output; control
and background work make progress, and wakeup/CPU improvements exceed measurement
noise. Reject optimizations that trade deadline failures for lower utilization.

### 5. Qualify and close

| Validation | Required evidence |
| --- | --- |
| Host | Transport lifecycle/fault tests; acquisition control and touch replay tests, including full queues and reset |
| Build and memory | Inkplate default, minimal, BLE and panel-waveform-fixture configurations; affected shared-target/updater builds; ELF section/task-pool deltas, both core stack margins and runtime high-water, internal-heap admission/high-water with existing profile-specific floors |
| Static gates | `check_stack_risk.sh`, `check_iram_flash_refs.sh`, `check_panel_waveform_placement.sh`, `check_panel_bus_gating.sh`, plus applicable repository quality checks |
| Device lifecycle | Normal and panel-fixture startup, missing/failing sensor, transaction timeout, repeated panel suspend/resume and available sleep/restart paths; no unexpected reset or stale reply |
| Concurrent behavior | Tap, hold, swipe, WAKE, clock/launcher navigation, partial/full/clean refresh, SD activity and network/upload; bounded deadlines and no sample/gesture continuity regression |
| Scheduling | All service profiles, sustained serial/log output and full queues; owner/control/consumer progress, measured poll/masking/queue latency within frozen budgets |

Use [build/flash guidance](../../guides/build-and-flash.md) and canonical hostctl
workflows; keep orchestration in scenario YAML. Preserve unfiltered serial output
so low-level timeouts and resets cannot disappear from filtered metrics. Record
firmware identity, configuration, workload and pass/fail evidence per run.

Run the [Wi-Fi regression gate](../../guides/wifi-regression-gate.md) for this scheduling
integration before landing. Before any Wi-Fi/upload A/B or tuning rerun, apply the
repository novelty gate against the archived decision ledgers; do not repeat rejected
knobs as new experiments. Placement qualification does not authorize radio tuning.

Measure power and settled raw/reference temperature separately only if claiming a
thermal or energy benefit. Keep host, device, visual and electrical proof distinct.
Update ownership/runtime references and the DRAM budget, retire obsolete proxy and
render-gate code, and archive this plan with its compact evidence ledger when done.

## Failure handling

Stop at a failed gate and fix the cause within that step. Do not increase deadlines,
queue depth, priority or stack reservations simply to hide a failure. Recalculate
memory and rerun affected checks when a justified design change alters a bound.
Restore the recorded passing image if a candidate destabilizes the device; preserve
its logs and isolate the failing change before continuing. Unavailable physical
validation remains explicitly pending and prevents claiming full qualification.
