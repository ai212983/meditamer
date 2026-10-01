# Shared Storage and Waveshare SD

- Status: Done — shared integration qualified on installed-card artifacts; wider acceptance transferred to parent
- Last-reviewed: 2026-09-07
- Parent: [Connectivity and storage parity](../../plans/connectivity-storage-parity.md)
- Related: [Shared Wi-Fi services](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md),
  [BLE asset upload](../../plans/ble/06-asset-upload.md),
  [esp-hal 1.2.0 migration closeout](esp-hal-1.2-migration-completed-2026-09-07.md) — this plan's S3 transport
  arrived with that bump. Milestone 1 requires that plan's hardware gates:
  S3 sleep, BLE and panic qualification, plus the existing Inkplate Wi-Fi upload regression.
  Those hardware gates passed on 2026-09-07; the migration plan is closed by user decision,
  with delivery limitations retained in its archived closeout.
  Medinote HTTP-to-SD upload is integrated and device-tested; wider physical qualification remains.

## Closeout (2026-09-07)

Closed for the implemented integration and recorded installed-card evidence.
The active [parent storage checklist](../../plans/connectivity-storage-parity.md#shared-storage-integration-closeout-2026-09-07)
now owns wider cards, physical failure/power/CRC coverage, current measurement,
and combined connected-BLE qualification. No outstanding physical check is
marked passed. The requirements and evidence below are retained as history;
remaining acceptance belongs to the parent rather than this archived plan.

## Outcome

Meditamer/Inkplate and Medinote/Waveshare expose the same supported SD file and upload
operations through one reusable storage service. Each target supplies its own transport,
resource allocation, and board power handling. Capability parity does not require equal
throughput or the same electrical interface; firmware updating and bootloader parity are
outside this plan.

## Current evidence

- [The Waveshare board reference](../../reference/hardware/waveshare-rlcd42/board.md#pin-map)
  identifies GPIO21/38/39 as native 1-bit SD `CMD`/`CLK`/`D0`, with no card chip select.
  The fitted slot cannot be treated as the existing SPI device with different pins.
- The vendor's own firmware confirms that reading and settles the working configuration:
  [`06_SD_Card/components/port_bsp/sdcard_bsp.h`](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2/blob/main/02_Example/ESP-IDF/06_SD_Card/components/port_bsp/sdcard_bsp.h)
  mounts the slot through `driver/sdmmc_host.h` with `clk = 38, cmd = 21, d0 = 39,
  width = 1`. The slot works as shipped in native 1-bit mode; no board rework is
  needed, and SPI mode is not the path.
- **An S3 transport now exists.** esp-hal 1.2.0 (released 2026-09-03; PR
  [esp-rs/esp-hal#5760](https://github.com/esp-rs/esp-hal/pull/5760), merged 2026-07-09)
  ships an SDMMC/SDIO host driver covering ESP32, ESP32-S3 and P4. It has what this board
  needs: GPIO-matrix pin routing, `BusWidth::Bit1`, an async interrupt-driven mode, DMA
  with a bounce path for cached/PSRAM buffers, and an *optional* card-detect pin — ours is
  grounded at the socket, so it simply goes unwired. Card enumeration is not ours to write
  either: `Slot<Async>` implements `sdio::MmcBus` from
  [embassy-rs/sdio](https://github.com/embassy-rs/sdio), whose `BlockDevice<…, 512>`
  implements `block_device_driver::BlockDevice` and owns CMD0/8/ACMD41/CMD2/3/9/7, CSD
  parsing, SDSC-vs-SDHC addressing, and CMD23/auto-stop multi-block transfers.
  This is not yet proof of card compatibility parity: the resolved `sdio 0.5.1` requires
  CMD8 success during enumeration, whereas the existing SPI driver handles legacy cards
  that reject CMD8. Addressing support alone does not establish legacy enumeration support.
  This supersedes the earlier finding that no SDMMC driver existed, which was true only of
  the then-pinned esp-hal 1.1.1 and its `vendor/esp-hal-1.1.1-s3-sleep-fixes` copy.
- The esp-hal 1.2.0 adoption is done: both targets are on `=1.2.0`, and the vendored esp-hal
  fork is deleted (all three of its backported S3 sleep fixes are ancestors of the 1.2.0 tag).
  [The SD manifest](../../../platform/sdcard/Cargo.toml) no longer selects a chip, so it follows
  the same target-driven feature unification as every other `platform/` crate.
- [The S3 target manifest](../../../targets/medinote-waveshare/Cargo.toml) now includes
  the isolated `sd-transport-probe`. The [transport slice](medinote-sd-slice-01-bringup-completed-2026-09-07.md)
  records successful installed-card single/multi-sector I/O, reset/readback and
  injected read-timeout/busy recovery at 20 MHz and receiver data-CRC recovery
  after a 1 MHz injection. No-card errors and reinsertion/reboot reads also passed.
  Wider card/failure coverage remains open; production now installs the shared FAT service.
- [The FAT engine](../../../platform/sdcard/src/fat/engine.rs) already advances through
  `FatIoAction` requests without async frames. `platform/sdcard/service.rs` now executes
  those actions for both products, including file, stream and upload-session operations.
  Shared request parsing lives alongside it. Product queues, HTTP policy and transfer
  buffers remain in Meditamer; Medinote installs its own bounded native-SD owner.

  The shared `sdcard::upload` module owns upload sessions, temporary publication and
  path policy for both consumers; target owners provide only transport, queue and policy
  adapters.

## Implemented integration (2026-09-07)

- Shared sector error vocabulary preserves driver detail and distinguishes integrity,
  timeout, unavailable media, bounds, protocol and I/O failures. SPI reads now validate
  CMD9/CMD17 CRC16 before metadata decoding or cache publication.
- The shared executor retains FAT/session state, list and stream callbacks, contiguous
  action batching, bounded CPU yielding, and cancellation between completed I/O actions.
  Transport failure invalidates filesystem/session state. Board rail recovery remains
  Inkplate policy; native SD recovery resets/re-enumerates without claiming a power cycle.
- Medinote's default `sd-storage` task lazily enumerates the card. Its existing console
  accepts the shared SDFAT file commands and streams complete file reads in 128-byte console
  frames. Two queued requests have correlated IDs and ten-second eligibility; in-progress I/O
  and recovery finish safely before deadline acknowledgement.
  Sleep closes admission, cancels queued work and awaits an epoch-specific quiescence
  acknowledgement (eight seconds); resume forces fresh enumeration/mount.
- The 4,768-byte engine is constructed directly in static internal RAM. A single aligned
  512-byte transport workspace avoids PSRAM. Native contiguous actions currently use
  sector-sized transfers within one two-second batch deadline; Inkplate retains CMD25.
- Shared executor/parser/CRC tests and the complete Inkplate SD/Wi-Fi upload gates pass.
  Medinote's installed-card file operations and lifecycle evidence are in hardware matrix
  §2N. The S3 `wifi-storage` composition now supplies the shared HTTP service and SD-backed
  credential path; broader card coverage, current qualification and connected-BLE coverage
  remain open. This does not close the parent parity plan.

## Ownership and contract

The installed-card qualification now includes the full S3 three-cycle HTTP gate,
512 KiB byte-exact readback, interrupted-session sleep cleanup, retained Wi-Fi
enablement after deep sleep, and removal/reinsertion recovery without reboot.
See hardware matrix §2O for both artifacts and evidence. Full/corrupt-volume
host tests pass; other physical card classes, abrupt power interruption and
seated-card idle/sleep current remain unqualified. The user has no current
measurement hardware available. These limits keep the parent plan active.

| Concern | Owner |
| --- | --- |
| FAT32 engine, file semantics, shared operation/session state | `platform/` storage service |
| Bounded sector I/O, timeout/CRC/error normalization | Chip transport adapter below the shared service |
| Card pins and board power sequencing | `boards/*` adapters |
| Chip feature selection, DMA/buffer placement, task installation | `targets/*` |
| Asset interpretation, UI commands and product policy | `products/*` |

Each target has one storage operation owner. All FAT access, upload sessions, credential-file
I/O, and product reads go through it; HTTP and BLE never open an independent filesystem or
drive the card directly. Preserve the existing bounded request correlation, backpressure,
cancellation acknowledgement, remount/recovery, and temporary-file publication behavior.

Use the existing FAT action boundary to separate SPI-specific error and execution details;
do not duplicate the FAT implementation. Retain the supported FAT32/card geometry contract
and existing list/stat, read/stream, write/append/truncate, directory, remove, rename, and upload operations.
Any unsupported format or target limit must produce an explicit error.

### Recovery and board power

The shared owner must distinguish switchable and always-powered media. Inkplate's
[existing retry path](../../../products/meditamer/src/firmware/storage/sd_task/dispatch.rs)
cycles the card rail; Waveshare's VDD is wired directly to 3.3 V with no load switch.
A no-op power callback must not report a successful physical power cycle.

Retain Inkplate's bounded power-cycle recovery. For Waveshare, stop peripheral/DMA access,
invalidate transport caches and filesystem/session state, then attempt bounded controller
reset and card re-enumeration. If recovery fails, report media unavailable and terminate
accepted work within its deadline; further recovery follows explicit retry/remount policy,
with physical card removal or board power cycling documented when required.
Sleep quiesces the host but leaves the card powered. Qualify wake reinitialization and
measure the seated card's contribution to idle and sleep current.

### Read integrity

The SPI driver now validates received CRC16 bytes. Retain validation on every card-read
path: CMD9 CSD blocks, CMD17 initialization/filesystem reads, and cached sector reads.
Confirm the card's CRC-generation requirements during implementation. Verify the received
CRC before decoding metadata, returning successful data, or marking a cache entry valid.
A mismatch returns a distinct integrity error and leaves the failed read uncached; finish
the transaction safely and use the owner's bounded recovery policy without accepting the
corrupt payload. Preserve completed-DMA ownership during cleanup.

The S3 SDMMC adapter must propagate controller CRC failures through the same integrity
contract. This work covers read validation; command/write CRC policy is a separate change
unless the selected card protocol requires it for reliable read verification.

## Delivery

### 1. S3 transport and resource feasibility

1. Record the S3 internal-RAM, DMA, task-stack, interrupt, and optional PSRAM budget before
   allocating transport workspaces. Follow the discipline in the
   [DRAM budget](../../reference/dram/dram-budget.md), with S3-specific measurements; do not
   transplant ESP32 linker layouts, heap sizes, or safety margins.
   The S3 `wifi-storage` image maps 156,672 bytes of octal PSRAM for network buffers and
   adds a 65,536-byte reclaimed internal heap for radio allocations. Keep controller objects,
   task state and DMA workspaces in their qualified internal placements; measure the complete
   image rather than treating the PSRAM byte buffer as a general allocator.
2. Compose the transport from the adopted drivers rather than writing one: esp-hal 1.2.0's
   `SdHostController` → `slot(…).with_clk(GPIO38).with_cmd(GPIO21).with_data0(GPIO39)` →
   `into_async()`, handed to `sdio`'s block device. Do not wire `with_card_detect`: the
   socket's `CD` pin is tied to ground, so insertion and removal are not observable and
   remount policy must be poll-or-error driven. Enumerate at the library's 400 kHz, then
   use 1-bit width and **20 MHz** for data — what
   `SDMMC_HOST_DEFAULT()` gives the vendor example, and the ceiling Zephyr models for this
   slot — and treat anything faster as a later, measured change.
3. Build a minimal target probe binary alongside the existing `src/bin/*_probe.rs`:
   enumerate and identify the card, read the CSD, boot sector and known sectors, then
   perform write/readback only on an explicitly designated scratch card or reserved test
   region. Never format or overwrite user media as an automatic bring-up step.
4. Prove bounded command/data completion, card-busy timeouts, CRC/error reporting, reset and
   reinitialization, and cancellation that releases no buffer until peripheral/DMA access
   has stopped. esp-hal 1.2.0's `wait_busy_async` waits for a post-write interrupt without
   its own deadline. Enforce adapter deadlines, including this wait, and verify abort
   cleanup before buffer reuse, retry or sleep. Exercise stuck-busy/missing-completion
   faults; successful transfers alone do not prove bounds. Measure scheduling latency and
   memory use; a blocking vendor example is not async product integration evidence.
5. Record a compatibility matrix covering legacy CMD8-rejecting cards, CMD8-capable SDSC,
   and SDHC/SDXC with supported FAT32 geometry. Check enumeration, reported capacity,
   512-byte sector addressing and single/multi-sector I/O on each supported class.
   Resolve the `sdio` legacy-enumeration gap through upstream support or an explicitly
   documented target support limit and error. Preserve Inkplate's existing support;
   do not infer common card support from one successful SDHC probe. Mark untested classes
   unqualified and record the resolved driver versions with the evidence.

Exit: the identified S3 board/card/artifact reads and writes sectors correctly, recovers
from bounded failures, resolves the compatibility contract, and fits the measured resource budget.
Record the clock, card model, single/multi-sector behavior and throughput as evidence,
not a product speed promise.

This milestone carries more weight than it would for a settled driver. Adopting esp-hal's
SDMMC makes this project an early user of code with no upstream hardware coverage yet, so
bring-up is the qualification, not a formality: a failure here is as likely to be a driver
bug worth reporting upstream as it is our own wiring, and the two need separating before
any extraction work is committed on top.

### 2. Shared storage service

1. Census the Meditamer operation owner, transfer buffers, request types, product callbacks,
   and transport-specific errors. Name their destination modules at real ownership boundaries.
2. Make `platform/sdcard` chip-neutral where reusable; select each concrete transport from
   its target. The manifest half of this is already done — the crate no longer names a chip
   — so what remains is the code seam: a block-device trait the FAT engine's I/O executor
   targets, implemented twice: the existing SPI probe (Inkplate) and `sdio`'s block device
   (Waveshare). Preserve SPI behavior except for the required read-integrity correction;
  it remains the qualified product file-service path; S3 wider physical qualification
  remains open. Keep the stackless
   FAT engine and normalize transport completions at the I/O boundary, retaining diagnostic
   detail without exposing SPI types as the common API.
3. Move the operation/session owner into the shared layer, preserving its file/session
   semantics with bounded ports for product policy, board power capabilities, telemetry,
   and buffer resources. Implement the recovery contract above without requiring rail
   switching on Waveshare. Preserve Meditamer as the first consumer and its current
   capacities until a measured change is justified.
4. Compose the same service into Medinote with the proven S3 transport. Connect shared
   credential persistence and HTTP work under the [Wi-Fi plan](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md);
   reserve the same owner for later [BLE uploads](../../plans/ble/06-asset-upload.md).

Exit: both products use one operation implementation, and host conformance tests cover
both transport adapters' completion/error mapping and the shared filesystem/session contract.
Include switchable/always-powered recovery, exhausted deadlines, and unavailable-media outcomes.

### 3. Lifecycle and qualification

- Coordinate admission with the radio and sleep owners: stop new work, drain or abort
  accepted requests, confirm quiescence, then release hardware. Wake/restart creates valid
  fresh sessions and reports unavailable media without hanging UI or network tasks.
- Exercise no-card, unsupported/corrupt filesystem, full card, removal or equivalent I/O
  failure during a write, timeout, abort, retry, power interruption, and remount. Verify
  final-file integrity and temporary-file cleanup; FAT32 is not a journaled filesystem,
  so document the demonstrated interruption guarantees without promising arbitrary
  power-loss atomicity.
- Run supported file operations and byte-for-byte upload/readback on both boards. Measure
  internal free/contiguous memory, DMA placement, stack headroom, task responsiveness and
  storage latency in each full product image, including the agreed radio policy.

## Validation and completion

Extend existing host FAT/storage tests and source ownership/stackless guards; run formatting,
orphan-module, code-size, and both target feature/build checks after extraction. Host tests
and compile success do not qualify physical SD operation.

Read-integrity tests must cover independent CRC16 vectors and wire byte order, valid CSD
and sector reads, corrupted payload and CRC bytes, unchanged caller output on failed
cached reads, cache invalidation and a subsequent valid reread, and transaction cleanup.
Exercise error propagation through the FAT owner so corrupt metadata cannot trigger a
follow-on mutation. Qualify real-card reads and measure CRC cost in the complete Inkplate
image; require corresponding CRC-error handling in the S3 transport qualification.

Extend existing SD and upload hostctl scenarios for target selection and the new transport;
keep retries, gate flow, and recovery orchestration in `tools/hostctl/scenarios/*.sw.yaml`.
Use the canonical flash/capture workflow and record board, card, artifact, command, logs,
and limitations in the [hardware matrix](../../reference/hardware-test-matrix.md).
Run the [Wi-Fi regression gate](../../guides/wifi-regression-gate.md) when shared upload or
network work changes, preserving Inkplate coverage and adding S3 coverage under the Wi-Fi plan.

Done means storage parity is demonstrated on both complete product images, the reusable
service has no product/board dependency, and the reference/workflow docs describe the
implemented capabilities and measured limits.
