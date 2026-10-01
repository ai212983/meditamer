# Runtime metrics

UART field semantics and evidence limits. Run [device validation](../../guides/diagnostics/device-validation.md)
for procedures and [serial control](serial-control.md) for commands.

## Baseline Runtime Periods

- LVGL service period: 8 ms, with panel refreshes driven by accumulated dirty areas.
- Battery task: independent Embassy task every 5 minutes (`300s`).
- Battery percentage source: BQ27441 fuel-gauge `SoC` register.

## Runtime Metrics

Runtime metrics are available over `UART0` (`115200` baud):

```text
METRICS
```

Response lines:

```text
METRICS WIFI attempt=<n> success=<n> failure=<n> no_ap=<n> scan_runs=<n> scan_empty=<n> scan_hits=<n>
METRICS WIFI_LINK rssi_last_dbm=<n> rssi_min_dbm=<n> rssi_max_dbm=<n> rssi_samples=<n> rssi_low_samples=<n>
METRICS UPLOAD accept_ok=<n> accept_err=<n> request_err=<n> req_hdr_to=<n> req_read_body=<n> req_read_body_reset=<n> req_sd_busy=<n> sd_errors=<n> sd_busy=<n> sd_timeouts=<n> sd_power_on_fail=<n> sd_init_fail=<n> sess_timeout_abort=<n> sess_mode_off_abort=<n>
METRICS UPLOAD_PHASE req=<n> bytes=<n> body_ms=<n> body_max=<n> sd_ms=<n> sd_max=<n> req_ms=<n> req_max=<n>
METRICS UPLOAD_DECOMP copy_ms=<n> copy_max=<n> sdq_ms=<n> sdq_max=<n> sdtask_ms=<n> sdtask_max=<n> commit_ms=<n> commit_max=<n> chunk_p50_max=<n> chunk_p95_max=<n> chunk_max=<n> chunk_samples=<n> chunk_drop=<n>
METRICS UPLOAD_RTT begin_n=<n> begin_ms=<n> begin_max=<n> chunk_n=<n> chunk_ms=<n> chunk_max=<n> commit_n=<n> commit_ms=<n> commit_max=<n> abort_n=<n> abort_ms=<n> abort_max=<n> mkdir_n=<n> mkdir_ms=<n> mkdir_max=<n> rm_n=<n> rm_ms=<n> rm_max=<n>
METRICS NET wifi_connected=<0|1> http_listening=<0|1> ip=<a.b.c.d>
METRICS NET_PIPELINE dhcp_wait_n=<n> dhcp_wait_ms=<n> dhcp_wait_ms_max=<n> dhcp_ready=<n> gate_wifi_down=<n> gate_link_down=<n> gate_no_ipv4=<n> listener_on=<n> listener_off=<n> accept_wait_n=<n> accept_wait_ms=<n> accept_wait_ms_max=<n>
METRICS NET_ACCEPT arm_gap_n=<n> arm_gap_us=<n> arm_gap_us_max=<n> arm_gap_after_mkdir_n=<n> arm_gap_after_mkdir_us=<n> arm_gap_after_mkdir_us_max=<n>
```

`UPLOAD_PHASE` reports per-request body/SD/end-to-end timing. `UPLOAD_DECOMP`
splits copy, SD queue/task, commit and bounded chunk-latency maxima. `UPLOAD_RTT`
reports SD roundtrip count/time totals/maxima by command phase.

## Console drop causes (Inkplate)

`CONSOLESTATS` reads cumulative UART diagnostic-drop counters without changing
them or the telemetry mask:

```text
CONSOLE_DROPS total=<n> contention=<n> deferred_overflow=<n> deferred_oversize=<n> stable=<true|false>
```

`contention` counts diagnostic writes rejected while the UART is reserved and
no runtime drain is enabled. With a drain enabled, CPU1 records and CPU0 writes
that encounter a busy UART use the same four-record queue without waiting.
`deferred_overflow` counts records rejected by that bounded queue;
`deferred_oversize` counts records exceeding its 256-byte limit. These are
diagnostic-write losses, not missing correlated responses. `stable=true`
indicates a coherent snapshot; otherwise query again before computing deltas.
Counters wrap at 32 bits and reset on boot. Compare only within one boot and
record stage boundaries so provisioning, uploads, BLE and restoration can be
distinguished. `STACKSTATUS` retains its existing aggregate `tx_drop` field.

## CPU load (Inkplate)

The default `cpu-load` feature measures busy wall time separately for both cores,
including task execution and normal maskable interrupts (levels 1–3). Sleeping or
blocked tasks do not count as busy. High-priority assembly/NMI handlers are not
instrumented; this is a utilization diagnostic, not an energy measurement or a
per-task profiler. Instrumentation overhead is included in the reading.

CPU queries read the cached typed observation without resetting counters.

```text
METRICS CPU provider=3 generation=<n> revision=<n> health=<state> attempt_ms=<n>
METRICS CPU core=<0|1> percent=<n> peak_60s=<n> elapsed_us=<n> busy_us=<n> irq_us=<n> age_ms=<n>
```

`percent` is rounded busy time divided by the actual window duration. `irq_us`
is the interrupt portion, already included in `busy_us`; nested interrupts are
counted once. `peak_60s` is the maximum five-second average in the last minute,
not an instantaneous peak. Boot's partial window is discarded. Before the first
complete observation, readings are unavailable. If scheduling delays sampling,
`elapsed_us` reports the longer window; the UI marks cached data older than
fifteen seconds unavailable.

## Hourglass timing

While Hourglass is active, firmware emits a cumulative timing report every 30 seconds:

```text
HOURGLASS_METRICS state=<...> ticks=<n> renders=<n> inputs=<n> physics_max_us=<n> render_max_us=<n> work_max_us=<n> lateness_max_us=<n> missed_periods=<n> flush_bytes_max=<n> cue_count=<n> cue_max_us=<n>
```

`physics_max_us` isolates the model step. `work_max_us` includes the optional render on that tick.
Compare both with the 30 Hz period, 33,333 us. The counters and maxima reset when Hourglass is
activated, not after each report. To exercise arbitrary-angle scheduling, start the hourglass and
trigger a 90-degree turn; the animated turn passes through intermediate angles. Compare the report
before and after the turn, especially `physics_max_us` and whether `missed_periods` increased.

## Longest I²C wait attribution

`METRICS I2C_WAIT` describes one longest completed lock attempt since
`TOUCHSCHEDRESET`: `us`, waiting `addr`, initial `holder` (`ff` means none),
`transfers`, `last8`, `busy_us`, `tail_us`, `at_ms`, `acquired`, `started_us`,
and `finished_us`.
Addresses are hexadecimal. `last8` packs the last eight released bus-owner
addresses, oldest on the left, newest in the low byte; `transfers` counts all
releases during the wait, including cancelled/failed attempts. It does not count
only successful physical transfers. More than eight releases truncate the history.

`busy_us` is overlapping lock ownership, including the owner's scheduling delay;
`tail_us` is time from the last release to this successful acquisition (zero when
no release was observed or acquisition timed out). Neither isolates wire time.
`at_ms` uses a wrapping 32-bit microsecond clock divided by 1000 (about 71 minutes).
Waits crossing a reset are omitted from attribution. CPU0 proxy requests also use
this CPU1 lock, so the address alone does not identify the originating task.

Storage is fixed and there is no per-transfer serial output. Acquisition time is
sampled before updating diagnostics; instrumentation still adds execution overhead.
The guard records release on success, error and cancellation. Compare exact
images when evaluating an arbitration change; no queue-wait pass/fail limit is
currently assigned.

The diagnostic `I2C_ADMISSION` line splits that same longest acquired wait into
FIFO admission and shared-mutex time; their sum is its `us`. `woke` says whether
the queued permit recorded a wake, and `wake_to_admit_us` measures from the
first such wake to admission. A timeout has no valid split. `I2C_TAIL_CPU`
retains total CPU1 task and IRQ time in the last-release tail. Its prep,
engine, and events fields count touch Sample CPU time within that tail; they
exclude IRQ time and time while an event send is suspended. These totals and
the existing longest `I2C_TAIL_WORK` run describe the same retained wait.
They include profiler overhead and do not by themselves assign causality.

`I2C_HOLDS` and its following `I2C_HOLD` lines belong to that same longest
touch wait, identified by `at_ms`. Each hold records the intervening client's
address and bus-lock acquisition/release times on the wrapping microsecond
clock. A hold can begin before the wait; clip its span to the `I2C_WAIT`
`started_us`/`finished_us` interval before comparing it with `busy_us`.
`finished_us` is acquisition time when `acquired=true` and timeout time otherwise.
`count` is retained holds, `transfers` is the true release count, and
`overflow=true` means the oldest holds fell outside the eight-entry history.
The spans separate bus occupancy from gaps; they do not isolate wire time or
identify CPU work during a gap. This wait is independent of the worst touch
start-gap pair.

## IMU deadline and delay attribution

`METRICS ACQUISITION` reports `samples`, `max_ms`, and `excluded` for active
touch acquisition, active IMU acquisition, and touch delivery. `TOUCH_SCHED`
reports active touch sample count and maximum start gap. Historical acquisition/delivery/bus-queue thresholds are retired; their old
counters in earlier logs are not current qualification limits.

`METRICS IMU_TIMING` retains the longest healthy active start-to-start `gap_us`,
the preceding sample's `prev_wake_us`, `prev_service_us`, `prev_publish_us`, and
the current `wake_us`, the preceding cycle's `prev_skipped` slots, and cumulative
`skipped` slots. Service includes bus
admission, sensor reads and the existing auxiliary PMIC read; it is not wire time
alone. Publication measures waiting for the input pipeline. Wake delay measures
lateness against the intended deadline. These fields describe a maximum interval,
not additive components that must sum to the gap. `TOUCHSCHEDRESET` resets them.
The existing acquisition counter remains the qualification authority; no late
healthy sample is excluded solely because it missed its deadline.

`METRICS IMU_DEADLINE` describes the preceding cycle of that same worst gap:
`rebase=1` means an explicit discontinuity reset its deadline,
`resumed=1` means it followed touch suppression, and `mode_changed=1` means
the sampling mode changed before deadline selection. These fields identify
the branch, not the cause of the I²C wait.

`METRICS IMU_READ` attributes the preceding service cycle for that same worst
`gap_us`: `at_ms` is the sample start on the same wrapping microsecond clock
used by `I2C_WAIT`, and `int_port_us` (expander),
`sensor_us` (TAP_SRC plus 12 axis bytes in one I²C transaction), and `aux_us`
(periodic idle-only PMIC power-good read; zero in active samples) are
end-to-end I²C request durations, including admission and any retry.
`other_us` is the remainder of
`prev_service_us` after those requests. These are a correlated worst-gap
snapshot, not independent maxima or a per-sample trace. A long field alone
does not distinguish queue delay from physical transfer time.

`METRICS IMU_BUS` adds three lines for that **same preceding worst-gap
sample**: stage `0` is expander port 1, `1` is TAP_SRC, and `2` is the axes
read. Stages `1` and `2` share one bus admission; its wait and configuration
are charged to stage `1`, so stage `2` has zero queue/configuration time.
`IMU_BUS_SPLIT` gives admission and mutex portions of those per-stage queue
totals. `IMU_WAIT`, `IMU_WAIT_CPU`, and `IMU_WAIT_SOURCE` retain the longest
single expander and sensor-pair waits from that preceding cycle, including
bus occupancy, free-bus tail, FIFO wake, CPU1 work, and prior holder. They can
be compared with `IMU_TIMING` and `IMU_BUS` for that cycle; they are not the
independent global `I2C_WAIT` maximum. A zero wait or `sampled=false` does
not prove absence of CPU work. `TOUCH_READ_SPLIT` gives admission and mutex
totals for each read around the worst active-touch gap.
`queue_us` sums admission/lock wait, `config_us` sums
synchronous HAL configuration, `transfer_us` sums elapsed async HAL transfer
futures, and `attempts`/`errors` include a failed
first attempt followed by a successful retry. `transfer_us` includes time
while the async future is suspended, so it is not physical wire time.
Retry delay and bookkeeping remain in the difference from the corresponding
`IMU_READ` stage duration. A cancelled future is not
recorded as a completed attempt.

## Touch read and retry attribution

`METRICS TOUCH_READ` has `prev` and `curr` lines for the longest active-touch
start-to-start gap since `TOUCHSCHEDRESET`. `prev` is the read before that gap;
`curr` is the read at its end. Each `at_ms` is that read's start. Each line
reports the full `read_us`, total bus `queue_us`, synchronous `config_us`,
async `transfer_us`, actual elapsed `retry_wait_us` across the driver's
requested 1 ms waits, `retries`, and completed I²C `attempts`/`errors`.
`other_us` is the saturating remainder after queue, configuration, transfer,
and retry waits; it includes decode and uninstrumented bookkeeping.
These cover touch-driver operations within a successful sample, including
internal retries. The gap is measured in whole milliseconds and does not
replace the acquisition-latency qualification counter. Failed samples without
a published touch count are not represented in these two lines. The prior
sample is cleared on the same explicit panel-suspend and touch-fault pauses
as the qualification counter; accumulated maxima remain available until
`TOUCHSCHEDRESET`.

`TOUCH_RETRY` gives the arm, CPU1 TIMG1 TIMER0 level-IRQ profiling-hook, and
resume timestamps for each retry in those selected `prev`/`curr` reads.
`valid=true` requires exactly one matching IRQ event within the arm-to-resume
interval; otherwise `irq_us=0` and the split is unknown. Arm-to-IRQ includes
timer setup and interrupt service; IRQ-to-resume includes the handler and task
handoff. All three timestamps wrap at 32 bits, so subtract them with wrapping
arithmetic. These lines describe only the retained worst-gap reads.

## Mountain asset timing

`MOUNTAIN_TIMING` is one summary per Mountain overlay request, including
failures. `total_us` includes card power/init and CPU/FIFO mode changes;
`stream_us` is the once-per-generation validation stream;
`histogram_read_us`/`histogram_compute_us` split the cached histogram build;
`render_read_us`/`render_compose_us` split the requested overlay. The two
read counts are completed SD range requests; with a resident pack
they should both be zero because row access is from segmented PSRAM. An
allocation failure logs `resident_alloc_failed fallback=ranges` and preserves
the previous bounded SD path. `MOUNTAIN_CACHE status=released` records why the
resident bytes were dropped. A cached-session render has zero stream and
histogram work. On failure, a partially completed row may not be
included in the corresponding aggregate, so use the surrounding FAT/error
markers as well.

## Persistence and RTC diagnostics

`APPSTATE_SAVE` and `FLASH_WRITE` mark progress through a synchronous settings
save. `METRICS PERSIST flash=<n> store=<n>` reports the latest boundaries; after
a reset, `PERSIST_PREVIOUS flash=Some(n) store=Some(n)` consumes the retained
RTC-slow breadcrumbs before the next flash access. Missing markers are printed
as `None`. Retention is reset evidence, not guaranteed across flashing or loss
of power. UART markers are best-effort and can be dropped during contention.
A CRC-valid record does not prove flash returned, core restoration completed,
or readback verification passed.

| Value | Flash boundary | Store boundary |
| --- | --- | --- |
| 0 | Idle | Idle |
| 1 | Write entered | Load entered |
| 2 | CPU-profile pause entered | Load returned |
| 3 | CPU-profile paused | Record write entered |
| 4 | CPU1 quiescence requested | Storage replacement entered |
| 5 | CPU1 quiescence skipped (not installed) | Storage replacement returned |
| 6 | CPU1 quiescence acknowledged | Verify read entered |
| 7 | Physical flash operation entered | Verification passed |
| 8 | Physical flash operation returned | Replacement or verification failed |
| 9 | CPU1 quiescence released | — |
| 10 | Write completed | — |

`RTC_DIAG op=<stage> reg=<register> addr=<address> err=<concrete error>` identifies
a failed RTC I2C transaction while preserving the existing `TIME_SYNC` protocol.
Stages distinguish snapshot reads, marker invalidation, STOP handling, calendar
and offset writes, immediate readback, and failure invalidation. The error is the
original proxy/HAL error; a stage identifies the failed transaction, not the
electrical cause. Successful transactions emit no diagnostic line.

The Inkplate proxy emits two failure-only records before `RTC_DIAG`:

```text
I2C_PROXY_TIMEOUT phase=<client_lock|publish|owner_claim|transfer_reply> id=<n> addr=0x<hex> elapsed_us=<n> claimed=<bool>
I2C_PROXY_OWNER id=<n> phase=<not_started|waiting|executing> last_id=<n> last_addr=0x<hex> age_us=<n> received=<n> discarded=<n> bus_addr=0x<hex> bus_age_us=<n>
```

`client_lock` means another proxy caller owns admission (`id=0`, since this
caller has no identity yet). `publish` means the one-slot channel did not accept
the request. `owner_claim` means the owner never claimed it; atomic cancellation
prevents it from executing later. These return `QueueTimeout`, with
`claimed=false`. `transfer_reply` returns `TransferTimeout`, with `claimed=true`:
writes may already have taken effect, even if the owner has since returned to
waiting. Queue and transfer budgets remain 40 ms each. `elapsed_us` starts at
the request and includes caller scheduling delay.

The coherent owner snapshot describes its last published phase. `age_us` is
time in that phase, including scheduling delay, not wire time. `last_id/last_addr`
identify its most recently claimed request, even while waiting. `received`
counts dequeued requests; `discarded` counts expired or cancelled requests that
never touched hardware. Counts start at boot and wrap at 32 bits. The separately
sampled physical-lock holder uses `bus_addr=0xff` and `bus_age_us=0` when idle.
`executing` means the proxy claimed a request; it includes physical-bus admission
and driver waits, and does not prove a hardware transfer started.
These observations help distinguish an unpolled owner from an owner awaiting the
shared bus; they do not identify which task prevented progress. Both records
contain metadata only and stay within the 256-byte deferred record bound.
Flash capture retains them during time-sync, together with physical
`I2C_TIMEOUT` and `I2C_ADMISSION_ERROR` records. Correlate the pair by request
`id`; `id=0` has no assigned identity, so use its adjacent failure record.
Each line is best-effort and can drop independently under UART contention or
queue overflow; a missing half is not evidence that the corresponding phase
did not occur.

For BLE handoff, `product_cleanup_failed` identifies an unsuccessful SD cleanup
fence; `quiescence_timeout` identifies remaining product work at its deadline.
Neither rejection grants radio ownership. Require the correlated
`OffConfirmed` acknowledgement before starting BLE.
If the off-state gate rejects, its `kind=off_rejected` snapshot is taken before
rollback. Inspect its ownership counters as well as memory: a zero
`block_above_reserve` can mean the probe was never reached because resources
were still owned. The later serving acknowledgement describes the restored
network, rather than the rejected off-state checkpoint.

## Startup I2C diagnostics

`I2C_STARTUP_ERROR` records address, ordered operations, lengths, first write byte,
timestamp and original HAL error. Combined write/read descriptors do not reveal
which operation NACKed; HAL `Unknown` does not distinguish address/data NACK.
A recovered sensor does not identify the original failure's electrical or
software cause.
