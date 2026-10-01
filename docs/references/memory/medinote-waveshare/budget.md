# Medinote / Waveshare memory budget

## Scope

ESP32-S3, `targets/medinote-waveshare`, binary `medinote-waveshare`.
`wifi-storage` with default BLE/UI features is the intended default. Until feature
defaults change, build with `--features crash-screen,wifi-storage`. Reduced builds
need separate measurements and do not qualify this production composition.

## Layout and placement

The target uses esp-hal's ESP32-S3 map through
[its linker configuration](../../../../targets/medinote-waveshare/.cargo/config.toml).
Internal instruction/data windows alias SRAM; `.rwdata_dummy` accounts for resident
instructions/vectors in the data window. Do not add the aliases as separate capacity.
CPU0 stack receives the remainder after linked internal reservations.

| Region / owner | Contract |
| --- | --- |
| Main internal RAM | UI/task statics, panel framebuffer, 48 KiB LVGL arena and 16,000-byte L8 draw buffer |
| HAL post-boot reclaimed RAM | `wifi-storage` owns a 65,536-byte internal radio heap, registered once after bootloader return |
| PSRAM | Target maps an exclusive byte-only upload/network buffer; it is not registered with the global allocator |
| SD transport | FAT state and aligned DMA staging remain internal |

[Network memory](../../../../targets/medinote-waveshare/src/network_memory.rs)
owns reclaimed heap and PSRAM mapping; [HTTP composition](../../../../targets/medinote-waveshare/src/net_http.rs)
owns byte-buffer sizing. [LVGL configuration](../../../../targets/medinote-waveshare/lvgl/lv_conf.h)
and [draw storage](../../../../boards/waveshare-rlcd42/src/panel_lvgl.rs) own UI reserves.
Keep controller objects, atomics, task state and DMA workspaces internal.
The reduced BLE-only composition uses a different heap arrangement; its margin
must not substitute for the Wi-Fi/BLE composition's margin.

## Acceptance

[The S3 ELF guard](../../../../scripts/ci/check_stack_risk.sh) requires at least
50,000 linked stack bytes and 8,192 bytes of modeled BLE/Hourglass path margin.
It also checks generated frames and BLE-report placement. These are static guards,
not observed runtime peaks. The [shared radio owner](../../../../platform/connectivity/netstack/src/owner.rs)
checks live internal capacity and contiguous allocation before granting BLE ownership;
validate its requirements on S3 rather than borrowing Inkplate measurements.

## Qualification

The integrated Wi-Fi/BLE/storage composition still requires candidate-specific
boot, handoff/reconnect, upload/display, heap/stack and sleep/wake qualification.
Requalify reclaimed-memory placement, allocator, buffer-capacity or workload changes.
Rebuild volatile state after deep sleep; retained PSRAM/heap contents are not a
cross-wake contract. Follow [memory validation](../../../guides/memory/validation.md).
