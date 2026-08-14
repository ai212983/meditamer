# 2026-03-18 Wi-Fi History Narrowing Follow-up 14

## Scope

This follow-up closes the comparator-side `g_chm` timer-slot state question from follow-up 13.

It answers two questions:

- what callback family is actually installed into the comparator's two `g_chm` timer slots?
- is the old stack using the same live timer-handle model as the current app?

## Artifacts

- Baseline app capture:
  - `logs/flash_capture_20260318_142006/capture.log`
- Comparator capture with direct `g_chm` slot logging:
  - `logs/flash_capture_20260318_comparator_chm_slots/capture.log`
- Comparator capture with `ets_timer_setfn` caller logging:
  - `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`
- Comparator binary:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`

## Results

### 1. The comparator does not keep live timer handles in the two `g_chm` slots

At every comparator checkpoint we instrumented:

- `after_start`
- `idf_explicit_compare postcall`
- `before_get_ap_num`
- `after_get_ap_num`
- `after_get_ap_records`

both slot objects stay:

- `timer_handle_ptr=0x0`
- `callback_ptr=0x0`
- `arg_ptr=0x0`
- `active=0`

Concrete evidence:

- `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`
  - lines `100-102`
  - lines `235-237`
  - lines `296-298`
  - lines `357-359`
  - lines `417-419`

This means the old stack is not using the same app-style live timer-handle model for those two embedded `g_chm` ETS timers.

### 2. The comparator still installs a concrete callback family into those ETS timer slots

The same comparator capture shows direct `ets_timer_setfn` events for the two slot timer addresses right after start:

- slot 0 timer ptr `0x3ffc6b24`
- slot 1 timer ptr `0x3ffc6b38`

with:

- `callback_ptr=0x401097a8`
- `arg_ptr=0x0` / `arg_ptr=0x1`
- `caller_ptr=0x80109c31` / `caller_ptr=0x80109c42`

Concrete evidence:

- `logs/flash_capture_20260318_comparator_chm_slots_callers/capture.log`
  - lines `76-77`

Symbol resolution on the comparator binary shows:

- `0x401097a8` lies inside `ieee80211_timer_process`
- `0x80109c31` / `0x80109c42` lie inside `chm_init`

So the old stack behavior is:

- `chm_init` directly installs `ieee80211_timer_process` into the two `g_chm` slot timers
- one slot gets `arg=0`, the other gets `arg=1`

### 3. The baseline app is materially different at the same conceptual slots

The baseline app capture already showed the corresponding slot pair as compat-timer-backed live timers:

- `ets_timer_ptr=0x3ffcae30` / `0x3ffcae44`
- non-null `timer_handle_ptr`
- callback family `0x4012279c`
- args `0` / `1`

Concrete evidence:

- `logs/flash_capture_20260318_142006/capture.log`
  - lines `232-233`
  - lines `263-264`
  - lines `422-423`
  - lines `492-493`
  - lines `641-642`

The baseline app `timer_compat_setfn_recent` caller resolves to the app's `esp_radio::common_adapter::ets_timer_setfn` wrapper, and the installed callback family is `nan_dp_schedule_ndc_start`.

So the app behavior is:

- the same conceptual channel-manager timer pair is routed through the compat timer layer
- the installed callback family is `nan_dp_schedule_ndc_start`, not `ieee80211_timer_process`

## Conclusion

This is a stronger boundary than the earlier generic timer-arm observations.

The split is now:

- comparator:
  - direct ETS-slot callback installation from `chm_init`
  - callback family `ieee80211_timer_process`
  - no live timer-handle objects behind the two `g_chm` slots
- baseline app:
  - compat-timer-backed slot state
  - callback family `nan_dp_schedule_ndc_start`
  - non-null live timer handles behind the two slots

That means the remaining root-cause target is no longer just “timer callbacks exist”.
It is the combined timer substrate and callback-family divergence at the exact `g_chm` slot pair.

## Practical Next Step

The next useful comparison is static and local to the app image:

1. determine whether app `chm_init` still contains a direct `ieee80211_timer_process` install path at all
2. if not, identify the app-side producer that installs `nan_dp_schedule_ndc_start` into the same conceptual slot pair
3. compare the state transition from that producer into the later explicit-scan branch split
