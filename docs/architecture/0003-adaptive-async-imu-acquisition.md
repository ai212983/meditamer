# ADR-0003: Adaptive async IMU acquisition

- Status: Accepted
- Date: 2026-07-16

## Context

LSM6DS3 reads, tap classification, face-down handling, and resulting display effects previously ran inside `display_task`. The sensor itself operated at 416 Hz, but firmware sampled at the display loop's nominal 20 Hz cadence and could be delayed further by rendering. Increasing direct polling to 416 Hz would consume nearly all of the shared 100 kHz I2C bus.

The internal GPIO expander carries IMU INT1 and INT2 while also controlling panel and board outputs. Its output/configuration cache belongs to `InkplateHal`, so independent IMU ownership must not duplicate writes to that cache.

## Decision

Use a dedicated async IMU acquisition task and a separate pure event-pipeline task on the existing Embassy executor.

The acquisition task owns a standalone `InkplateImu` driver and a shared-bus I2C device. It reads the LSM6DS3 and accesses the expander only through read-only input registers. Initialization, retries, adaptive deadlines, touch suppression, and upload suspension belong to acquisition.

Configure three separate rates:

- sensor ODR: 416 Hz;
- idle direct polling: 20 Hz;
- active direct polling: 125 Hz.

Tap, interrupt, jerk, gyro, sequence, or recovery evidence extends a high-rate window. Direct polling is limited to 125 Hz on the current bus. Sampling discontinuities clear stale motion history before classification resumes.

The pipeline consumes ordered timestamped frames through a bounded channel, owns `EventEngine` and face-down state, and never awaits display work. It publishes idempotent backlight requests and parity-preserving background toggles through a nonblocking mailbox. `display_task` remains the sole owner of frontlight, app-state, and rendering effects.

Touch activity reaches acquisition through a latest-value signal before display consumption, preserving touch priority without borrowing display state.

## Consequences

- Display refresh no longer defines IMU cadence.
- The first latched tap can promote subsequent motion capture to 125 Hz.
- Touch and upload modes stop IMU bus traffic explicitly.
- Event processing and acquisition can later move to another executor/core without protocol redesign.
- Metrics expose idle/active samples, gaps, suppression, faults, recovery, and action coalescing.
- Current direct polling does not deliver every 416 Hz sensor frame.

Future full-rate capture will use LSM6DS3 FIFO batches, reconstructed timestamps, and overflow/discontinuity reporting. FIFO acquisition may emit several ordered `SensorFrame` values per bus transaction without changing the pipeline or display interfaces. Raising the shared bus to 400 kHz requires separate compatibility validation.

## Alternatives considered

- Keep IMU work in `display_task`: rejected because rendering remains the scheduler.
- Poll directly at 416 Hz: rejected because transaction overhead would monopolize the current shared bus.
- Always poll at 125 Hz: workable, but unnecessary bus and power cost while stationary.
- Move directly to core 1: deferred until measurements justify cross-core complexity; ownership is separated first.
