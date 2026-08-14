# ADR-0004: Use DMA-only SPI and a stepped FAT engine

- Status: Accepted
- Date: 2026-07-17
- Supersedes: [ADR-0002](0002-phased-async-sd-spi.md)

## Context

ADR-0002 made native asynchronous SPI the default and retained DMA as an experiment. Hardware
testing at the unchanged 36 MHz data clock found native async probe hangs. DMA passed probe,
baseline, and burst checks, but a nested-directory write exhausted the approximately 32.5 KiB
executor stack. The panic address matched the CPU0 stack guard.

The dominant stack cost was not the 512-byte static DMA buffers. It was the nested asynchronous FAT
call graph: command dispatch, mount, path traversal, directory scanning, allocation, data writes,
and probe operations were all simultaneously represented in one polled future.

## Decision

SPI2 uses `SpiDmaBus<Async>` unconditionally with one static 512-byte RX buffer and one static
512-byte TX buffer. Initialization remains at 400 kHz and data transfers remain at 36 MHz.

FAT operations run through an SD-owned synchronous `FatEngine` and fixed `SdWorkspace`. The engine
contains no async functions, awaits, heap allocation, recursion, or borrowed state across steps. It
emits one probe-level I/O action at a time; the SD task awaits that action and feeds completion back
to the engine. Payload actions identify their external buffer and byte offset.

The driver yields after at most eight consecutive CPU-only transitions. Each underlying DMA
transfer has a 250 ms deadline; card data-token and write-busy waits use the same 250 ms bound, and
initialization has a two-second deadline. A sector or multi-sector action is not timed as one DMA
transfer because it contains multiple transfers and protocol waits. Timeout synchronously
deasserts chip select, invalidates card, FAT, and upload session state, and enters the existing
power-cycle retry path. Main stack size is unchanged.

Post-write readiness uses small DMA polling bursts and explicitly yields after every unsuccessful
burst. Data-token waits remain byte-oriented so polling cannot consume the start of a returned
block, but also yield after each unsuccessful token check. Each executor turn therefore performs
one bounded protocol poll; the timeout remains time-based rather than iteration-based.

SD payload writes use full-duplex DMA and discard MISO through the probe's existing sector-cache
scratch space. This keeps write completion on the same RX-EOF-backed path used by stable reads and
avoids the ESP32 TX-only SPI completion path without adding another 512-byte buffer.

## Consequences

- FAT call depth no longer becomes executor poll depth.
- Touch, IMU, network, and display tasks can run between sector operations.
- Native async SPI and the `sd-spi-dma` feature are removed; backend comparison is no longer a
  supported test mode.
- Engine/workspace state increases the static SD task pool and must be measured in each build.
- Operations require explicit stage and cleanup logic, making host action-trace tests important.

## Alternatives considered

- Increase the executor stack: rejected because it masks unbounded poll depth and consumes scarce
  internal RAM.
- Put FAT on another RTOS thread or core: deferred because it introduces another scheduler and
  ownership boundary without removing the underlying nested future.
- Keep native async SPI: rejected because the hardware probe hang makes it an unreliable default.
- Use blocking DMA in the SD task: rejected because it would starve cooperative executor tasks.

## Validation

Debug and release builds must pass the SD correctness, expected-failure, burst, nested-write, touch
scheduling, memory, stack, and upload-soak gates on both devices. Minimum stack headroom is 8 KiB;
12 KiB is the target. Internal free memory must remain at least 16 KiB and median upload throughput
must remain within 10 percent of the latest valid baseline.
