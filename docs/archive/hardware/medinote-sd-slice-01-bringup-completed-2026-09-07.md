# Medinote SD — transport bring-up

- Status: Done — installed-card transport qualified; current card support limit recorded; parent qualification remains
- Last-reviewed: 2026-09-07
- Scope: `targets/medinote-waveshare` (Waveshare ESP32-S3-RLCD-4.2)
- Parent: [Shared storage and Waveshare SD](shared-storage-and-waveshare-sd-completed-2026-09-07.md), milestone 1

## Closeout (2026-09-07)

Closed for the implemented integration and recorded installed-card evidence.
The active [parent storage checklist](../../plans/connectivity-storage-parity.md#shared-storage-integration-closeout-2026-09-07)
now owns wider cards, physical failure/power/CRC coverage, current measurement,
and combined connected-BLE qualification. No outstanding physical check is
marked passed. The requirements and evidence below are retained as history;
remaining acceptance belongs to the parent rather than this archived plan.

## Goal and prerequisites

Establish whether esp-hal 1.2.0 SDMMC + `sdio` can reliably read and write
sectors on this board within its resource budget. Start with one small probe
and one scratch card; qualify failures and compatibility once basic I/O works.

Before board bring-up, confirm the parent plan's
[HAL migration prerequisites](esp-hal-1.2-migration-completed-2026-09-07.md) are satisfied
(S3 sleep/BLE/panic and Inkplate upload qualification). Desk packaging remains
a migration delivery requirement, not an isolated-probe prerequisite. Reuse
recorded evidence; completing or rerunning that migration is separate work.
Preserve unrelated working-tree edits.

## Bring-up

1. Record the S3 internal-RAM, DMA, task-stack and interrupt budget before
   allocating transport workspaces, following the
   [DRAM budget guidance](../../reference/dram/dram-budget.md) with S3 measurements.
   The S3 `wifi-storage` image maps 156,672 bytes of PSRAM for network buffers and
   uses a 65,536-byte reclaimed internal radio heap; do not transplant the ESP32 linker
   layout or heap assumptions.
2. Add a minimal target probe beside `src/bin/*_probe.rs`, using esp-hal's
   `SdHostController` and async slot with `sdio`'s block device. Follow the
   [board pin map](../../reference/hardware/waveshare-rlcd42/board.md): GPIO21 CMD,
   GPIO38 CLK, GPIO39 D0, 1-bit width. Leave card detect unwired because the
   socket CD is grounded. Enumerate at the library's 400 kHz, then use 20 MHz
   for data. Card power is always on; recovery must reset/reinitialize the
   controller and card without claiming a rail power cycle.
3. Identify the card, read its CSD, boot sector and known sectors, then prove
   single- and multi-sector write/readback on an explicitly designated scratch
   card or reserved test region. Never automatically format or overwrite user
   media. If basic I/O fails, diagnose it before adding more infrastructure.
4. Verify bounded command/data completion, post-write busy waits, CRC/error
   reporting, reset/reinitialization and cancellation. Enforce adapter deadlines
   around waits, including esp-hal's `wait_busy_async`; exercise stuck-busy or
   missing-completion faults. Confirm peripheral/DMA access has stopped before
   releasing or reusing buffers. Successful transfers alone do not prove bounds.
5. Resolve the parent's compatibility contract: record enumeration, capacity,
   sector addressing and single/multi-sector behavior for legacy CMD8-rejecting
   cards, CMD8-capable SDSC and SDHC/SDXC with supported FAT32 geometry. The
   `sdio 0.5.1` legacy-enumeration gap needs upstream support or an explicit
   target support limit and error. Mark unavailable card classes unqualified;
   one successful card does not establish parity or change Inkplate support.

## Evidence and completion

Use the existing flash/capture workflow. Keep evidence to the probe output and
one S3 entry in the [hardware matrix](../../reference/hardware-test-matrix.md):
board, card model, firmware artifact, resolved driver versions, commands/logs,
clock, I/O and recovery results, throughput, memory and scheduling measurements,
and unsupported or untested cases. Reference that entry here when complete.

Done means sector I/O and bounded recovery work on the identified hardware,
the compatibility contract is resolved, and the measured resource budget fits.
Host tests and compile success alone do not qualify physical SD operation.

## Scope boundary

OTA, radio coexistence and broader write-CRC policy remain in the parent plan's later work. This slice
adds no document-consolidation project, archive audit, progress ledger or new
hostctl scenario. Extend existing tooling only if a concrete probe requirement
cannot be met by the current workflow; any retry/gate orchestration belongs in
existing hostctl workflow YAML.

## Implementation and next run (2026-09-07)

The isolated `sd-transport-probe` binary and `src/sd_probe/` modules implement
sector reads, explicitly enabled scratch writes, byte-for-byte pattern checks,
controller reset/reacquisition and explicit read-completion, post-write busy and
receiver data-CRC fault injection. The
small PAC cleanup closes esp-hal 1.2.0's cancelled busy-wait interrupt cleanup
gap; DMA quiescence failure halts before buffer reuse. No vendor fork was added.

Build the read-only probe from the repo root:

```bash
env -u MEDINOTE_SD_SCRATCH_START -u MEDINOTE_SD_SCRATCH_SECTORS \
  -u MEDINOTE_SD_FAULT targets/medinote-waveshare/build.sh --locked \
  --no-default-features --features sd-transport-probe --bin sd-transport-probe
```

After migration qualification, use the existing capture wrapper with
`MEDINOTE_FLASH_BIN=sd-transport-probe` and an explicitly identified
`ESPFLASH_PORT`. Its probe capture window defaults to 60 seconds; it retains
the ELF/hash and requires `SD_PROBE state=done result=ok`. That marker means
only the selected probe operations passed, not that this milestone is complete.

The build-time controls are:

| Setting | Meaning |
| --- | --- |
| Both scratch settings absent | Read-only; no sectors are written unless both settings are provided. |
| `MEDINOTE_SD_SCRATCH_START` and `MEDINOTE_SD_SCRATCH_SECTORS` | Explicit disposable region in 512-byte sectors. Both required, start above zero, count at least two, entire range within detected capacity. The first two sectors are overwritten. A scratch card means a spare whose contents may be destroyed. |
| `MEDINOTE_SD_FAULT=missing-completion` | Suppress read-completion delivery, require a 100 ms timeout, then reset/reacquire and verify data again. Unset for the ordinary run. |
| `MEDINOTE_SD_FAULT=write-busy` | Requires scratch authorization. After single-write DATA_OVER, hold the controller DAT0 input low; require observed HAL busy-wait arming and a 100 ms deadline, then quiesce/reset/reacquire/readback and another write. |
| `MEDINOTE_SD_FAULT=read-crc` | Reacquire at 1 MHz and invert the receiver's DAT0 input only during payload bytes 128–256. Require raw data-CRC status and the actual driver CRC error; restore routing and recover at 20 MHz. No pin output is driven by either fixture. |

The user authorized overwriting the installed card on 2026-09-07. The prepared
scratch build uses start `2048`, count `2` (two sectors, checked against capacity
before writing); this is probe scope, not a claim that those sectors were unused.
Clear these variables when returning to read-only builds. Enumeration has a
5-second deadline; ordinary I/O has 2 seconds and cleanup/reset waits are each
bounded at 100 ms. Deadlines are cooperative; logs separately report maximum
synchronous poll duration, pending-timer lateness and sampled stack space.
They do not establish deepest-call stack high-water or product responsiveness.

Current desk evidence and open qualification cases are recorded in
[hardware matrix §2L](../../reference/hardware-test-matrix.md#2l-medinote-native-sd-transport-probe).
HAL hardware gates passed; migration commit/vendor packaging remains in its
owner plan. The installed 16 GB high-capacity card passed single/multi-sector
write/readback and reset/reacquire at 20 MHz, using sectors 2048–2049. The
missing-completion run timed out at 100097 µs and recovered with boot/scratch
data intact. Evidence: `logs/medinote-sd-basic/` and
`logs/medinote-sd-missing-completion/`, with artifact hashes and measurements
in matrix §2L. Normal production firmware was restored and boot-checked afterward in
`logs/medinote-sd-production-restored/`; the device no longer runs a panic or SD probe.

The follow-up write-busy fixture observed clean DATA_OVER and HAL's busy-wait
arming, then timed out at 100513 µs. The read-CRC fixture corrupted receiver
bytes 128–256 at 1 MHz and observed raw data-CRC plus the driver's `Crc` error.
Both restored routing, reset/reacquired at 20 MHz, checked preserved data and
verified a changed post-recovery write. Final artifacts and resource measurements
are in matrix §2L and `logs/medinote-sd-{write-busy,read-crc}-final/`.

Physical no-card enumeration and reset/retry returned bounded timeout errors;
after reinsertion and a fresh probe boot, enumeration, reads and reset/readback
passed without writes (`logs/medinote-sd-no-card/`, `logs/medinote-sd-reinserted/`).
Live hot-plug admission/recovery remains product lifecycle work. Normal
production was restored and boot-checked after these fixtures in
`logs/medinote-sd-faults-production-restored/`.

The parent's shared storage/FAT integration now runs in production: file operations,
sleep/abort remount, no-card errors and reinsertion without reboot passed in
`logs/shared-storage-medinote-final/` (hardware matrix §2N). Only this card is available; other classes remain explicitly
unqualified, and legacy CMD8-rejecting cards remain unsupported by the current
sdio dependency. Failed hardware reset, physical electrical-noise tolerance and
write/response CRC still require separate evidence. These remaining coverage
limits are recorded rather than treated as proof of card parity. HTTP integration uses the
shared `platform/http-upload` handler; parent qualification remains open for wider cards,
current and connected-BLE coverage.
