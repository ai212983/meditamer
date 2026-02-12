# ESP-HAL Migration Plan (Inkplate 4 TEMPERA)

This checklist is for migrating this project from the current ESP-IDF Rust stack to the `esp-hal` stack.

## Current Baseline

- Target/tooling is ESP-IDF: `xtensa-esp32-espidf`.
- Runtime and driver coupling to `esp_idf_sys` is concentrated in:
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/lib.rs`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs`
- Build configuration coupling is in:
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/.cargo/config.toml`
  - `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/build.rs`
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

- [ ] Replace target from `xtensa-esp32-espidf` to `xtensa-esp32-none-elf` in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/.cargo/config.toml`.
- [ ] Remove ESP-IDF-specific build plumbing:
  - [ ] Remove `esp-idf-sys` dependency from `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`.
  - [ ] Remove `embuild` build-dependency from `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/Cargo.toml`.
  - [ ] Remove `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/build.rs` usage.
- [ ] Add minimal `esp-hal` runtime stack dependencies:
  - [ ] `esp-hal`
  - [ ] `esp-backtrace`
  - [ ] `esp-println`
  - [ ] allocator crate if needed (`esp-alloc`)
- [ ] Make firmware boot with a minimal "alive" loop and serial logs.
- [ ] Update flash command scripts for the new target output path.

Exit criteria:
- [ ] `cargo build` succeeds for `xtensa-esp32-none-elf`.
- [ ] Binary flashes and prints heartbeat logs.

## Phase 2: Platform Shim Layer (Prepare for Incremental Port)

- [ ] Create a small hardware shim module (for example `src/platform.rs`) to isolate:
  - [ ] Delay API (`delay_us`, `delay_ms`)
  - [ ] I2C API (`write`, `write_read`, `probe`, `reset`)
  - [ ] GPIO mode/set operations
  - [ ] Optional logging wrappers
- [ ] Refactor `Inkplate` internals to call shim traits/functions instead of direct `sys::*`.
- [ ] Keep behavior identical before changing internals.

Exit criteria:
- [ ] No direct `esp_idf_sys` usage remains in application logic files.
- [ ] All low-level calls are routed through one abstraction point.

## Phase 3: I2C Port and Recovery Behavior

- [ ] Port `I2cMasterBus` (currently in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/lib.rs`) to `esp-hal` I2C.
- [ ] Recreate functional parity for:
  - [ ] Basic write/write-read
  - [ ] Probe semantics used by buzzer/frontlight paths
  - [ ] Bus recovery strategy on NACK/error
  - [ ] Address-specific tuning currently used for `0x2F` buzzer digipot
- [ ] Validate these devices first:
  - [ ] Internal IO expander (`0x20`)
  - [ ] PMIC (`0x48`)
  - [ ] Frontlight digipot (`0x2E`)
  - [ ] Buzzer digipot (`0x2F`)

Exit criteria:
- [ ] Frontlight brightness write works repeatedly.
- [ ] Buzzer frequency write works with retries and no lockups.
- [ ] No hard I2C dead-state after repeated NACK injection tests.

## Phase 4: GPIO Fast Path for E-Paper Waveforms

- [ ] Port direct register writes currently used for timing-critical panel scan:
  - [ ] `gpio_out_set/clear`
  - [ ] `gpio_out1_set/clear`
  - [ ] Pin mode transitions for panel on/off and Z-state
- [ ] Keep timing-sensitive sequence order unchanged in:
  - [ ] `vscan_start`
  - [ ] `vscan_end`
  - [ ] `hscan_start`
  - [ ] `clean`
  - [ ] `display_bw`
- [ ] Verify no accidental optimization removes pulse timing behavior.

Exit criteria:
- [ ] Full refresh succeeds with expected image orientation/contrast.
- [ ] No obvious regressions in ghosting compared to baseline.
- [ ] Panel power-up/power-down sequence is stable across repeated runs.

## Phase 5: Application Runtime Port

- [ ] Replace `std::thread::sleep` in `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs` with:
  - [ ] blocking HAL delay (initial port), or
  - [ ] Embassy timer (`esp-rtos`) if async path is chosen.
- [ ] Replace libc time calls (`time`, `localtime_r`) with explicit strategy:
  - [ ] temporary monotonic uptime labels, or
  - [ ] SNTP/RTC integration phase.
- [ ] Confirm `embedded-graphics` + `u8g2-fonts` path still compiles and renders.

Exit criteria:
- [ ] Clock screen renders on target.
- [ ] Refresh interval loop remains stable for multi-hour soak.

## Phase 6: Test Matrix and Stabilization

- [ ] Add a hardware test checklist script/doc:
  - [ ] Cold boot 20 times
  - [ ] 200 display refreshes
  - [ ] 100 frontlight writes
  - [ ] 100 buzzer notes
  - [ ] I2C bus fault recovery scenarios
- [ ] Capture and compare metrics versus baseline:
  - [ ] Boot latency
  - [ ] Refresh time
  - [ ] Failure count
  - [ ] Power profile (if measurement setup available)
- [ ] Fix regressions before enabling extra sensors/features.

Exit criteria:
- [ ] Zero critical failures in soak run.
- [ ] Known issues list is explicit and acceptable.

## Phase 7: Cutover

- [ ] Remove dead ESP-IDF-only files/config.
- [ ] Update docs for new toolchain and flash steps.
- [ ] Update CI to build the new target.
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

