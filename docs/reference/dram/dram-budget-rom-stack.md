# DRAM Budget: Reclaiming the ROM Stack

A separate topic from [DRAM Budget](./dram-budget.md): that document is the
day-to-day accounting you check before adding a static, this one is the
linker-level reclaim behind it and is only relevant when you touch
`config/linker/esp32/` or deep sleep. Covers why
`config/linker/esp32/meditamer-memory.x` extends `dram2_seg` 15072 bytes
below esp-hal's default, why deep sleep does not endanger it, and why the
PRO CPU half of the same reclaim was measured unsafe and reverted.

## Result Summary

| Region | Range | Status |
| --- | --- | --- |
| APP CPU ROM stack + hole | `0x3FFE4350`-`0x3FFE7E30` | in use, holds `FRAMEBUFFER_BW` — **now enforced by the linker**, see [Enforcing the order](#enforcing-the-order) |
| PRO CPU ROM stack + hole | `0x3FFE0440`-`0x3FFE3F20` | **reverted, measured unsafe** |

Do not re-add the PRO CPU region without new evidence. See
[Measured Failure](#measured-failure).

The "holds `FRAMEBUFFER_BW`" half of that first row was an *intent*, not a fact, until
2026-08-21: nothing enforced it, and for some builds it was false. See
[Enforcing the order](#enforcing-the-order).

## The Original Reasoning

This is the argument that justified the reclaim. Keep it for context, but read
[Measured Failure](#measured-failure) first: on hardware it proved sufficient
for holding our own framebuffer and **not** sufficient for holding heap.

esp-hal's `memory.x` reserves two 11264-byte ROM stacks and leaves two
3808-byte holes unmapped between `0x3FFE0440` and `0x3FFE7E30`, with the comment
that they "can be reclaimed once both cores are running, but for now we play it
safe". `config/linker/esp32/meditamer-memory.x` reclaims the APP CPU half of
that.

Three pieces of evidence, all of which turned out to be necessary but not sufficient:

1. **ESP-IDF does it on this board.** The `esp_idf_wifi_control` capture logs
   `heap_init: At 3FFE0440 len 00003AE0` and `At 3FFE4350 len 0001BCB0`. Those
   two ranges are exactly the holes plus both ROM stacks. ESP-IDF reserves only
   the ROM *data* blocks (`0x3FFE0000`-`0x3FFE0440` and
   `0x3FFE3F20`-`0x3FFE4350`).
2. **ESP-IDF says why.** `components/heap/port/esp32/memory_layout.c`: "after
   the scheduler has started, the ROM stack is not used anymore by anything."
   It defers allocation from the region until then rather than reserving it.
3. **Neither CPU writes there under esp-hal.** Disassembly of both reset paths
   shows the same pattern: `entry a1, N` (which only decrements `a1`, it writes
   no memory in a fresh register window), interrupts masked, then `a1` is
   overwritten with the real stack top before any call. PRO CPU switches to
   `_stack_start` in `Reset`; APP CPU switches to `APP_CORE_STACK_TOP` in
   `start_core1_init`, which the DPORT boot address vectors to directly without
   a ROM stub.

Constraints:

- **NOLOAD only.** The second-stage bootloader runs on the PRO ROM stack while
  it copies our image into RAM, so a loaded section here would be written and
  then immediately clobbered. `.dram2_uninit` is `NOLOAD`, so nothing is placed
  there at flash time.
- **ROM data stays reserved.** `dram2_seg` starts at `0x3FFE4350`, exactly where
  ESP-IDF's `rom_app_data` reservation ends. Do not extend below that.
- **Nothing here survives deep sleep.** See the next section; this is
  automatic on ESP32, not a rule to remember.

esp-hal puts its own `OUT_DIR` ahead of ours on the linker search path, so a
file named `memory.x` in the project linker directory is silently ignored. The
override therefore goes through `config/linker/esp32/meditamer-linkall.x` (a
copy of esp-hal's `linkall.x` that includes `meditamer-memory.x`), selected by
`-Tmeditamer-linkall.x`. `alias.x`, `esp32.x`
and `hal-defaults.x` still resolve from esp-hal.

## Measured Failure

The three arguments above were also used to justify reclaiming the **PRO CPU**
ROM stack (`0x3FFE0440`-`0x3FFE3F20`, 15072 bytes) as a second internal heap
region, `dram3_seg`. On hardware that turned out to be wrong, and the follow-up
experiments cast doubt on the reasoning itself rather than just that one region.

What the earlier reasoning missed: ESP-IDF's own comment says it defers
allocation from this region "until the scheduler has started". Reading its
region list was not sufficient — the list does not capture *when* ESP-IDF
considers the region usable.

The panic is a null store (`EXCVADDR: 0`) reached from a Wi-Fi ISR:

```
pp_post -> esp_radio::common_adapter::queue_send_from_isr
        -> esp_rtos_queue_try_send_to_back_from_isr
        -> CompatQueue::try_send_to_back_from_isr
        -> CompatSemaphore::try_take_from_isr
```

`scripts/device/soak_boot.sh`, 8 second boot windows, same board, 40 boots each.
p values are Fisher's exact against config A.

| # | Internal heap layout | Panics | p vs A |
| --- | --- | ---: | ---: |
| A | one region, above the reclaimed window (**shipping**) | 0/40, then 0/100 | — |
| D | two regions, stock `dram2_seg`, no reclaim anywhere | 4/40 | 0.058 |
| B | two regions, second one in the PRO ROM stack | 11/40 | 0.0002 |
| C | two regions, heap inside the APP ROM stack window | 13/40 | 0.0000 |

Two conclusions, of different strength:

- **Heap in reclaimed ROM stack memory is bad** (B and C, both p < 0.001), and it
  is bad in *either* half. C is the one that rules out "only the PRO half is
  special": it has no `dram3_seg` at all, only heap sitting in the APP window.
- **A second esp-alloc region may be bad on its own** (D, 4/40 with no reclaimed
  memory involved at all), but at p = 0.058 that is not established. It is enough
  to justify not adding regions casually.

Config A was re-run at 100 boots: still 0. Against config D's 10% that outcome
has probability 0.9^100, so A is genuinely different rather than lucky; by the
rule of three its true boot-panic rate is under 3% at 95% confidence.

Methodology note: 20 boots is not enough. The same build gave 7/20 then 4/20.
Use 40 minimum, and re-soak after any change to heap region count or placement.

### The likely root cause is not memory layout

The backtrace is a Wi-Fi ISR posting to a queue *after* the log shows
`upload mode off; wifi paused` — that is, after teardown. That is the shape of a
use-after-free, not of memory corruption from a bad region.

If so, layout is not the bug; it only changes how often the freed queue/semaphore
still happens to look valid, which is exactly the graded 0/4/11/13 pattern above.
That would make config A the lucky layout rather than the correct one, and would
mean there is a real Wi-Fi teardown race worth fixing independently of any of
this. Anyone picking that up should start from the `pp_post` frame rather than
from the allocator.

### Not established

- Whether the reclaimed window is safe for data the Wi-Fi driver never sees. Only
  heap was ever placed there, and only heap failed. The shipping config puts
  `FRAMEBUFFER_BW` there and has been clean, but that is weaker evidence than it
  looks given the use-after-free hypothesis.

  **Corrected 2026-08-21:** "the shipping config puts `FRAMEBUFFER_BW` there" was
  not reliably true — the placement was unpinned, and builds in which the *heap*
  took that slot are what produced the ADR-0014 `EXCVADDR=0x4000c0d4` crash. It is
  true and enforced now; see [Enforcing the order](#enforcing-the-order). The
  narrower claim this bullet makes — that the window is safe for passive data of
  ours — is now supported by 40/40 clean boots with the framebuffer there.

## Deep sleep is compatible with the reclaim

Adding deep sleep does not put the reclaimed ROM stack at risk, because the ROM
only re-enters it after a full reset, by which point nothing of ours is live
there:

- ESP32 has no retention domain for internal SRAM. `esp_sleep.h` exposes
  `ESP_PD_DOMAIN_*` for RTC periph, RTC slow/fast mem, XTAL, CPU, RTC8M and
  VDDSDIO only, and esp-hal's `RtcSleepConfig::deep()` likewise has power-down
  bits for RTC memories but none for SRAM. All of `dram_seg` and `dram2_seg` is
  lost on every deep sleep regardless of where it sits.
- Wake from deep sleep is a chip reset: ROM, then bootloader, then our image.
  That is the cold-boot path analysed above.
- Light sleep never re-enters ROM at all. esp-hal's `sleep()` is
  `start_sleep(); finish_sleep();` on the caller's own stack, with no reset.
- An RTC wake stub, if one is ever added, runs from RTC fast memory on the *PRO*
  ROM stack, which this change does not touch.

Two consequences worth planning for:

- **Cross-wake state needs flash or RTC memory, and RTC needs a custom sleep
  config.** `AppStateStore` (flash) already covers slow-changing state. RTC fast
  and slow are unused (0 of 8192 bytes each), but esp-hal's
  `RtcSleepConfig::deep()` sets `rtc_slowmem_pd_en` *and* `rtc_fastmem_pd_en`,
  and `apply()` writes `slowmem_pd_en` — so stock `sleep_deep()` powers RTC slow
  down. `fastmem_pd_en` is commented out upstream with a TODO. Retention
  requires building a config with `set_rtc_slowmem_pd_en(false)` and confirming
  it on hardware.
- **The first refresh after a wake must be a full refresh.** Both framebuffers
  live in `.dram2_uninit` and are zeroed in `InkplateHal::new`, while the panel
  physically still shows the pre-sleep image. Partial-refresh diffing against a
  zeroed `FRAMEBUFFER_BW_PREVIOUS` would produce artifacts. This is unrelated to
  the ROM stack change, but it goes live the moment deep sleep ships.

## Enforcing the order

Added 2026-08-21, after the "config C" layout this document already measured as unsafe turned
out to be what was actually shipping.

`.dram2_uninit` holds two statics: the internal heap (`src/firmware/psram/init.rs`, 68,736 bytes)
and `FRAMEBUFFER_BW` (`src/platform/inkplate/hardware.rs`, 45,000 bytes). esp-hal's `dram2.x`
emits a bare `*(.dram2_uninit)`, so **which of them landed at the bottom of `dram2_seg` — that
is, on the APP CPU ROM stack — was decided by incidental link order.** Nothing expressed the
intent recorded in [Result Summary](#result-summary), so nothing preserved it. In the failing
builds the heap was at the bottom (`0x3ffe4350`-`0x3fff4fd0`), covering
`reserved_rom_stack_app` (`0x3ffe5230`-`0x3ffe7e30`) — exactly config C, measured here at 13/40
boot panics.

That is also the mechanism behind the "binary layout perturbs interrupt timing" conclusion in
ADR-0014's 2026-08-19 addendum. Layout was not nudging a timing window; it was moving the heap
into and out of the ROM stack. Symptom, in full:

- Core 1 walks its ROM stack coming out of reset and overwrites whatever lives there.
- An `esp_rtos` `TaskListItem.next` (offset `0x10c`) took the value `0x4000bfd4` — exactly
  `_xtos_p_none`, a ROM symbol, not plausible heap garbage.
- `RunQueue::mark_task_ready` then ran `addmi a14, a11, 0x100` / `s8i a9, a14, 0`. `s8i` is a
  *byte* store, and a narrow store into instruction memory is what raises `LoadStoreError`
  (`EXCCAUSE=3`), with `EXCVADDR = 0x4000bfd4 + 0x100 = 0x4000c0d4` — the fixed address seen in
  every occurrence of this crash since it was first reported.

The fix gives the two statics distinct input sections and pins their order in a `SECTIONS` block
at the end of `config/linker/esp32/meditamer-memory.x`, which is included before esp-hal's
`esp32.x`/`dram2.x`:

```
.dram2_uninit (NOLOAD) : ALIGN(4) {
  *(.dram2_uninit.framebuffer)
  *(.dram2_uninit.heap)
} > dram2_seg
```

`FRAMEBUFFER_BW` is 45,000 bytes, comfortably larger than the `0x3AE0` offset from
`0x3FFE4350` to the top of the ROM stack window, so it always covers the window and the heap
always begins above `0x3FFE7E30`. No memory is given up; the 15,072-byte reclaim is retained.

Hardware A/B, same source and board, only the two `link_section` strings differing:

| Layout | Boot panics | `RUNTIME_READY` |
| --- | ---: | ---: |
| heap above the window (fixed) | 0/40 | 40/40 |
| heap over the window (control) | 12/12 | 0/12 |

Every control panic was `EXCVADDR=0x4000c0d4`. Touch also recovered: pre-fix boots logged
`touch: init_failed ... Err(I2c(I2c(ArbitrationLost)))`, post-fix boots reach `touch: ready`.
The `ArbitrationLost` was a second symptom of the same corruption — the touch driver runs on
core 1 — not the coincidence ADR-0014's addendum took it for.

**If you add a third static to `.dram2_uninit`, give it its own `.dram2_uninit.*` input section
and place it explicitly in that block.** A bare `.dram2_uninit` will land after the pinned two
via esp-hal's trailing wildcard, which is safe today only because the framebuffer already covers
the window — do not rely on that. Verify placement after any change to statics in this section:

```bash
xtensa-esp32-elf-nm -n -S target/xtensa-esp32-none-elf/release/meditamer \
  | awk '$1 >= "3ffe4350"'
```

The heap symbol must start at or above `0x3FFE7E30`.
