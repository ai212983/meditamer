# ADR-0005: Isolate touch acquisition on core 1

- Status: Accepted
- Date: 2026-07-17
- Complements: [ADR-0001](0001-fully-async-touch-acquisition.md),
  [ADR-0004](0004-dma-stepped-fat-engine.md)

## Context

Embassy task priorities keep ready touch work ahead of SD, HTTP, and network work, but cannot
interrupt a synchronous poll already running on the cooperative executor. During a 512 KiB upload,
per-sector CMD25 and busy-poll yields reduced the worst touch loop gap only from 44 ms to 38 ms;
93 samples still exceeded the 8 ms gate.

The ESP32 Wi-Fi firmware task is pinned to core 0. ESP-RTOS supports a scheduler and Embassy
thread-mode executor on core 1, but ESP-HAL deliberately makes asynchronous I2C drivers non-`Send`.
Touch, IMU, display control, and SD power also share the board's I2C0 bus.

## Decision

Run touch acquisition alone on a core-1 Embassy thread executor. Keep touch processing and product
state on core 0, connected through the existing fixed-capacity sample and event channels.

Use blocking ESP-HAL I2C behind `BlockingAsync` and the existing
`Mutex<CriticalSectionRawMutex, _>`. Each device operation retains its async interface, while each
short register transaction completes synchronously without a core-affine I2C interrupt. The mutex
serializes touch, IMU, display-control, and SD-power transactions across both cores.

Reserve a fixed 4 KiB guarded core-1 stack. Report its minimum instantaneous headroom separately
from the main executor stack; hardware gates require 1 KiB for the dedicated stack and 8 KiB for
the main stack. Scheduler profiles continue to control relative core-0 task priorities, but touch
acquisition's core assignment does not change by mode.

## Consequences

- Wi-Fi, HTTP, and FAT polls on core 0 cannot delay the 2 ms touch acquisition timer.
- Blocking I2C no longer occupies the core-0 executor while touch transactions run on core 1.
- A long or failed I2C transaction can still hold the shared bus for its configured 40 ms timeout.
- The dedicated stack consumes 4,112 static bytes including `StaticCell` bookkeeping.
- Display work explicitly reports when it cannot service queued SD-power operations, avoiding
  timeout retries while a slow e-paper render is active.
- Core-1 code must remain a narrow acquisition boundary; product state and rendering stay on core 0.

## Alternatives considered

- Reduce protocol poll batches: retained as a fairness measure, but hardware evidence showed it
  cannot provide an 8 ms latency guarantee.
- Embassy metadata priority only: retained for core-0 policy, but it is cooperative rather than
  preemptive.
- Interrupt-mode Embassy executor: rejected because shared I2C ownership would cross into interrupt
  context and the async HAL driver is intentionally non-`Send`.
- Add an unsafe `Send` wrapper around async I2C: rejected as unsound.
- Move the full application or FAT engine to core 1: unnecessary for the latency boundary and would
  expand shared-state and stack costs.

## Validation

Device 1 debug and release passed full SD cutover and ten-cycle upload gates. Worst touch loop gaps
were 4 ms and 3 ms respectively, with dedicated-stack minima of 3,332 and 3,220 bytes. Device 2
debug and release remain mandatory before rollout promotion.
