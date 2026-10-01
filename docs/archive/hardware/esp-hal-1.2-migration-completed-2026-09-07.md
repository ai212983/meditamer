# esp-hal 1.2.0 Migration Closeout

- Status: Done — hardware migration qualified; closed by user decision with delivery limitations recorded
- Last-reviewed: 2026-09-07
- Related: [hardware test matrix 2G](../../reference/hardware-test-matrix.md) — the recorded
  sleep behaviour this migration has to reproduce; [vendor/README.md](../../../vendor/README.md) —
  what is patched and why; [DRAM budget](../../reference/dram/dram-budget.md)

## Outcome

Both targets run released esp-hal 1.2.0 with the vendored esp-hal fork retired, and the ESP32-S3
sleep behaviour that fork existed to protect is demonstrated on hardware rather than assumed.

Hardware testing has now exercised timer wake, light sleep, BLE, both panic paths
and Inkplate upload. It exposed startup scheduling and host capture problems;
fixes and exact artifact results are recorded in the hardware matrix. Deep-sleep recovery now passes with timer-only wake and fresh providers;
see the 2026-09-07 HAL requalification under hardware matrix §2G.

## Closeout decision (2026-09-07)

The user closed this migration after substantial subsequent work became interleaved
with it. A separate migration commit and fresh-checkout validation of that isolated
commit are no longer required for this plan's closure. The recorded hardware
qualification remains the evidence for the migration outcome.

This is an administrative closeout, not a claim that the desk delivery checklist
below passed. At closeout the migration plan and replacement vendor trees were
untracked, final lint/guard validation of an isolated committed tree had not been
performed, and the listed memory deltas had not been independently remeasured.
These repository delivery concerns remain relevant when packaging the accumulated
work; archiving this plan does not establish fresh-clone reproducibility.

The original delivery requirements below are retained as historical context and
are superseded by this closeout decision for the purpose of this plan's status.
Downstream storage, networking and broader device qualification retain their own
owners and are not marked complete by this decision.

## What changed

Adopted because two things arrived together. esp-hal 1.2.0 (released 2026-09-03) contains all three
ESP32-S3 sleep fixes `vendor/esp-hal-1.1.1-s3-sleep-fixes` existed to backport — commits `9487850`,
`434755e`, `90b2b3b` are all ancestors of the `esp-hal-v1.2.0` tag — and it ships an SDMMC/SDIO host
driver with ESP32-S3 support, which unblocks
[shared storage and Waveshare SD](../../plans/shared-storage-and-waveshare-sd.md). That plan is downstream of
this one and should not start until the bump is qualified, or an SD bring-up failure has two
candidate causes.

Versions moved: esp-hal 1.1.1→1.2.0, esp-backtrace 0.19.0→0.20.0, esp-rtos 0.3.0→0.4.0,
xtensa-lx-rt 0.22.0→0.23.0, esp-alloc 0.10.0→0.11.0, esp-radio-rtos-driver 0.3.0→0.4.1,
esp-println 0.17.0→0.18.0, esp-storage 0.9.0→0.10.0, esp-bootloader-esp-idf 0.5.0→0.6.0,
esp-sync 0.2.1→0.3.0, the esp32/esp32s3 PACs, and esp-rom-sys.

Vendor forks: three deleted (`esp-hal`, `esp-rtos`, `xtensa-lx-rt` — upstream releases now carry
their backports), three rebased onto new bases (`esp-backtrace`, renamed to
`esp-backtrace-0.20.0-custom-pre-backtrace-info` now that only the project-owned
`custom_pre_backtrace` hook remains; `esp-alloc`; `esp-radio-rtos-driver`), and two carrying
manifest-only dependency-pin widening with no source change (`esp-radio`, plus a new `esp-phy` fork
one level down). See [vendor/README.md](../../../vendor/README.md).

Real source migrations, in rough order of risk: the sleep API redesign
(`targets/medinote-waveshare/src/sleep.rs` rewritten end to end), `SoftwareInterruptControl` removal,
`RTC_TIMER` splitting off `LPWR`, DMA buffers needing `DmaAlignedMut`, `SpiDmaBus` merging into
`SpiDma`, esp-storage and esp-bootloader-esp-idf feature and API changes, `Adc::cancel_oneshot`
removal, and esp-hal moving its `RESERVE_DRAM` computation into a preprocessor the pinned linker
script bypasses.

## Closed

Recorded so this plan stands alone as the closeout evidence.

| Item | Resolution |
| --- | --- |
| IRAM→flash literal baseline raised 78→87 without attribution | All 87 attributed to owning IRAM functions by matching literal addresses against `.rwtext` `l32r` instructions. No first-party symbol; 40 belong to esp-hal 1.2.0's new `gpio::wakeup` hooks and `LowPower::sleep`. Table recorded in [check_iram_flash_refs.sh](../../../scripts/ci/check_iram_flash_refs.sh). |
| `RESERVE_DRAM` recomputed in build.rs, unguarded | New [check_bt_dram_reservation.sh](../../../scripts/ci/check_bt_dram_reservation.sh) asserts the invariant on the ELF (`.data` VMA is `dram_seg`'s origin; BT is linked iff `btdm_controller_init` resolves), so it cannot drift with the feature graph. Self-tested by [test_check_bt_dram_reservation.sh](../../../scripts/tests/host/test_check_bt_dram_reservation.sh). |
| Battery ADC `cancel` became an always-`Ok` no-op, making `ProviderLoop::suspend` report `Quiesced` unconditionally | `cancel` now drains the abandoned conversion with one non-blocking poll (what clears esp-hal's private `active_channel` and runs the ADC's own `reset`), tracks `in_flight` so it cannot *start* a conversion on an idle converter, and returns `Err(AdcTimeout)` when the drain fails. Two tests added. |
| `check_stack_risk.sh --medinote-elf` failing on a renamed symbol | Two symbol paths lost their `medinote::apps::` prefix in the `platform/hourglass` extraction; the guard and its self-test fixture both updated. All three gate modes pass. |
| Plan and board docs asserting no S3 SD transport exists | [shared-storage-and-waveshare-sd.md](../../plans/shared-storage-and-waveshare-sd.md) and [board.md](../../reference/hardware/waveshare-rlcd42/board.md) corrected. |

## Delivery

### 1. Desk-sized closeout

These are delivery requirements. They do not block isolated hardware diagnosis
on an identified working-tree ELF; do them before claiming migration closeout.

1. **Track the new esp-phy vendor tree.** `vendor/esp-phy-0.2.0-hal-pin-widen/` is untracked, so a
   `git clean -fd` deletes it and a fresh clone never had it. Either way the build fails at
   dependency resolution with an error that does not name the cause. Every other `vendor/` tree is
   tracked, which is exactly why this one is easy to miss.
2. **Commit the bump separately** from the in-flight board-extraction work it currently sits on top
   of. The two are already interleaved across the working tree and are not reviewable together.
3. **Resolve the four `clippy --all-features` lints** in `tools/rendering/refresh-probe/panel/main.rs`
   (`needless_range_loop`, `needless_option_as_deref`, `manual_is_multiple_of`, `too_many_arguments`).
   None are on lines this migration touched; they look like clippy-version drift. The non-clippy
   `--all-features` build is clean.

Exit: the tree builds from a fresh clone, the bump is one reviewable commit, and the full lint suite
is green.

### 2. On-device qualification

The migration's actual risk. Qualify sleep, BLE and panic on ESP32-S3; run the existing
Wi-Fi-to-SD upload regression on Inkplate. Record S3 results under the existing §2G in the
[hardware test matrix](../../reference/hardware-test-matrix.md) and Inkplate upload evidence
under its applicable SD/network coverage. Medinote's HTTP-to-SD service is downstream of
the SD plan and must not be required to unblock that plan's transport probe.

1. **Deep sleep, timer wake.** Confirm the chip enters deep sleep without resetting on entry and
   wakes on the 15-second `LowPower::set_wakeup_deadline` deadline. §2G's recorded baseline is USB
   enumeration disappearing for 15.18–15.21 s and returning normally. Record USB
   absence as host-observed timing, not an exact RTC tolerance: enumeration and
   host polling add variable latency. Require the configured 15-second deadline,
   a single `CoreDeepSleep` reset with timer-only wake, and complete Home/provider
   recovery; an immediate reset, wrong wake source or failed recovery blocks SD.
   This corrects the earlier plan's unsupported requirement to reproduce a
   30 ms-wide USB enumeration window exactly.
2. **Light sleep, GPIO18 `KEY` wake.** The pull changed kind, not just spelling: `rtcio_pullup(true)`
   (RTC domain) became `InputConfig::with_pull(Pull::Up)` (digital) plus
   `apply_wakeup_config(low_power_path)`. Whether that pull survives the digital domain powering down
   is what separates "wakes instantly", "never wakes", and correct. **Test with the button idle for
   the full 15 s**, not only pressed — an immediate wake on an untouched board is the symptom, and a
   pressed-button test cannot see it. Also re-confirm §2G's behaviour that input is re-armed only
   after the physical key is released.
3. **S3 BLE start/stop and one Inkplate Wi-Fi upload cycle.** Use Inkplate's existing
   complete product service for upload/readback under the
   [Wi-Fi regression gate](../../guides/wifi-regression-gate.md). This does not qualify S3 Wi-Fi;
   its probe and full-service acceptance remain owned by the
   [Wi-Fi plan](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md). `esp-radio` and `esp-phy` are source-unchanged but
   now compile against an esp-hal upstream never shipped them against; upstream's own in-progress
   trees for both carry real source changes for 1.2.0 that this migration deliberately did not
   hand-port. That is the conservative call, and its consequence is a version combination nobody has
   run.
4. **One real panic**, via `panic-screen-probe` and the production `crash-screen` build. The
   esp-backtrace fork changed base version and kept only the `custom_pre_backtrace` hook the Guru
   Meditation screen depends on.

Exit: §2G's recorded sleep observations reproduce on 1.2.0, S3 BLE completes a normal cycle,
Inkplate passes the Wi-Fi upload regression, and a deliberate S3 panic still renders the crash
screen. These hardware gates unblock the isolated SD transport probe on the
identified working-tree artifacts. Desk closeout remains required for delivery,
without depending on Medinote storage or HTTP integration.

Note that item 2.1 and 2.2 also discharge the one caveat
[check_iram_flash_refs.sh](../../../scripts/ci/check_iram_flash_refs.sh) cannot settle statically: the
new `gpio::wakeup` hooks hold flash pointers and run at sleep entry and exit, which is when the flash
cache may be down. Holding a flash pointer is not dereferencing one, and only a sleep test can tell
the difference.

## Measured deltas to re-check

Recorded during the bump but **not independently re-measured**, except the IRAM count. Confirm
against [the DRAM budget](../../reference/dram/dram-budget.md)'s procedure once the tree is committed,
and update that document if they hold.

| Measure | Before | After |
| --- | ---: | ---: |
| IRAM→flash literals (verified) | 78 | 87 |
| BLE-release `.data` | 16808 | 16984 |
| BLE-release `.bss` | 73212 | 73548 |
| Linked CPU0 stack | 39172 | 38660 |
| `.dram2_uninit` | 113736/113840 | unchanged |

The recorded stack decrease is 512 bytes less headroom, not an increase in safety
margin. These unconfirmed figures are retained as historical measurements to
recheck when packaging the accumulated work.

## Validation and completion

Desk-side verification already passing, to re-run after the commit split: both firmware targets
across their profiles, `scripts/host-test.sh test all` and `lint all`, and the deep guards —
`check_ble_controller_patch.sh`, `check_network_owner_source.sh`, `check_pinned_linker_scripts.sh`,
`check_ble_image_budget.sh`, `check_stack_risk.sh` in all three modes, `check_iram_flash_refs.sh`,
`check_bt_dram_reservation.sh`, and `check_script_surface.py`.

Host tests and a clean build do not qualify sleep, radio, or panic behaviour. Done means §2's four
checks are recorded in the hardware matrix with the board, build, and observed timings, the desk
suite is green on the committed tree, and `vendor/README.md` describes exactly the forks that remain.
