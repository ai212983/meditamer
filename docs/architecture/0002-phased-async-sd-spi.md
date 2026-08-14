# ADR-0002: Phase SD SPI through native async before DMA promotion

- Status: Superseded by [ADR-0004](0004-dma-stepped-fat-engine.md)
- Date: 2026-07-16

## Context

The SD/FAT API used async functions, but `SdCardProbe` owned `Spi<Blocking>` and its sector,
command, token, and card-ready operations called blocking embedded-hal SPI methods. Those calls
could occupy the Embassy executor while touch acquisition, networking, and other tasks were ready.

The ESP32 SPI2 peripheral supports both interrupt-driven FIFO operation and DMA. DMA can reduce
CPU work for sector transfers, but it requires internal DMA buffers. Upload payloads may live in
PSRAM, so DMA still copies through fixed internal buffers and is not automatically faster.

## Decision

Make the SD protocol generic over an asynchronous SPI bus while retaining exclusive SPI2 and
chip-select ownership in the SD task.

Use native `Spi<Async>` as the default transport. Preserve 400 kHz initialization, the configured
36 MHz data default, command framing, poll limits, and CMD25 fallback. Await all transfers and add
explicit cooperative yields to long one-byte token and ready loops. Token searches remain
byte-wise because reading beyond the first token would consume payload bytes.

Provide `SpiDmaBus<Async>` behind the non-default `sd-spi-dma` feature. It owns one 512-byte RX and
one 512-byte TX buffer in internal DMA-capable memory. Existing PSRAM upload buffers are copied
through those buffers by esp-hal.

DMA is promoted to the default only after matched hardware runs show either at least 10% higher
median upload throughput or at least 20% lower p95 SD-handler time, with no correctness,
scheduling, memory, discovery, or reliability regression.

## Consequences

- SD waits no longer busy-spin on the Embassy executor.
- Native and DMA transports share one SD protocol implementation and error model.
- The default path pays interrupt wakeup overhead for each 64-byte FIFO chunk.
- DMA adds approximately 1 KiB of payload buffers plus descriptors in internal RAM.
- DMA remains opt-in until evidence satisfies the promotion gate; there is no runtime fallback that
  could hide transport faults.

## Alternatives considered

- Keep blocking SPI inside an SD task: rejected because task ownership does not prevent executor
  starvation on a cooperative executor.
- Promote DMA immediately: rejected because PSRAM copies, small-command overhead, and internal RAM
  cost need measurement.
- Allocate DMA buffers as large as upload chunks: rejected because 64 KiB internal buffers are not
  compatible with current Wi-Fi memory headroom.

## Validation

Both transports must build in default and slim configurations and pass the complete SD hardware
suite, burst regression, touch scheduling limits, and Wi-Fi/upload regression gate. Native async
must remain within 10% of the fresh blocking throughput baseline before DMA comparison begins.
