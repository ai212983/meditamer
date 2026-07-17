# 2026-03-18 Wi-Fi History Narrowing Follow-up 13

## Scope

This follow-up closes the static `chm_init` code-shape question raised by follow-up 12.

It answers one question:

- are the baseline app and working comparator using materially different `chm_init` timer-arm control flow, or the same control flow with different runtime callback/setup state?

## Artifacts

- App disassembly window:
  - `target/xtensa-esp32-none-elf/debug/meditamer`
- Comparator disassembly window:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`
- Supporting runtime captures:
  - `logs/flash_capture_20260318_141757/capture.log`
  - `logs/flash_capture_20260318_142006/capture.log`

## Static Comparison

### App `chm_init`

Relevant window:

- `0x40122dc0 .. 0x40122ea1`

Key shape:

1. common prologue loads `g_chm`, current/home/op-channel state, and logs
2. optional current-channel adjustment path
3. branch on the two timer-slot words at `a2+8` and `a2+12`
4. first timer path:
   - guard ends at `0x40122e3d`
   - callback object at `g_chm + 36`
   - timeout value from `a2+8`
   - arm call path ends at `0x40122e6e`
5. second timer path:
   - starts at `0x40122e71`
   - callback object at `g_chm + 56`
   - timeout value from `a2+12`
   - arm call path ends at `0x40122e9e`

### Comparator `chm_init`

Relevant window:

- `0x40109968 .. 0x40109a39`

Key shape:

1. same prologue over `g_chm`, current/home/op-channel state, and logs
2. same optional current-channel adjustment path
3. same branch on the two timer-slot words at `a2+8` and `a2+12`
4. first timer path:
   - guard ends at `0x401099d9`
   - callback object at `g_chm + 36`
   - timeout value from `a2+8`
   - arm call path ends at `0x40109a06`
5. second timer path:
   - starts at `0x40109a09`
   - callback object at `g_chm + 56`
   - timeout value from `a2+12`
   - arm call path ends at `0x40109a36`

## Conclusion

The app and comparator `chm_init` timer-arm control flow is structurally the same.

The earlier baseline app runtime callsites from follow-up 12:

- `0x40122e71`
- `0x40122ea1`

are not evidence of a unique app-only arm algorithm. They are the app-image equivalents of the same two comparator timer-arm branches.

That closes the idea that the root split is in `chm_init` code generation itself.

## Narrowed Boundary

What remains live after this comparison:

- the timer-object callback/setup state feeding those identical callsites
- the callback identity actually installed into the two `g_chm` timer objects
- the later runtime behavior after those same callsites run

Current strongest statement:

- baseline app and comparator share the same `chm_init` timer-arm structure
- the live split is therefore in runtime timer-object contents or later consumption, not in the `chm_init` arm code shape itself

## Practical Next Step

The next useful target is the timer-object producer path, not `chm_init` arm code.

1. Compare baseline app vs comparator `compat_timer_setfn` / live timer-object state for the two `g_chm` slots.
2. Prove whether the comparator installs a different callback family into the same `g_chm + 36` / `g_chm + 56` slots.
3. If that callback identity differs, trace who installs it and when.
