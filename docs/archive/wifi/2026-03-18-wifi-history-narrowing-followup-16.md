# 2026-03-18 Wi-Fi History Narrowing Follow-up 16

## Scope

This follow-up closes the immediate `chm_init` post-install / arm question left by follow-up 15.

It answers one question:

- aside from callback identity, do app and comparator differ on the adjacent timeout/arm sequence around the two `g_chm` slot installs?

## Artifacts

- App binary:
  - `target/xtensa-esp32-none-elf/debug/meditamer`
- Comparator binary:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`
- App runtime reference:
  - `logs/flash_capture_20260318_142006/capture.log`
- Comparator runtime reference:
  - `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`

## Static Comparison

### App `chm_init` around slot install and arm

Relevant app window:

- `40122e40 .. 40122ea1`

Observed shape:

1. if `a2+8` is nonzero and `< a2+12`, app logs and then:
   - calls `g_osi_funcs_p[236]` with `g_chm + 36`
   - then calls `g_osi_funcs_p[232]` with `g_chm + 36` and timeout from `a2+8`
2. app then unconditionally reaches the second slot path:
   - calls `g_osi_funcs_p[236]` with `g_chm + 56`
   - then calls `g_osi_funcs_p[232]` with `g_chm + 56` and timeout from `a2+12`
3. app returns at `40122ea1`

### Comparator `chm_init` around slot install and arm

Relevant comparator window:

- `40109cd4 .. 40109d25`

Observed shape:

1. if `a2+8` is nonzero and `< a2+12`, comparator logs and then:
   - calls `g_osi_funcs_p[236]` with `g_chm + 36`
   - then calls `g_osi_funcs_p[232]` with `g_chm + 36` and timeout from `a2+8`
2. comparator then reaches the second slot path:
   - calls `g_osi_funcs_p[236]` with `g_chm + 56`
   - then calls `g_osi_funcs_p[232]` with `g_chm + 56` and timeout from `a2+12`
3. comparator returns at `40109d25`

## Conclusion

The immediate post-install / arm sequence matches.

That closes these hypotheses:

- different timeout words at `a2+8` / `a2+12`
- different choice of `g_osi_funcs_p` setfn/arm entries
- different first-slot vs second-slot arm ordering
- missing arm on the app path

So the remaining coupled split is now tighter:

1. callback-family choice at the two `chm_init` installs
2. timer substrate behind those installs

Runtime evidence already shows the substrate difference:

- app slots have non-null timer handles and live compat-timer state
- comparator slots keep `timer_handle_ptr=0` and no live timer-handle object in the slot itself

## Strongest Current Statement

At the `g_chm` slot pair, app and comparator match on:

- branch shape
- timeout-source words
- setfn/arm sequencing
- slot offsets `+36` and `+56`

They differ on:

- callback family
- timer substrate model

That is now the most precise root-cause boundary established so far.

## Practical Next Step

Stop diffing `chm_init` control flow.

The next useful comparison is the timer substrate implementation itself:

1. compare old `esp-wifi 0.15.1` `ets_timer_setfn` / timer compat implementation to current `esp-radio 0.17.0`
2. explain why comparator slot `priv_` stays null while app slot `priv_` becomes a live timer handle
3. treat the active root-cause target as:
   - callback-family choice in `chm_init`
   - plus the old-vs-new timer substrate semantics for those same slot timers
