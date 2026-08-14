# ADR-0001: Fully async touch acquisition

- Status: Accepted
- Date: 2026-07-16

## Context

Touch sampling previously lived in the display loop and used blocking I2C calls. Display refresh,
sensor work, and touch acquisition therefore competed through control flow rather than an explicit
bus boundary. A held contact also depended on repeated reads to advance gesture timers. This made
sampling cadence sensitive to rendering and made a future second-core deployment unnecessarily
hard.

GPIO36 is shared by the active-low WAKE button and touchscreen interrupt. The line identifies an
assertion, but the firmware must read the controller to classify its source. The controller also
emits intermittent zero frames during held contacts.

## Decision

Use the Embassy async I2C adapter behind `embassy-embedded-hal`'s mutex-backed shared-bus device.
Display, sensors, and touch receive independent device handles and await bus ownership.

Give a dedicated Embassy task ownership of:

- the touch-controller driver and power/reset state;
- GPIO36 level observation and touch/WAKE classification;
- touch initialization, retries, recovery reads, and raw sample publication.

Keep normalization and gesture recognition in a separate task. It consumes timestamped samples
and advances gesture time every 8 ms without performing I2C. Acquisition reads at 8 ms while a
contact is active so motion remains observable. An explicit decoded contact remains active until a
zero report is observed; continuity grace then expires from logical time.

Keep both tasks on the main executor initially. Their owned state and channel messages do not
borrow display state, so acquisition can move to a second executor/core later without redesigning
the protocol. GPIO36 remains level-polled at 2 ms because this ESP32 input is shared and needs
source classification; controller reads are event/recovery driven rather than continuous.

## Consequences

- Display refresh can no longer directly delay touch acquisition through a shared display loop.
- All Inkplate I2C operations are async and serialized at the bus boundary.
- Gesture timers advance independently, while active motion samples remain capped at 125 Hz.
- Touch hardware ownership is separate from panel/display ownership.
- A second-core move is optional optimization, not an architectural migration.
- Acquisition timing and gesture continuity are testable without hardware.

The shared I2C mutex is not a priority scheduler. Long multi-transaction display operations must
continue yielding between transactions, and hardware validation remains required for swipe
smoothness and GPIO36 source classification.

## Alternatives considered

- Keep touch in the display task: rejected because rendering remains coupled to sample latency.
- Dedicated blocking RTOS thread: workable, but it preserves blocking bus ownership and adds a
  second concurrency model beside Embassy.
- Move directly to core 1: deferred until measurements show contention; cross-core execution adds
  synchronization and ESP32 peripheral-affinity constraints without fixing ownership by itself.
- Poll the controller continuously during contact: rejected as the primary timer mechanism because
  it adds bus traffic and still couples gesture progress to successful reads.
