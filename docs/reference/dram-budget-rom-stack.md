# DRAM Budget: Reclaiming the ROM Stack

A separate topic from [DRAM Budget](./dram-budget.md): that document is the
day-to-day accounting you check before adding a static, this one is the
linker-level reclaim behind it and is only relevant when you touch `ld/` or
deep sleep. Covers why `ld/meditamer-memory.x` extends `dram2_seg` 15072 bytes
below esp-hal's default, why deep sleep does not endanger it, and why the
PRO CPU half of the same reclaim was measured unsafe and reverted.

## Result Summary

| Region | Range | Status |
| --- | --- | --- |
| APP CPU ROM stack + hole | `0x3FFE4350`-`0x3FFE7E30` | in use, holds `FRAMEBUFFER_BW` |
| PRO CPU ROM stack + hole | `0x3FFE0440`-`0x3FFE3F20` | **reverted, measured unsafe** |

Do not re-add the PRO CPU region without new evidence. See
[Measured Failure](#measured-failure).

## The Original Reasoning

This is the argument that justified the reclaim. Keep it for context, but read
[Measured Failure](#measured-failure) first: on hardware it proved sufficient
for holding our own framebuffer and **not** sufficient for holding heap.

esp-hal's `memory.x` reserves two 11264-byte ROM stacks and leaves two
3808-byte holes unmapped between `0x3FFE0440` and `0x3FFE7E30`, with the comment
that they "can be reclaimed once both cores are running, but for now we play it
safe". `ld/meditamer-memory.x` reclaims the APP CPU half of that.

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
file named `memory.x` in `ld/` is silently ignored. The override therefore goes
through `ld/meditamer-linkall.x` (a copy of esp-hal's `linkall.x` that includes
`meditamer-memory.x`), selected by `-Tmeditamer-linkall.x`. `alias.x`, `esp32.x`
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
