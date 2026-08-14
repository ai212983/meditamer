## 2026-07-17: stepped FAT DMA throughput recovery candidate

- Scope: device 1, debug and release profiles, stepped FAT cutover candidate.
- The DMA-only SD path originally used 512-byte RX/TX buffers for a 546-byte
  write frame. `SpiDmaBus` therefore split every sector across two DMA
  operations. RX/TX capacity now uses the 548-byte aligned full-frame size.
- Upload begin preallocates a contiguous chain, chunks use the cached cursor,
  commit replaces an existing destination record directly, and CMD25 issues a
  best-effort ACMD23 pre-erase hint.
- The esp-radio receive queue retains all ten default static RX buffers and is
  profile-calibrated: four queued packets in debug and two in release. Debug
  needs four to meet the throughput gate; release needs two to preserve the
  16 KiB internal-memory floor.
- A nine-static-buffer candidate passed one upload but then failed boot
  discovery and was rejected. Raising upload-mode Network/HTTP scheduler
  priority was also rejected: it did not recover debug throughput with a
  two-packet queue and reduced release memory headroom with four packets.
- Serial TX now writes bounded 32-byte FIFO slices with a cooperative yield.
  This removed the debug touch-scheduling miss caused by back-to-back long
  `SDDONE`/`SDWAIT` responses.
- Disabled touch-wizard tracing now reserves one placeholder record per dump
  vector instead of the full 48/64/192-record diagnostic buffers. This reduced
  `time_sync_task::POOL` from `19,544` to `9,536` bytes and made queue-four
  debug memory headroom deterministic.
- The FAT runner now yields after each completed DMA action and also enforces a
  one-millisecond CPU slice budget. This gives higher-priority touch work an
  executor boundary before the engine advances or arms another transfer.
- PSRAM builds no longer reserve 3 KiB of unused internal HTTP fallback
  buffers. HTTP startup now requires the real PSRAM buffer allocations and
  reports allocation failure explicitly. The SD diagnostic line queue is eight
  entries; per-I/O FAT yields allow it to drain without losing required output.
- The hardware workflow waits for the readiness probe's terminal `SDDONE`
  result before starting the repeated-probe gate, preventing overlapping SD
  power requests.

Validation artifacts:

- SD baseline:
  `logs/fat_cutover_device1_debug_full_frame_dma_rxq4_baseline_20260717.log`
- one cycle with the final four-packet queue:
  `logs/fat_cutover_device1_debug_full_frame_dma_rxq4_upload_1cycle_20260717.log`
- three cycles:
  `logs/fat_cutover_device1_debug_full_frame_dma_rxq4_upload_3cycle_20260717.log`
- ten cycles:
  `logs/fat_cutover_device1_debug_full_frame_dma_rxq4_upload_10cycle_20260717.log`
- final debug ten-cycle soak:
  `logs/fat_cutover_device1_debug_fat_io_yield_psram_required_sdlog8_upload_10cycle_20260717.log`
- final debug cutover:
  `logs/fat_cutover_device1_debug_fat_io_yield_psram_required_sdlog8_cutover_20260717.log`
- final release ten-cycle soak:
  `logs/fat_cutover_device1_release_fat_io_yield_psram_required_sdlog8_upload_10cycle_20260717.log`
- final release cutover:
  `logs/fat_cutover_device1_release_fat_io_yield_psram_required_sdlog8_cutover_20260717.log`

Results:

- 1-cycle throughput: `121.13 KiB/s`.
- 3-cycle average: `124.89 KiB/s`; median: `126.42 KiB/s`.
- Final debug 10-cycle average: `121.61 KiB/s`; median: `122.18 KiB/s`;
  minimum internal free memory: `16,968 bytes`.
- Final release 10-cycle average: `160.13 KiB/s`; median: `159.83 KiB/s`;
  minimum internal free memory: `17,016 bytes`.
- Queue-three debug was rejected: `119.27 KiB/s` passed the throughput floor,
  but `15,292 bytes` failed the memory floor. Queue four before removing the
  disabled trace storage also failed memory at `13,568 bytes`.
- The first strict audit invalidated these artifacts as final acceptance evidence:
  the debug cutover had two SD power-response retries after `upload=off`, and
  the upload runs did not enforce the touch gate and recorded maximum loop gaps
  above 200 ms while verbose synchronous commit diagnostics were enabled.
- Final `sd_task::POOL` is `10,136` bytes in both profiles, 200 bytes below the
  same-profile pre-cutover value.
- Status: the throughput and memory measurements remain useful candidate
  evidence, but device-1 debug/release acceptance is reopened pending strict
  no-timeout and touch-scheduling reruns. Device-2 debug/release also remain
  required before promotion.

## 2026-07-17: strict Device-1 closure with touch isolation

- Cooperative priority and per-sector CMD25 yields were insufficient: a strict
  one-cycle debug run still recorded `loop_gap_max_ms=38` and 93 gaps above
  8 ms.
- Touch acquisition now runs alone on core 1. Shared I2C uses the standard
  blocking-to-async adapter behind the cross-core critical-section mutex; an
  unsafe `Send` override was not used.
- The dedicated core uses a fixed 4 KiB guarded stack. Final minimum headroom
  was 3,332 bytes in debug and 3,220 bytes in release.
- Debug ten-cycle average was `138.03 KiB/s`; median was `138.36 KiB/s`.
  Touch loop maximum was 4 ms, main stack headroom was 36,104 bytes, and
  internal-memory low-water was 20,024 bytes.
- Release ten-cycle average was `190.41 KiB/s`; median was `196.17 KiB/s`.
  Touch loop maximum was 3 ms, main stack headroom was 37,064 bytes, and
  internal-memory low-water was 18,840 bytes.
- The full SD cutover suite also passed both profiles: debug reported
  main/touch stack minima of 28,408/3,332 bytes and release reported
  24,712/3,220 bytes. Both had a 3 ms touch loop maximum and no timeout,
  panic, reset, or filesystem residue.
- Final `sd_task::POOL` remains 10,136 bytes in both profiles. The dedicated
  touch stack occupies 4,112 static bytes including `StaticCell` state.

Final Device-1 artifacts:

- `logs/fat_cutover_device1_debug_touch_core_sd_cutover_final_20260717.log`
- `logs/fat_cutover_device1_debug_touch_core_upload_10cycle_20260717.log`
- `logs/fat_cutover_device1_release_touch_core_sd_cutover_final_20260717.log`
- `logs/fat_cutover_device1_release_touch_core_upload_10cycle_final_20260717.log`

Status: Device 1 debug and release pass the strict cutover gates. Device 2
debug and release remain mandatory before ADR-0004 rollout promotion.
