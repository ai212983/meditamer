# Upload Throughput History (Part 12)

## 2026-03-05: force-stop-before-start A/B did not recover discovery

Change under test:

- compile-time knob `MEDITAMER_WIFI_FORCE_STOP_BEFORE_START` to run a
  pre-start `disconnect+stop` before each `start_async()` attempt.

A/B runs (`recover_before_round=false`, listener off, `rounds=1`):

- off:
  `logs/wifi_discovery_debug_step65_forcestop_ab_off_1r_20260305_100906.log`
  - summary: `scan_zero=41`, `scan_nonzero=0`.
- on:
  `logs/wifi_discovery_debug_step65_forcestop_ab_on_1r_20260305_101331.log`
  - summary: `scan_zero=32`, `scan_nonzero=0`.

Observed:

- staged scan `result_count` stayed `0` across all stages in both runs.
- no positive scan-result stages in either run.
- forcing pre-start stop introduced repeated:
  `force_stop_before_start stop timeout=5000ms` (`4` samples).

Conclusion:

- hard-reset forcing before start does not recover non-zero discovery.
- keep knob off by default and treat as rejected variant for this root-cause
  branch.

## 2026-03-05: event-age introspection rejects immediate start/stop race

Implementation:

- added event-age telemetry for scan entry:
  - last `sta_start` age
  - last `sta_stop` age
  - last `scan_done` age/count/status/id.

Validation run:

- `logs/wifi_discovery_debug_step65_eventage_diag_1r_20260305_102456.log`
  (`recover_before_round=false`, listener off).

Observed (`scan_entry_readiness`, `n=4`):

- `sta_start_age_ms`: `~850..851`
- `sta_stop_age_ms`: `-1` (first), then `~3452..3463`
- `last_scan_done_age_ms`: `-1` (first), then `~3663..3671`
- no scan-entry sample had `last_scan_done_age_ms < 1000`.
- all staged scans still `result_count=0`.

Conclusion:

- scan entry is not immediately racing a recent stop/scan-done edge.
- zero-result condition remains deeper in radio/driver scan behavior.

## 2026-03-05: protocol reapply A/B (post-start) showed no bounded delta

Change under test:

- compile-time knob `MEDITAMER_WIFI_REAPPLY_PROTOCOL_AFTER_START` (`0` default)
  to call `set_protocol(802.11 b/g/n)` immediately after `start_async`.

Reference parity check:

- parent sibling `Inkplate-Arduino-library` uses STA connect only
  (`WiFi.mode(WIFI_MODE_STA)` + `WiFi.begin`) and does not set explicit
  protocol/country overrides.
- current firmware path already follows STA-only behavior with esp-radio
  default `b/g/n` client profile.

Bounded A/B (`rounds=1`, `recover_before_round=false`, listener off):

- off:
  `logs/wifi_discovery_debug_step65_protocol_ab_off_1r_20260305_111316.log`
- on:
  `logs/wifi_discovery_debug_step65_protocol_ab_on_1r_20260305_111518.log`

Observed:

- both runs passed discovery with identical counters:
  - `ready=true`
  - `scan_zero=0`, `scan_nonzero=1`
  - `ssid_seen=2`
- both logs contain positive scan stage:
  `scan_stage end ... outcome=ok ... result_count=10`.
- knob behavior validated:
  - off: `reapply_protocol_after_start=false`, no
    `post_start_protocol_reapply` marker.
  - on: `reapply_protocol_after_start=true`,
    `post_start_protocol_reapply result=ok profile=bgn`.

Conclusion:

- post-start protocol reapply executes correctly but did not improve bounded
  discovery behavior in this sample.
- keep knob off by default and continue deeper radio/driver readiness
  investigation for Step 65.

## 2026-03-05: country override A/B showed no discovery recovery

Change under test:

- compile-time knob `MEDITAMER_WIFI_COUNTRY_US_OVERRIDE` (`0` default),
  applying `CountryInfo::from(*b"US")` at Wi-Fi init when enabled.

Bounded A/B (`rounds=1`, `recover_before_round=false`, listener off):

- off:
  `logs/wifi_discovery_debug_step65_country_ab_off_1r_20260305_112317.log`
- on:
  `logs/wifi_discovery_debug_step65_country_ab_on_1r_20260305_112743.log`

Observed:

- both runs were identical:
  - `ready=false`, `zero_discovery=true`
  - `scan_zero=42`, `scan_nonzero=0`, `ssid_seen=0`
- both logs repeatedly emitted:
  `scan_stage end ... outcome=ok ... result_count=0`
  and `event scan_done status=0 count=0`.
- knob verification:
  - off: `country_us_override=false`
  - on: `country_us_override=true`.

Conclusion:

- country override did not recover discovery in this bounded sample.
- keep override off by default and continue deeper driver/radio root-cause
  branch work.

## 2026-03-05: scan-entry driver-state probe confirms protocol/country apply

Change under test:

- compile-time knob `MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG` (`0` default)
  to log low-level driver state at scan entry via:
  - `esp_wifi_get_protocol(STA, ...)`
  - `esp_wifi_get_country(...)`.

Bounded runs (`rounds=1`, `recover_before_round=false`, listener off):

- diag on, country default:
  `logs/wifi_discovery_debug_step65_driverstate_diag_offcountry_1r_20260305_115535.log`
- diag on, country US override:
  `logs/wifi_discovery_debug_step65_driverstate_diag_oncountry_1r_20260305_120043.log`

Observed:

- discovery remained empty in both samples:
  - off-country: `scan_zero=46`, `scan_nonzero=0`
  - on-country: `scan_zero=45`, `scan_nonzero=0`.
- driver-state probe succeeded and was stable:
  - both: `protocol_rc=0`, `protocol_bitmap=0x07` (b/g/n).
  - country reflected config:
    - off-country: `cc=CN.`, `max_tx_power=20`
    - on-country: `cc=US.`, `max_tx_power=30`
  - both: `schan=1`, `nchan=13`, `policy=1`.

Conclusion:

- protocol/country config is being applied and read correctly at scan entry.
- discovery-empty root cause remains deeper than protocol bitmap/country setup.

## 2026-03-05: direct IDF comparator matches all-zero staged scan results

Change under test:

- compile-time knob `MEDITAMER_WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG` (`0` default)
  to run direct IDF scan path at scan entry:
  - `esp_wifi_scan_start(NULL, true)`
  - `esp_wifi_scan_get_ap_num`
  - bounded `esp_wifi_scan_get_ap_records`.

Bounded runs (`rounds=1`, `recover_before_round=false`, listener off):

- `logs/wifi_discovery_debug_step65_idfcompare_diag_1r_20260305_121115.log`
- `logs/wifi_discovery_debug_step65_idfcompare_diag_repeat_1r_20260305_121434.log`

Observed:

- both runs remained all-zero in staged scan path:
  - run 1: `scan_zero=39`, `scan_nonzero=0`
  - run 2: `scan_zero=43`, `scan_nonzero=0`.
- direct comparator in same runs also reported zero APs:
  - `scan_entry_idf_compare outcome=ok ... ap_num=0 records_returned=0`.
- no staged scan sample had positive result counts in either run.

Conclusion:

- direct IDF scan comparator and staged scan agree on zero AP visibility.
- this rejects a wrapper-only/staged-path bug branch.
- failure remains deeper radio/driver readiness behavior.

## 2026-03-05: expanded runtime probe shows stable STA runtime state

Change under test:

- extend `MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG` output to include:
  - `esp_wifi_get_mode`
  - `esp_wifi_get_channel`
  - `esp_wifi_get_ps`
  - `esp_wifi_get_max_tx_power`
  - `esp_wifi_get_event_mask`
  - `esp_wifi_get_scan_parameters`
  - plus existing protocol/country reads.

Validation run (`rounds=1`, `recover_before_round=false`, listener off):

- `logs/wifi_discovery_debug_step65_runtimeprobe_diag_1r_20260305_122658.log`

Observed:

- discovery remained empty (`scan_zero=43`, `scan_nonzero=0`).
- runtime probe samples at scan entry were stable and successful:
  - `mode=1` (STA), `primary=1`, `second=0`
  - `ps=0`
  - `max_tx_power=80`
  - `event_mask=0x00000001`
  - `protocol_bitmap=0x07`
  - `country=CN` (`schan=1`, `nchan=13`, `policy=1`)
  - scan defaults: active `0..120 ms`, passive `360 ms`, home-dwell `30 ms`.

Conclusion:

- no obvious runtime-state inconsistency was observed at scan entry.
- discovery-empty root cause remains deeper than common STA runtime state.

## 2026-03-05: scan-entry promiscuous RX sweep stayed zero across channels

Change under test:

- compile-time knob `MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG` (`0` default) to
  run scan-entry promiscuous RX diagnostics with channel sweep
  (`[8, 1, 6, 11]`) and per-window packet counters.
- dwell tuning knob:
  `MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS` (default `120`).

Bounded runs (`rounds=1`, `recover_before_round=false`, listener off):

- `logs/wifi_discovery_debug_step65_promiscsweep_1r_20260305_124610.log`
- `logs/wifi_discovery_debug_step65_promiscsweep_sanity_1r_20260305_125455.log`

Observed:

- both runs remained discovery-empty (`scan_zero=44`, `scan_nonzero=0`).
- each scan-entry sweep window reported zero RX packets:
  - `channel=8 total=0`
  - `channel=1 total=0`
  - `channel=6 total=0`
  - `channel=11 total=0`
  - aggregate `total=0 mgmt=0 ctrl=0 data=0 misc=0`.
- diagnostic control calls succeeded (`set_channel_rc=0`, `enable_rc=0`,
  `disable_rc=0`, `restore_channel_rc=0`), so this is not a control-path
  failure.
- staged scan in the same cycles remained all-zero (`scan_done count=0`).

Conclusion:

- root-cause signal now points to radio RX-ingress blackout behavior, not only
  staged/IDF scan wrapper behavior.

## 2026-03-05: `esp_wifi_restore()` recovery branch rejected (runtime panic)

Change under test:

- temporary recovery branch calling `esp_wifi_restore()` after successful
  stop/restart on recovery path (`MEDITAMER_WIFI_RESTORE_AFTER_STOP`).

Validation run:

- `logs/wifi_discovery_debug_step65_restoreafterstop_promiscsweep_1r_20260305_125124.log`

Observed:

- immediate runtime panic during recovery:
  - `panicked ... esp-radio-0.17.0/src/common_adapter.rs:318:5`
  - `not yet implemented: misc_nvs_restore`.

Conclusion:

- `esp_wifi_restore()` is not usable in this stack/runtime path.
- branch was reverted and documented as rejected to prevent retry churn.


_Continued in [Part 12, continuation 2](./part-12-02.md)._
