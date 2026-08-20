# Device Wall-Clock Synchronization

- Status: Proposed
- Last-reviewed: 2026-08-17

## Goal

Provide the firmware with a persistent wall clock backed by the onboard
PCF85063A RTC. Synchronize UTC time and a fixed local offset from the host over
the existing serial service interface and provide fresh RTC reads through that
interface.

This plan establishes the clock foundation only. Ambient Home integration,
NTP, BLE time synchronization, daylight-saving rules, timezone databases, and
RTC alarms remain out of scope.

## Design constraints

- Store UTC in the RTC and store the fixed local offset in its free RAM byte.
- Support calendar dates from 2000 through 2099, matching the RTC's two-digit
  year representation.
- Accept offsets from UTC-12:00 through UTC+14:00 in 15-minute increments.
- Treat an unset or invalid offset, an invalid calendar, or the RTC oscillator
  stop flag as unavailable wall-clock time. Treat an asserted STOP bit as a
  separately invalid, stopped clock.
- Use 24-hour mode as the canonical register representation.
- Do not add a task, channel, or cross-task clock cache. The serial task owns
  RTC access and performs fresh reads.
- Keep all RTC traffic on the existing shared I2C bus and use its established
  transaction timeout policy.
- Do not block normal firmware startup when wall-clock time is unavailable.
- Account for any memory growth against the [DRAM budget](../reference/dram/dram-budget.md).

## Serial interface

Add these service commands:

```text
TIMESET <utc_epoch_seconds> <offset_minutes>
TIMEGET
```

Successful synchronization returns the RTC readback:

```text
TIMESET OK utc=<readback_epoch_seconds> offset_min=<minutes>
```

Failures use a stable reason value:

```text
TIMESET ERR reason=<range|offset|i2c|verify|clock_stopped>
```

`TIMEGET` returns either a valid snapshot or an explicit reason that time is
unavailable:

```text
TIMEGET OK valid=on utc=<epoch_seconds> local=<epoch_seconds> offset_min=<minutes> os=clear
TIMEGET OK valid=off reason=<oscillator_stopped|clock_stopped|offset_unset|invalid_calendar>
TIMEGET ERR reason=i2c
```

`hostctl timestatus` exits successfully when it receives and parses either form
of `TIMEGET OK`. A transport error, parse error, or `TIMEGET ERR`
returns nonzero. This lets status inspection report `valid=off` without treating
the inspection itself as failed.

## Firmware implementation

1. Add a PCF85063A driver using a fourth `I2cDevice` at address `0x51`.
   Read and write the full BCD calendar register block in one burst and expose
   oscillator-stop, STOP-bit, 24-hour-mode, and calendar-validity checks. Use the
   [PCF85063A datasheet](https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf)
   as the register-level authority.
2. Encode the fixed offset in the RTC free RAM byte using a marked range that
   deliberately excludes `0xAA`, so the old Arduino-library marker cannot be
   mistaken for a valid offset.
3. Implement `TIMESET` as a failure-safe sequence:
   validate the input; invalidate the offset marker; read Control_1; preserve
   only `CIE` and `CAP_SEL`; force `EXT_TEST`, `SR`, `12_24`, and unused bits to
   zero; assert STOP; write the UTC calendar in one transaction; release STOP;
   write the encoded offset; then read back and verify both values with STOP and
   the oscillator-stop flag clear. On every failure path, attempt to release
   STOP with the same normalized Control_1 value and leave the offset invalid.
4. Do not issue an RTC software reset during boot. Create the fourth shared-bus
   device during system initialization, pass it through `BoardRuntimeResources`
   to `serial_task`, and construct the concrete RTC owner in `SerialTaskState`.
   The serial task starts after the existing touch bootstrap and serves fresh
   RTC reads; unavailable time does not block startup.

## Host synchronization

Add hostctl commands for explicit operation:

```text
hostctl timeset
hostctl timestatus
```

`timeset` samples the host wall clock immediately before each attempt. It
passes UTC epoch seconds and the host's current fixed offset. After an immediate
readback succeeds, it waits slightly longer than one second, issues `TIMEGET`,
and requires UTC to have advanced, STOP and the oscillator-stop flag to remain
clear, the offset to match exactly, and the delayed readback to be within two
seconds of the host's then-current UTC time.

Use a bounded readiness policy of at most eight attempts with a 700 ms delay
between attempts and a 1,200 ms serial acknowledgement timeout. Resample UTC and
the local offset immediately before every attempt; do not reuse a timestamp
captured before a readiness delay.

Extend the canonical flash-capture workflow so synchronization runs after each
capture branch (`boot`, `stream`, and `none`) and before the generic
post-command. Enable it by default for all flash and capture modes, with these
overrides:

- `--no-time-sync` disables synchronization explicitly.
- Direct `hostctl flash-capture` continues to consume
  `FLASH_SET_TIME_AFTER_FLASH=0` as a compatibility override.
- `--no-time-sync` takes precedence when the flag and environment setting are
  both present.

Record `time_sync=ok|skipped|failed`, the requested and read-back values, the
offset, and any stable error reason in the run artifacts. A failed sync must not
abort the workflow before artifacts are complete: record the failure, skip the
generic post-command, write the summary, and then run an explicit failing action.
Retain the successful flash result while returning a nonzero outcome summarized
as `flash=ok time_sync=failed`.

Keep orchestration and fallback policy in the Serverless Workflow YAML; hostctl
Rust code should provide only the primitive serial actions and context I/O.

## Documentation

- Update the [hardware test matrix](../reference/hardware-test-matrix.md) with
  RTC synchronization and retention coverage.
- Update the [build and flash guide](../guides/build-and-flash.md) with automatic
  synchronization, opt-out behavior, and artifact interpretation.

## Validation

### Host tests

- Gregorian calendar and epoch conversion, including leap years and the
  supported date boundaries.
- Weekday and BCD encoding and decoding.
- Oscillator-stop, STOP-bit, and 12/24-hour-mode handling, including
  normalization to 24-hour mode by `TIMESET`.
- Every supported offset, plus rejection of invalid granularity, reserved
  marker values, and unused encodings.
- Mock-I2C coverage for single-burst calendar access, marker invalidation,
  STOP cleanup, partial failures, and readback verification.
- Serial and hostctl parsing, range validation, fresh timestamp sampling,
  delayed advancement checks, pseudo-terminal operation, verification
  tolerances, every capture branch, readiness exhaustion, override precedence,
  summary generation, post-command skipping, and final failure propagation.

### Repository gates

- Run focused source and host tests.
- Build the firmware and hostctl targets.
- Run the static checks and repository quality lane.
- Compare the serial task pool and linked DRAM sections against the current
  baseline.

### Hardware checks

- Confirm automatic post-flash synchronization completes and reads back within
  two seconds of the host time.
- Confirm `TIMEGET` advances normally after synchronization.
- Confirm reset preserves time, and flashing with synchronization disabled does
  not alter a previously valid RTC value.
- Exercise repeated `TIMEGET` calls alongside normal display, touch, and sensor
  I2C activity.
- Disconnect main power while RTC backup power remains present, then confirm
  retained UTC and offset after startup.

Source tests and successful builds do not constitute proof of physical RTC
retention; that claim requires the backup-power hardware test.

## Assumptions

- The installed PCF85063A backup supply is present and functional.
- UTC is authoritative; local time is UTC plus the last synchronized fixed
  offset.
- The offset remains fixed until the next host synchronization. Firmware does
  not infer daylight-saving changes.
