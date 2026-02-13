# ESP-HAL Migration Plan (Inkplate 4 TEMPERA)

This checklist is for migrating this project from the current ESP-IDF Rust stack to the `esp-hal` stack.

## Current Baseline

- Target/tooling is `esp-hal`: `xtensa-esp32-none-elf`.
- Runtime now uses:
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/inkplate_hal.rs`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs`
- Build configuration coupling is in:
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/.cargo/config.toml`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/scripts/build.sh`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/scripts/flash.sh`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/wokwi.toml`

## Phase 0: Branching and Safety

- [ ] Create migration branch: `codex/esp-hal-migration`.
- [ ] Capture a "known-good" baseline on real hardware:
  - [ ] Boot + init
  - [ ] Full e-paper refresh
  - [ ] Frontlight brightness write
  - [ ] Buzzer chirp
- [ ] Save baseline logs for comparison.
- [ ] Add a rollback note in PR description: how to switch back to ESP-IDF build if migration stalls.

## Phase 1: New Build Skeleton (No Feature Port Yet)

- [x] Replace target from `xtensa-esp32-espidf` to `xtensa-esp32-none-elf` in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/.cargo/config.toml`.
- [ ] Remove ESP-IDF-specific build plumbing:
  - [x] Remove `esp-idf-sys` dependency from `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`.
  - [x] Remove `embuild` build-dependency from `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`.
  - [x] Remove `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/build.rs` usage by moving linker args into `.cargo/config.toml`.
- [ ] Add minimal `esp-hal` runtime stack dependencies:
  - [x] `esp-hal`
  - [x] `esp-backtrace`
  - [x] `esp-println`
  - [ ] allocator crate if needed (`esp-alloc`)
- [x] Make firmware boot with a minimal "alive" loop and serial logs.
- [x] Update flash command scripts for the new target output path.

Exit criteria:
- [x] `cargo build` succeeds for `xtensa-esp32-none-elf`.
- [x] Binary flashes and prints heartbeat logs.

### Serial Debug Baseline (Working)

- Current debug output path: explicit `UART0` setup in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs` (`GPIO1` TX, `115200` baud).
- Flash flow:
  - `ESPFLASH_PORT=/dev/cu.usbserial-540 ./scripts/flash.sh debug`
- Monitor flow:
  - `ESPFLASH_PORT=/dev/cu.usbserial-540 ./scripts/monitor.sh`
- Known-good monitor mode uses:
  - `--before default-reset`
  - `--after hard-reset`

## Phase 2: Platform Shim Layer (Prepare for Incremental Port)

- [x] Create a small hardware shim module (for example `src/platform.rs`) to isolate:
  - [x] Delay API (`delay_us`, `delay_ms`)
  - [x] I2C API (`write`, `write_read`, `probe`, `reset`) (with `HalI2c` implementation)
  - [ ] GPIO mode/set operations
  - [ ] Optional logging wrappers
- [ ] Refactor `Inkplate` internals to call shim traits/functions instead of direct `sys::*`.
  - [x] Added `src/inkplate_hal.rs` with migrated IO-expander + PMIC core init/frontlight path.
  - [x] Ported panel scan GPIO fast-path and display pipeline.
- [ ] Keep behavior identical before changing internals.

Exit criteria:
- [x] No direct `esp_idf_sys` usage remains in application logic files.
- [ ] All low-level calls are routed through one abstraction point.

## Phase 3: I2C Port and Recovery Behavior

- [x] Port `I2cMasterBus` behavior to `esp-hal` I2C trait implementation and recovery hooks.
- [ ] Recreate functional parity for:
  - [x] Basic write/write-read
  - [x] Probe semantics used by buzzer/frontlight paths
  - [x] Bus recovery strategy on NACK/error (retry + software timeout; hardware reset hook still pending)
  - [ ] Address-specific tuning currently used for `0x2F` buzzer digipot
- [ ] Validate these devices first:
  - [x] Internal IO expander (`0x20`)
  - [x] PMIC (`0x48`)
  - [x] Frontlight digipot (`0x2E`)
  - [x] Buzzer digipot (`0x2F`) (basic beep self-test)

Exit criteria:
- [x] Frontlight brightness write works repeatedly.
- [x] Buzzer frequency write works with retries and no lockups.
- [ ] No hard I2C dead-state after repeated NACK injection tests.

Current observations:
- Runtime loop with repeated frontlight brightness writes is stable in current test runs.
- Startup brightness write is stable in latest repeated boot checks after wake/settle/retry hardening.

## Phase 4: GPIO Fast Path for E-Paper Waveforms

- [x] Port direct register writes currently used for timing-critical panel scan:
  - [x] `gpio_out_set/clear`
  - [x] `gpio_out1_set/clear`
  - [x] Pin mode transitions for panel on/off and Z-state
- [x] Keep timing-sensitive sequence order unchanged in:
  - [x] `vscan_start`
  - [x] `vscan_end`
  - [x] `hscan_start`
  - [x] `clean`
  - [x] `display_bw`
- [ ] Verify no accidental optimization removes pulse timing behavior.

Exit criteria:
- [x] Full refresh succeeds with expected image orientation/contrast.
- [ ] No obvious regressions in ghosting compared to baseline.
- [x] Panel power-up/power-down sequence is stable across repeated runs.

Current groundwork:
- Added `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/gpio_fast.rs` with ESP32 PAC-backed `out_w1ts/out_w1tc/out1_w1ts/out1_w1tc` helpers and board masks.
- Added migrated waveform primitive methods in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/inkplate_hal.rs`:
  - `prepare_panel_fast_io`
  - `panel_fast_waveform_smoke`
  - `panel_waveform_primitives_smoke`
  - `panel_clean_smoke`
- Runtime bring-up now verifies these on hardware and logs:
  - `panel fast-gpio smoke: ok`
  - `panel waveform primitives: ok`
  - `panel clean smoke: ok`
  - `display test pattern: ok` (full `display_bw(false)` path)

## Phase 5: Application Runtime Port

- [x] Replace `std::thread::sleep` runtime loop with blocking HAL delay in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs`.
  - [x] blocking HAL delay (initial port), or
  - [ ] Embassy timer (`esp-rtos`) if async path is chosen.
- [ ] Replace libc wall-clock path with explicit strategy:
  - [x] temporary monotonic uptime labels, or
  - [ ] SNTP/RTC integration phase.
- [x] Confirm `embedded-graphics` + `u8g2-fonts` path still compiles and renders.

Exit criteria:
- [x] Clock screen renders on target.
- [x] Refresh interval loop remains stable for medium-duration soak (`190s` check); multi-hour soak deferred by user request.

## Phase 6: Test Matrix and Stabilization

- [x] Add a hardware test checklist script/doc:
  - Added `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/scripts/soak_boot.sh` for repeated reset-cycle validation against required boot/render log markers.
  - Added `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/scripts/soak_refresh.sh` for long-duration refresh monitoring and failure scanning.
  - Added `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/docs/development/hardware-test-matrix.md` for manual/automated Phase 6 validation runs and reporting format.
  - [ ] Cold boot 20 times
  - [ ] 200 display refreshes (deferred with multi-hour soak)
  - [x] 100 frontlight writes
  - [x] 100 buzzer notes
  - [x] I2C bus fault recovery scenarios (address-NACK injection smoke + post-fault bus readback)
- [ ] Capture and compare metrics versus baseline:
  - [ ] Boot latency
  - [ ] Refresh time
  - [ ] Failure count
  - [ ] Power profile (if measurement setup available)
- [ ] Fix regressions before enabling extra sensors/features.

Exit criteria:
- [x] Zero critical failures in reset/medium soak runs.
- [ ] Known issues list is explicit and acceptable.

Current observations:
- On February 12, 2026, reset-cycle soak runs with `scripts/soak_boot.sh` passed `10/10` cycles (plus a separate `5/5` run) after frontlight wake/prep retry hardening in `set_brightness_checked`.
- On February 12, 2026, an extended reset-cycle soak passed `20/20` cycles (`SOAK_WINDOW_SEC=8`) with zero missing required startup/render markers.
- On February 12, 2026, `scripts/soak_refresh.sh 190` passed with `refresh_count=3` and no panic/reboot signatures.
- On February 12, 2026, an additional `scripts/soak_boot.sh 100` run produced `99/100` pass in strict mode due one timing miss on first uptime refresh marker, while all `100/100` cycles had `frontlight brightness write: ok`, `buzzer test: ok`, `display test pattern: ok`, and `i2c fault-recovery smoke: ok`.
- Existing soak currently validates reset cycles, not physical power disconnect/reconnect cold boots.

## Phase 7: Cutover

- [x] Remove dead ESP-IDF-only files/config.
- [x] Update docs for new toolchain and flash steps.
- [x] Update CI to build the new target.
- [ ] Merge only when exit criteria from all phases are met.

## Recommended Milestones and Time Box

- [ ] Milestone A (2-3 days): boot + I2C + expander + PMIC basic control.
- [ ] Milestone B (3-5 days): stable e-paper full refresh pipeline.
- [ ] Milestone C (2-3 days): runtime loop + time strategy + soak tests.
- [ ] Milestone D (2-4 days): hardening and CI/docs cleanup.

## References

- Rust on ESP book (overview): <https://docs.espressif.com/projects/rust/book/overview/index.html>
- Rust on ESP (`std` and ESP-IDF): <https://docs.espressif.com/projects/rust/book/overview/std.html>
- Rust on ESP (`no_std` and `esp-hal`): <https://docs.espressif.com/projects/rust/book/overview/no-std.html>
- Async options (`esp-rtos`, Embassy): <https://docs.espressif.com/projects/rust/book/application-development/async.html>
- `esp-hal` crate docs: <https://docs.espressif.com/projects/rust/esp-hal/1.0.0/esp32/esp_hal/index.html>
- `esp-hal` repository: <https://github.com/esp-rs/esp-hal>
- `esp-rtos` docs: <https://docs.espressif.com/projects/rust/esp-rtos/0.2.0/esp32/esp_rtos/index.html>
- Inkplate Arduino reference library: <https://github.com/SolderedElectronics/Inkplate-Arduino-library>
- Existing project sensor notes: `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/docs/sensors.md`
