# DRAM Budget

Internal DRAM, not flash, is the binding constraint on this firmware. This file
records where it goes, what was recovered, and how to re-measure.

## The Budget

esp-hal exposes ESP32 internal data RAM as two windows:

| Region | Range | Bytes | Notes |
| --- | --- | ---: | --- |
| `dram_seg` (SRAM2) | `0x3FFB0000`-`0x3FFE0000` | 196608 | `.data`, `.bss`, `.stack` |
| `dram2_seg` (SRAM1) | `0x3FFE4350`-`0x40000000` | 113840 | `.dram2_uninit` only |

`dram2_seg` is extended 15072 bytes below esp-hal's default `0x3FFE7E30` by
`ld/meditamer-memory.x`; see
[DRAM Budget: Reclaiming the ROM Stack](./dram-budget-rom-stack.md).

`dram_seg` is always 100% consumed, because `.stack` is defined in esp-hal's
`stack.x` as whatever is left after `.data` and `.bss`. **Every static byte in
`dram_seg` is a byte the CPU0 stack does not get.** That is the whole reason
this budget matters.

`dram2_seg` sits above the ROM data/stack reservations. It cannot back the CPU0
stack, so capacity placed there is free from the stack's point of view.

IRAM is not the constraint (71508 of 130048 used), and ESP32 IRAM is
instruction-only, so it cannot absorb data.

## Baseline (before recovery)

Measured on the default feature set, release profile.

| Item | Bytes | Source |
| --- | ---: | --- |
| esp-alloc internal heap | 65536 | `src/firmware/psram/core.rs` |
| CPU0 stack (remainder) | 36492 | esp-hal `stack.x` |
| Embassy task pools (14 tasks) | 35368 | `#[embassy_executor::task]` |
| Wi-Fi/PHY blob statics | 15455 | `g_cnxMgr`, `s_wifi_nvs`, ... |
| Jump tables forced into RAM | 13168 | `ESP_HAL_CONFIG_PLACE_SWITCH_TABLES_IN_RAM` |
| Static channels / trace buffers | 13105 | `src/firmware/config/channels.rs` |
| Core-1 touch stack | 4112 | `src/firmware/runtime/bootstrap/tasks.rs` |
| Grayscale waveform LUT | 2048 | `src/drivers/inkplate/mod.rs` |
| embassy-net `StackResources` | 1864 | `src/firmware/storage/upload/mod.rs` |
| SD DMA buffers | 1096 | `src/firmware/runtime/bootstrap.rs` |
| esp-hal / esp-rtos / esp-phy / misc | 8364 | `PHY_STATE`, `SCHEDULER`, ... |

Largest task pools: `sd_task` 10160, `serial_task` 9648, `display_task` 5424,
`board_runtime_task` 3920.

`dram2_seg` held the two 45000-byte panel framebuffers, leaving 8768 free.

## Runtime Evidence

From the serial `PSRAM`/`psram:` telemetry across captured runs:

- worst internal-heap high-water: `min_internal_free_bytes=17016` against the
  64 KB region, so peak internal use is about 48520 bytes
- `large_alloc_internal_ok=0`, `large_alloc_fail=0`: every large buffer lands in
  PSRAM, so the internal heap only ever serves small allocations

A `min_internal_free_bytes=6` line appears in one capture. It is a truncated log
line, not a real near-exhaustion.

### UI shell reservation and latest lifecycle measurement

Capacities remain providers 8, surfaces 16, navigation 8, overlays 4, modals 4, intents 8, callback
routes 2, and UI-step acknowledgements 2. Retained payload is zero; four future references are unallocated.

| Exact debug layout | Xtensa bytes |
| --- | ---: |
| `ShellModel` (`960` host) / `CompositionReferences<4, 4>` | 924 / 168 |
| `Backend` / `LvglState` | 1104 / 1336 |
| `ActiveSurface` / `TimerServiceMetrics` | 52 / 24 |
| `PreparedNavigation` / `NavigationPlan` / `TransitionResult` | 172 / 152 / 136 |
| Callback intent queue / route table, including mutex wrappers | 272 / 212 |
| `lv_mem_monitor_t` / allocator snapshot stack temporaries | 28 / 52 |

The Phase 3 release ELF has pool 4128 bytes, callback storage 272/212/1, `.data` 14036, `.bss` 67876,
`.stack` 114156, and `.dram2_uninit` 104392. Against E-0003, named UI storage grew 376 bytes, linked
`.data` plus `.bss` grew 360, and stack remainder fell 352; alignment absorbed the difference.

The E-0006 lane added a 40-byte result channel; its release artifact has `.data` 14156, `.bss` 67916,
and `.stack` 113988. Release/debug cycles recorded CPU0 minima 101064/101304 and stable LVGL blocks.

The Phase 4 pure model adds 48 bytes to the shell/backend/state/display pool (now 4176). Release is
`.data` 14092, `.bss` 67948, `.stack` 114020; callback/ACK statics and capacities are unchanged.

## What Was Recovered

| Section | Before | After | Delta |
| --- | ---: | ---: | ---: |
| `.data` | 23652 | 13700 | -9952 |
| `.bss` | 135924 | 58724 | -77200 |
| **`.stack`** | **36492** | **123644** | **+87152** |
| `.dram2_uninit` | 90000 | 104392 | +14392 |

CPU0 stack headroom grew by 239%.

### Jump tables out of DRAM (-9912)

`ESP_HAL_CONFIG_PLACE_SWITCH_TABLES_IN_RAM=false` in `.cargo/config.toml` moves
switch tables to flash. The knob is a blanket one, covering three section
groups, and two of them are dereferenced by IRAM-resident code. Those are kept
in DRAM by `ld/rwdata_hook.x`, enabled via
`ESP_HAL_CONFIG_USE_RWDATA_LD_HOOK=true`:

- `.rodata.*_esp_hal_internal_handler*`: esp-hal interrupt dispatch tables
- `.rodata.cst*`: LLVM constant pools. `esp_radio::common_adapter::semphr_give`
  and `semphr_take` are `#[ram]` and load one of these, so a Wi-Fi semaphore
  operation during a cache-disabled flash write would fault if it lived in flash

Dropping the knob without the hook recovers 13168 instead of 9912, but adds
exactly one flash reference from `.rwtext` - the `semphr_*` constant above. The
extra 3256 bytes are not worth that failure mode.

### Internal heap moved out of `dram_seg` entirely (-59392)

`INTERNAL_HEAP_DRAM2_BYTES` in `src/firmware/psram/core.rs` is the whole
internal-capability heap, 58 KB, living in `dram2_seg` via `.dram2_uninit`.
Nothing is left in `dram_seg`, so all of it is stack now.

Two things made that possible: the `dram2_seg` extension below, and moving
`FRAMEBUFFER_BW_PREVIOUS` to PSRAM, which freed 45000 bytes of `dram2_seg`.

Capacity went 64 KB to 58 KB, leaving about 9.6 KB of margin over the
48520-byte measured peak. Constraints:

- esp-alloc supports exactly three heap regions; this one plus PSRAM uses two.
  Do not spend the third on the PRO CPU ROM stack — that was measured at an
  11/40 boot panic rate, see
  [Reclaiming the ROM Stack](./dram-budget-rom-stack.md#measured-failure).
- `.dram2_uninit` is 104392 of 113840 bytes. Growing
  `INTERNAL_HEAP_DRAM2_BYTES` overflows `dram2_seg` at link time.

Watch `min_internal_free_bytes` in the serial telemetry. If it drops below about
4 KB on real workloads, the heap needs to grow into the 9448 bytes still spare
in `dram2_seg`.

### Previous framebuffer to PSRAM (frees 45000 in `dram2_seg`)

`FRAMEBUFFER_BW_PREVIOUS` is only diffed against when building the partial
transition, and `scan_partial_framebuffer_pass` reads the transition buffer
rather than it, so unlike `FRAMEBUFFER_BW` it is never touched inside the
interrupt-masked scan. `bootstrap` allocates it from PSRAM and installs it via
`install_previous_framebuffer`, mirroring the transition buffer. Absent either
buffer, partial refreshes fall back to full ones.

### Task-local state onto the heap (-11664)

Embassy task pools live in `.bss` in `dram_seg`, so large task-local state costs
stack. Two moves, both to the heap in `dram2_seg`:

- `FatEngine` boxed in `sd_task`: pool 10160 to 5416.
- The serial command dispatcher boxed with `Box::pin` at its call site in
  `serial_task`: pool 9648 to 2728. Its arms do not share stack slots, so
  inlining the future inflated the pool.

The serial one trades robustness for space: dispatch now allocates about 7 KB
per command and can fail under heap pressure, on the path used to debug the
device. Revert it if that matters more than the bytes.

### Reclaiming the ROM stack (+15072 to `dram2_seg`)

`ld/meditamer-memory.x` extends `dram2_seg` down over the APP CPU ROM stack
and the unmapped hole below it. The evidence, the reset-path analysis, the
linker-search-order workaround and the deep-sleep interaction are in
[DRAM Budget: Reclaiming the ROM Stack](./dram-budget-rom-stack.md).

## Guards

- `scripts/ci/check_iram_flash_refs.sh` counts literal-pool words in `.rwtext`
  that point into the flash-mapped rodata window and fails if the count exceeds
  the baseline of 78. A rise means an IRAM function gained a flash-resident
  constant or jump table, which faults with the cache disabled.
- `scripts/ci/check_pinned_linker_scripts.sh` diffs `ld/meditamer-memory.x`
  against the esp-hal version in `Cargo.lock` and fails if it differs by
  anything other than the `dram2_seg` line. A pinned linker script would
  otherwise silently revert upstream fixes on an esp-hal upgrade.
- `scripts/ci/check_panel_waveform_placement.sh` already pins the panel scan
  passes to `.rwtext` and the waveform LUTs to `.data`.

The baseline is not zero: esp-hal and the Wi-Fi blob ship `#[ram]` functions
that reference flash independently of this change.

## Re-measuring

Section totals and the stack remainder:

```bash
xtensa-esp32-elf-size -A target/xtensa-esp32-none-elf/release/meditamer
```

Per-symbol DRAM attribution, largest first:

```bash
xtensa-esp32-elf-nm -C -S --size-sort -r target/xtensa-esp32-none-elf/release/meditamer
```

Filter that output to addresses in `3ffb....`-`3ffd....` for `dram_seg`. Task
pool sizes are the `::POOL` symbols; each is the size of one task's future.

## Remaining Levers

Not yet applied, roughly in order of payoff:

- ~~**PRO CPU ROM stack, 15072 bytes.**~~ Tried and reverted: backing heap with
  it panicked 11 of 40 boots in a Wi-Fi ISR. See
  [Reclaiming the ROM Stack](./dram-budget-rom-stack.md#measured-failure).
- **`serial_task` pool, ~5 KB.** The whole serial command dispatcher is
  inlined into one future. Splitting the heavy branches off the await path is
  the single biggest task-pool win.
- **Diagnostic statics, ~2 KB.** `SD_SERIAL_LINES` (`heapless::String<256>` x 8
  = 2112 bytes) does not use the const-capacity-1 pattern that
  `TOUCH_TRACE_SAMPLES` already uses.
- **Channel depths, ~2 KB.** `TOUCH_PIPELINE_EVENTS` at depth 64 costs 2600
  bytes; `SD_REQUESTS` at depth 8 with a ~265-byte payload costs 2144.
Not recoverable: the Wi-Fi/PHY blob statics, the grayscale waveform LUT
(deliberately in `.data` for the interrupt-masked scan loop), and the panel
framebuffers (already in `dram2_seg`, and they must stay internal).
