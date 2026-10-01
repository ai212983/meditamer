# Multicore scheduling execution evidence

- Status: Done — implementation closed with residual qualification recorded
- Last-reviewed: 2026-09-08
- Plan: [multicore scheduling](multicore-scheduling.md)

## Baseline identity and capture

Source is the existing dirty working tree at `2b93cdf12bfbb7058c249019fa993da0b4496e93`.
Commit identity alone cannot reproduce this image. The prior installed release is
preserved under `logs/async-i2c-20260908/render-gate-flash/`, including ELF,
application, bootloader, partition table, build metadata and flash logs.
ELF SHA-256: `0e3ed8f611f77dc2d773df95343a547de35510cf0b4d545e604e96df20f003de`.
Application SHA-256: `7491f839eabad19435a3040adde3fac138f4beaadd88ab2ec0ef30876acbc957`.
Restore through the canonical flash-capture image path, preserving production boot
selection; do not rebuild an old commit and assume it recreates this dirty image.

The unmodified-device capture is
`logs/multicore-scheduling-20260908/baseline-capture/`: hostctl's existing
`cpu-profile.sw.yaml` ran normal (60 s), display paused (240 s), restored (60 s).
`result.json` confirms completion and restoration; `serial.log` is unfiltered.
There were zero logged I²C timeouts, startup-I²C errors, environmental acquisition
failures or resets. Three complete paused intervals averaged CPU0 **13.97%** and
CPU1 **0.22%**. These are workload observations, not power or thermal evidence.
Reported minimum stack headroom: CPU0 **81808**, CPU1 **3412** bytes; these are
existing sampled-SP hooks, not whole-stack watermark scans.

## Bus clients and transaction boundaries

All physical transfers currently run on CPU0 at 100 kHz unless marked 400 kHz.
`ConfiguredI2cDevice` configures the device under the bus mutex and applies a
40 ms transfer timer. Waiting for that mutex currently has no deadline.
Reads below are distinct from preceding writes unless written as W/R, which is
one repeated-start transaction. Sizes include the register byte on writes.

| Client | Transactions / payload | Lifetime, intended owner and dependencies |
| --- | --- | --- |
| ELAN touch | 4-byte commands; 4-byte hello/resolution and 8-byte reports | Startup reset/hello exclusive; runtime driver on CPU1, currently proxied to CPU0; remains CPU1-local after migration |
| External PCAL at 0x21 | W1/R23 startup cache; W2 output/config updates | Touch power/reset only; cache and both writes remain with touch owner |
| LSM6DS3 IMU | W2 configuration; W1/R1 status; W1/R12 axes | CPU0 acquisition today, CPU1 after migration; three reads per sample, including PCAL input before read-to-clear TAP_SRC |
| BME688 at 0x76 | W1/R23, R14, R5 calibration; W1/R17 forced-mode measurement; bounded register/value writes | Initialize before ELAN; CPU1 acquisition/retry after migration; preserve forced conversion delay and optional availability |
| SHT45 at 0x44 | W1 command; timer; R6 with CRC | Optional reference in environment acquisition; CPU1 after migration; no bus lock during conversion |
| BQ27441 at 0x55 | W1/R2 SoC | Battery provider moves to CPU1; needs shared PCAL fuel-gauge wake before reading |
| PCF85063A at 0x51 | W1/R11 snapshot; W8 calendar; W2 controls/offset; readback | Serial owns policy and clock requests on CPU0; physical access moves behind CPU1 requests; preserve invalidate/STOP/write/restart/verify sequence and failed-write cleanup |
| Internal PCAL at 0x20 (400 kHz) | W1/R1 cached registers, W2 mutation | Panel, frontlight, SD power, buzzer and fuel-gauge wake share one cache; lock spans each logical update, not just one transfer |
| TPS65186 at 0x48 (400 kHz panel, 100 kHz IMU diagnostic) | W5 startup; W2 control; W1/R1 power-good and temperature | Panel power/cleanup stays CPU0 policy with owner requests; IMU auxiliary power-good read stays acquisition-local |
| Frontlight digipot at 0x2e (400 kHz) | W2 brightness | CPU0 display request; retain checked power preparation and retry policy |
| Buzzer digipot at 0x2f (400 kHz) | W1 wiper payload (pitch control disabled by default) | CPU0 panel driver/service request; retain power ordering |
| Recovery at 0x7f (400 kHz) | W1 | Existing board recovery path; include in request coverage, not an ordinary sensor |

Source owners: `boards/inkplate-tempera/src/{base,control,i2c,panel,expander,imu,
environment,sht45,fuel_gauge}.rs`, `touch/{elan,power}.rs`,
`platform/rtc/src/driver.rs`, and pinned `bme68x` 0.2.0 async implementation.
The forced-mode product path does not use the BME68x parallel-mode 51-byte read;
transport support must nevertheless be explicit about rejected shapes/capacities.
Do not infer general transaction support from the current touch bridge's 32 bytes.

## Startup and scheduling dependencies

`system.rs` constructs blocking I2C0 then enters async on CPU0, binding I²C there.
UART0/SPI2 and primary RTOS startup also originate there. CPU1 has a 4096-byte
touch stack with 60-byte guard offset and its own thread-mode executor.
`system/tasks.rs` currently performs panel initialization, BME688 initialization,
starts the CPU0 touch bridge, completes exclusive ELAN bootstrap, then starts CPU1.
Normal CPU0 tasks: touch and IMU pipelines, IMU/environment/battery acquisition,
display, serial/RTC, SD, diagnostics, firmware health, CPU observations and
application network services. The panel fixture omits the CPU1 startup today.
Radio-owned RTOS work is not an Embassy task-class priority. The pinned Wi-Fi
owner sets `wifi_task_core_id` to its current CPU0; the pinned BLE BTDM adapter
requires lifecycle core 0 and rejects another core. Their RTOS adapter forwards
that affinity, and no radio affinity changes are planned. CPU0 initialization binds
UART0, SPI2, I2C0 and the GPIO interrupt; CPU0 and CPU1 use FROM_CPU_INTR0/1 for
scheduler wakeups. The existing HAL profiler attributes individual interrupts to
the executing core; the baseline shows I2C_EXT0 on CPU0.

The migration must hand blocking HAL ownership to CPU1 before async conversion,
report bus readiness separately from sensor availability, and start the request
owner in fixture mode. CPU0 cannot await sensor startup while holding a resource
the sensor requires from CPU0. No initialized async driver may cross cores.

The current touch bridge has one queued owned request, one reply signal, a client
mutex and a wrapping correlation counter. It supports R, W and W/R only. Reply
correlation protects returned data, but cancellation does not remove queued work
or stop a late write; admission and reply waiting are unbounded. Its tests do not
establish the all-client lifecycle required by step 2.

Panel suspend waits for IMU, environment, battery, touch acquisition and touch
pipeline; resume starts the pipeline before acquisition. Each correlated control
wait retains its existing 2-second deadline. Several IMU acquisition sends await
the eight-slot pipeline queue outside control selection. Touch also depends on
CPU0 draining its 32-slot raw queue and application events. A CPU1 owner must keep
servicing control while those consumers are stalled; priority changes cannot
repair a producer blocked on publication.

Keep the CPU0 `wait_for_i2c_idle` rendering/persistence gates until replacement
ownership and panel arbitration pass hardware checks. Single-transaction bus
serialization cannot replace PCAL register-cache atomicity or reconcile uncertain
late writes before dependent panel transitions.

## Budgets and measurement coverage

Preserve these existing numeric contracts: touch acquisition **8 ms** active
period; IMU **50 ms** idle / **8 ms** active (20/125 Hz), hardware ODR 416 Hz;
environment and battery normal observation cadence **300 s**; transfer timeout
**40 ms**; each control acknowledgement **2000 ms**. Cadence is not an allowance
for arbitrary extra queue delay. Fault recovery and intentional panel suspension
must be reported separately from healthy sampling continuity.

Frozen candidate budgets (normal running excludes boot, explicitly acknowledged
suspension, reinitialization and failing-device recovery):

| Boundary | Budget | Derivation |
| --- | ---: | --- |
| Healthy active acquisition gap | 16 ms | Two existing 8 ms periods; IMU already marks larger gaps discontinuous |
| Healthy owner queue service | 4 ms | Half one active period, leaving transfer/service time inside the 8 ms cadence |
| Queued-request expiry | 40 ms | Distinct finite admission bound, one unchanged transfer deadline; expiry rejects rather than starts stale work |
| Owner transfer | 40 ms | Existing peripheral contract; never increased |
| CPU0 raw touch delivery | 24 ms | Three active periods: one 8 ms acquisition interval plus one scan bounded below 16 ms |
| Individual masked scan | 16 ms | Two active periods; observed maximum 12.425 ms leaves 3.575 ms, not a license to lengthen waveforms |
| Individual suspend/resume acknowledgement | 2000 ms | Existing correlated control deadline, separate from normal input latency |

These are acceptance targets, not claims that the current image meets them. An
in-flight failure may occupy one 40 ms transfer; expired queued operations must
never start. Task-poll/wake measurements include startup and are cumulative, so
classify the owning task and workload before asserting a normal-runtime violation.

Instrumentation added for the next capture:

- `CPUPROFILE` reports maximum completed poll wall time and task ID, including
  interrupt/preemption time. Existing counters still report exclusive CPU work.
- First coalesced wake-to-poll maximum searches both core registries, so a wake
  originating on the other core is attributed to the task owner. First-spawn
  latency before registration is excluded. Host tests cover coalescing, self-wake
  and wrapping microsecond time.
- `METRICS I2C` separates maximum completed mutex wait and transfer duration and
  counts transfer timeouts. Dropped/incomplete futures are excluded; these counters
  cannot prove a bound on a stuck queue or cancellation.
- Scan timing measures the closure with local interrupts masked, excluding lock
  entry/exit. It is not a measurement of every critical section in the firmware.

Counters are cumulative since boot and bounded; tracing overhead is included.
Task/IRQ table capacities, channel depths, stack reservations, priorities, bus
rates and sensor configuration are unchanged. Compare the instrumented capture
with the saved baseline before using it to judge a scheduling improvement.

## Instrumented results and gate status

`instrumented-flash/` and `instrumented-capture/` under the same log root preserve
the candidate ELF, flash hashes, unfiltered serial capture and completed/restored
result. Maxima: completed bus wait **257864 us**, transfer **38639 us**, masked scan
**12425 us**; CPU0 poll **627747 us**, wake-to-poll **645110 us**; CPU1 poll
**16996 us**, wake-to-poll **278 us**. No logged bus timeout, reset or environment
acquisition failure occurred. Physical tap/hold/swipe/WAKE/navigation and display
checks: user reported “looks good”. This does not cover injected bus faults.

Paused CPU0/CPU1 average **15.65% / 0.26%**, against **13.97% / 0.22%** before
instrumentation. About 1.69 percentage points of CPU0 difference is an observed
tracing/workload cost, not isolated causal overhead or a scheduling regression.
Use matching instrumentation for placement comparisons. The maxima include boot
and the physical interaction workload. IMU gap counts include acknowledged panel
suspensions; they cannot establish healthy continuous-acquisition gap maxima alone.

Default and minimal builds, 71 board tests and seven CPU-accounting tests pass.
The unchanged waveform and IRAM gates pass (86 flash references; baseline 87).
The source stack gate required moving an existing reviewed-NOLOAD annotation onto
its declaration line in Medinote; its allocation is unchanged. The Inkplate ELF
stack gate passes. A pre-existing BLE command-future overflow also reproduces with
all instrumentation removed. Dispatching RTC commands at the top level removes
one nested enum/future lifetime; production BLE now builds under the unchanged
2048-byte command bound. This source fix postdates the captured image.

Step 1 baseline, numeric targets and restorable image are recorded. Subsequent
transport and placement results follow; full qualification remains open.

## CPU1 owner probe and integration gates

The focused `bus-owner-probe` constructs blocking HAL on CPU0 and converts it on
CPU1. `probe-flash/` and `probe-overlap-flash/` under the execution log directory
record RTC, BME688 and 400 kHz PCAL reads, queued expiry, NACK recovery, and a
40 ms stalled-transfer deadline followed by recovery. The overlap run observed
13 I2C interrupts on CPU1 and zero on CPU0. During a 25 ms CPU0 local interrupt
mask, physical transfers stayed below 2038 us. Both runs restored the instrumented
baseline through canonical flash-capture; the user confirmed baseline touch,
WAKE, navigation and visual output as good. This does not validate the new image.

The transport owns up to four operations / 64 total bytes and one queued request.
Its atomic claim separates safely cancelled queued work from uncertain in-flight
writes. Owner completion/deadline precedes the next transaction; reply IDs never
wrap. Both admission and owner-local mutex waiting are bounded at 40 ms, separately
from the unchanged 40 ms physical transfer deadline. Local and remote clients
contend for the same short transaction lock; no client holds it across conversion
or application waits. Expander locks still span logical register updates.

Host validation passes 78 board tests including cancellation, stale replies, late
write ordering, configuration, deadline recovery and external-PCAL cache refresh.
Touch replay/control suites pass, including a full output queue whose producer
acknowledges suspend/resume without a consumer. Production acquisition and touch
pipeline publication now select control while waiting for capacity; interrupted
samples/gestures establish a reset/discontinuity boundary.

The initial integrated release passes source/ELF stack, IRAM references (86/87),
unchanged waveform placement and panel-bus gates. Sections `.data/.data.wifi/.bss`
are 24488/540/70596 bytes, versus instrumented baseline 24392/540/70324.
The CPU1 reservation remains 4096 bytes with its existing 60-byte guard offset;
owner pool is 1264 bytes, bootstrap pool 120 and request transport 232.
Runtime stack, latency, fault and panel qualification remains pending.

## Integrated runtime and bounded logging correction

`integration-flash/` rejected the CPU1 external-heap startup allocation and the
2-second readiness failure triggered updater recovery. This candidate was rejected
and `integration-failure-restore/` restored the instrumented baseline. Startup is
now allocation-free on CPU1: its future stays in the internal Embassy task pool
(1616 bytes). The lower-level cause of the external allocation rejection was not
isolated; acquisition no longer relies on external allocator/cache availability.
CPU1 stack size and both readiness deadlines are unchanged.

`internal-startup-flash/` then booted successfully, initialized BME688 before ELAN,
started all acquisition on CPU1, synchronized RTC, confirmed the image and completed
panel power/suspend cycles. The user confirmed touch, WAKE and visuals, but reported
an immediate Clock overlay after choosing Ambient Home from the launcher's Clock
entry path. The unscoped clock-tap latch could survive navigation; committed screen
changes now discard departing tap intent and pending clock work. Physical retest
of that correction remains pending.

`integration-timing/` completed normal/paused/restored capture with zero I2C timeouts
or resets. Paused totals averaged CPU0 **8.40%**, CPU1 **3.14%**, versus the
instrumented baseline **15.65% / 0.26%**. This is placement/workload evidence, not
power or thermal proof. Minimum sampled headroom was CPU0 **79472**, CPU1 **3284**
bytes. Boot-inclusive maxima were local I2C queue **4141 us**, transfer **6344 us**,
masked scan **12579 us**, and CPU1 poll/wake **65644/65214 us**. Touch's old gap
counter reached 36 ms but includes deliberate panel suspension, so it cannot prove
the healthy 16 ms target. New bounded counters retain those totals while separately
counting normal gaps, explicit suspension/recovery exclusions, and CPU0 raw-frame
delivery. Queued owner service now has its own measurement too.

Source inspection found CPU1 diagnostic formatting/transmission could synchronously
wait behind a CPU0 response. Fixing that control dependency is a prerequisite to
accepting placement: CPU1 now enqueues at most four 256-byte records for a CPU0
background drain. Overflow/oversize records are dropped and counted. UART writers
share one async mutex; each response remains contiguous across 32-byte chunks and
yields. Cancellation terminates a partial line before releasing ownership. CPU0
boot diagnostics retain their existing direct path. This is not yet the serial
readiness/priority optimization in step 4.

The bounded-logging candidate links `.data/.data.wifi/.bss/.stack` at
24520/540/73652/97884 bytes. The unchanged source/ELF stack, IRAM (86/87) and waveform
gates pass. Host tests cover full output queue control, interrupted expander writes,
record overflow without partial publication, timer wrap and latency exclusions.
Runtime qualification of this candidate remains pending in `deferred-console-flash/`.


The `deferred-console-timing/` capture was stopped after the user reported no
visible touch response during its paused phase. Cleanup acknowledged
`DISPLAYPAUSE paused=false` at uptime 306319 ms. Pause mode drains touch input but
skips LVGL timers and normal screen updates; the log contains Down/Up events during
this interval. Physical checks must run with display updates enabled, separately
from paused CPU measurements. The user subsequently reported that navigation works but the Clock returns after
a delay. The navigation correction therefore remains unqualified; this aborted
capture does not qualify the candidate.

The aborted capture also recorded healthy-gap counters of touch 18 ms, IMU 339 ms
and raw touch delivery 33 ms, exceeding the frozen targets. These observations
remain unresolved. CPU1 maximum poll/wake fell to 1048/864 us with deferred output,
but that improvement alone does not establish end-to-end latency compliance.


The user then reported visible screen corruption with normal display updates
running. `navigation-passive.log` preserves the preceding ordinary partial refresh
activity; refresh status remained `ok`, so software completion is not visual proof.
The deferred-console candidate is rejected at the step 3 display gate.
`display-failure-restore/` restores the saved `internal-startup-flash/firmware.elf`,
an earlier image on which individual touch/display checks had looked normal.
Neither image is a confirmed passing endpoint for intermittent corruption. A later
REPAINT on the earlier image looked clean, which establishes only that one physical
observation. The user requested investigation and a fix without automatic rollback;
`corruption-reproduction-flash/` reinstalls the exact previously failing artifact.
No new candidate should be accepted without isolating the corruption and repeating
the affected visual and latency checks.


## Display retest and explicit soak deferral

At the user's request, `corruption-reproduction-flash/` reinstalls the exact
previously failing ELF (SHA-256
`51f073a1a30ee9712d3fb42963c005dd027d5b8c70fc46b4be5620168dcf507d`).
`reproduction-pairs-02/` completed ten observed partial-to-full pairs with correlated
completion records, upload service left enabled and no display pause. The user
reported "no visible corruption so far" and explicitly requested that investigation
stop for now and soak tests run later. This observation does not establish a passing
endpoint or rule out intermittent corruption. That issue and final visual soak
qualification remain open while scheduling implementation continues. The exact
previously failing image was left installed for the readiness comparison below;
there was no further rollback as part of the display investigation.

## Serial readiness and independent output

The readiness-only image replaces the 10 ms receive timeout with UART readiness,
outbound queue signals and the wall-clock session's actual deadline. Each drain
has a finite per-iteration budget. `serial-readiness-before/` and
`serial-readiness-after/` capture 62 seconds each with display updates active.
Serial polls fell from 231.41/s to 105.15/s (55%); serial CPU time was 2.72% versus
2.85%. This demonstrates fewer wakeups, not a CPU-time improvement. Ordinary
display activity differed between windows. Raw counters and symbol identities are
in `serial-readiness-comparison.json` under the local artifact root above.

Metrics and trace output now run in the existing background console task. The RX
task retains the physical UART; output helpers receive only a writer capability.
One metrics operation is admitted at a time; another request receives BUSY.
Complete records retain the shared writer lock across bounded transmission chunks.
The first control probe replied in 411 ms only after the dump ended. Investigation
found immediate writer reacquisition between records and an omitted Console entry
in task-index decoding. The corrected writer yields after releasing a complete
record, and index decoding now preserves Console when profiles change.

`serial-fairness-flash/` is the currently installed default image.
`serial-fairness-after-02/` demonstrates a PING response in 21 ms with metrics lines
continuing afterward. Display updates stayed enabled. The initial `-after/` attempt
stopped before the probe because device uptime was below the capture's 60-second
minimum; it is not a runtime failure. Host tests now execute the production
scheduling policy, including all task-index round trips; all five pass.
The complete host replay suite passes 285 tests across 18 targets. Default,
minimal and BLE builds compile. The subsequent 62-second active-display capture
completed with two refreshes; there were no active acquisition/delivery samples
in its reset window, so it does not qualify those latency budgets.

Default section sizes are `.data` 24512, `.data.wifi` 540, `.bss` 74556 and `.stack`
96996 bytes. Stack-risk, IRAM-reference (86/87) and waveform-placement gates pass.
BLE compiles but its linked stack is 22116 bytes, below the unchanged 33900-byte
floor. That memory gate, acquisition/delivery latency qualification, remaining
service-profile checks and the explicitly deferred visual soaks remain open.


## Final implementation closeout — 2026-09-08

The user requested closure, confirmed that everything was responsive after reset,
and deferred navigation defects and visual soaks. This supersedes earlier
"currently installed" references and pending implementation/memory statements;
historical failures above remain evidence, not passing qualification.

The installed image is `logs/multicore-qualification-20260908/preemption-flash/firmware.elf`,
SHA-256 `91cefa7bc11a53d3b262b6021add17df1764569ce0563b864e91ff08e8fefbe1`.
The image was reset, not reflashed, after the Settings incident. The artifact was
built from a dirty working tree; the base commit does not identify its contents.
All paths below are under `logs/multicore-qualification-20260908/` (local, ignored
artifacts). The durable follow-up owner is
[REL-009](../../reference/reliability-issues.md#rel-009-multicore-integration-residual-qualification).

### Implementation and memory

- CPU1 owns the shared physical I²C bus and acquisition. The obsolete reversed
  touch proxy is removed. CPU0 retains product state and input interpretation.
- A measured 49 ms raw delivery delay during a 614833 us display poll motivated
  placing only the CPU0 touch pipeline in the Priority1 software-interrupt-2
  executor. Nested profiler context is preserved; IRQ task time is charged to IRQ
  time rather than counted twice. Masked waveform windows remain unchanged.
- Profiler snapshots use fallible CPU0 external allocation; live counters stay
  internal and retain all 132 IRQ identities. The 9600-byte CPU0 LVGL L8 drawing
  scratch moves to PSRAM; the scan framebuffer remains internal. A non-inline
  allocation boundary avoids a large transient stack frame.
- Installed default `.data/.data.wifi/.bss/.stack`: 21416/540/64988/109652 bytes.
  Final BLE: 20944/1872/73492/34764 bytes; the unchanged 33900-byte linked stack
  floor passes with 864 bytes margin. This is linked reserve, not runtime watermark
  proof. CPU1 stack remains 4096 bytes.
- `ACQWINDOW` requests 30 seconds of existing active IMU demand; it changes no
  sampling rate. `multicore-qualification.sw.yaml` owns profile order, capture and
  AUTO restoration. A zero-touch-sample window cannot qualify touch delivery.

### Verification and remaining limits

| Evidence | Result and limit |
| --- | --- |
| `board-tests.log`, `profile-preemption-tests.log`, `host-workflow-tests.log` | 76 board tests, 10 profiler tests and 4 workflow tests passed |
| Build logs | Default, minimal, BLE, binary panel fixture, updater and Medinote builds completed; fixture build is not physical startup proof |
| Stack/IRAM/waveform checks | Default allocation frames 48 bytes; IRAM 87/87 and placement checks passed; no waveform timing constants changed |
| `scheduling-01/` | Thread-mode candidate: IMU max 14 ms, touch max 10 ms; raw delivery max 49 ms failed the 24 ms budget |
| `scheduling-02/` | Interrupt candidate: interactive/upload IMU max 15 ms with no gap violations; no physical touch samples in the measured windows; local queue max 4007 us failed the 4000 us budget, remote max 355 us; run stopped before Diagnostics and restored AUTO |
| Control during metrics | PING 13 ms with metrics continuing afterward; does not establish every service-profile budget |
| `wifi-gate/` | Discovery passed; upload completed, then acceptance failed on overlapping metrics responses; three-cycle acceptance skipped. Host response draining was corrected but the gate has not been rerun |
| `settings-close/`, `settings-reset.log` | Settings Close stopped responding; subsequent attach received no serial acknowledgment. Reset restored boot and PONG. User confirmed responsiveness afterward; cause remains unknown |
| `code-ratchet.log` | Quality ratchet failed, including touched IMU, serial dispatch/parser and host workflow functions amid unrelated dirty work; no clean quality-gate claim |

Remaining runtime stack/heap margins, all-profile physical input, fixture/lifecycle
coverage and regression gates are unqualified. Clock reappearing after Launcher →
Ambient Home is still reported after reset and is deferred. Intermittent horizontal
panel bands are not resolved by an individual clean refresh or the short responsive
check. No power, thermal, CPU-time improvement or complete visual qualification is
claimed. These limitations survive plan closure in REL-009.
