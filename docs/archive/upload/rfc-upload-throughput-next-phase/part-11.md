# RFC: Upload Throughput Next Phase (Part 11)

## 11.57 2026-03-05 Protocol-Consistency A/B (Post-Start Reapply)

Objective:

- use the reference-library suggestion to verify protocol handling parity and
  test whether explicit post-start protocol reapply recovers scan reliability
  during Step 65 root-cause work.

Reference comparison:

- sibling parent library (`Inkplate-Arduino-library`) uses STA-mode connect
  (`WiFi.mode(WIFI_MODE_STA)` + `WiFi.begin`) and does not set explicit
  protocol/country overrides.
- current firmware path already uses STA client config and esp-radio default
  STA protocol profile (`802.11 b/g/n`).

Implementation:

- files:
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
    - add compile-time knob
      `MEDITAMER_WIFI_REAPPLY_PROTOCOL_AFTER_START` (`0` default).
    - add helper `maybe_reapply_sta_protocol_after_start(...)` that calls
      `controller.set_protocol(b/g/n)` after successful start when knob is on.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`
    - invoke post-start protocol reapply helper.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
    - include `reapply_protocol_after_start` in `scan_entry_readiness` log.

Runs:

- exploratory (default 8-round profile, not used for A/B decision):
  - `logs/wifi_discovery_debug_step65_protocol_ab_off_1r_20260305_104638.log`
  - drifted to low-memory/transport failure late in run; not comparable.
- bounded A/B (`rounds=1`, `recover_before_round=false`, listener off):
  - off:
    `logs/wifi_discovery_debug_step65_protocol_ab_off_1r_20260305_111316.log`
  - on:
    `logs/wifi_discovery_debug_step65_protocol_ab_on_1r_20260305_111518.log`

Observed (bounded A/B):

- both off/on runs:
  - `ready=true`, `zero_discovery=false`, `scan_zero=0`, `scan_nonzero=1`,
    `ssid_seen=2`.
  - positive scan stage in both logs:
    `scan_stage end ... outcome=ok ... result_count=10`.
- knob behavior:
  - off log: `reapply_protocol_after_start=false`,
    `post_start_protocol_reapply` markers `0`.
  - on log: `reapply_protocol_after_start=true`,
    `post_start_protocol_reapply result=ok profile=bgn` marker `1`.

Interpretation:

- explicit post-start protocol reapply executes correctly but did not show a
  discovery improvement in the bounded sample (both variants already passed).
- keep knob non-default and continue deeper radio/driver readiness root-cause
  work for Step 65.

## 11.58 2026-03-05 Country-Policy A/B (US Override at Wi-Fi Init)

Objective:

- probe whether ESP radio country initialization contributes to discovery-empty
  recurrence by A/B-testing default country config vs explicit US override.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi.rs`
    - add compile-time knob `MEDITAMER_WIFI_COUNTRY_US_OVERRIDE`
      (`0` default, fallback `WIFI_COUNTRY_US_OVERRIDE`).
    - when enabled, `wifi_runtime_config()` applies:
      `with_country_code(CountryInfo::from(*b"US"))`.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
    - extend `scan_entry_readiness` telemetry with
      `country_us_override={true|false}`.

Runs (bounded, same profile, `rounds=1`, listener off):

- off:
  `logs/wifi_discovery_debug_step65_country_ab_off_1r_20260305_112317.log`
- on:
  `logs/wifi_discovery_debug_step65_country_ab_on_1r_20260305_112743.log`

Observed:

- both variants produced identical discovery-empty counters:
  - `ready=false`, `zero_discovery=true`
  - `scan_zero=42`, `scan_nonzero=0`, `ssid_seen=0`
- both logs showed repeated all-zero stage completions:
  `scan_stage end ... outcome=ok ... result_count=0`
  with `event scan_done status=0 count=0`.
- telemetry verified knob path:
  - off log: `country_us_override=false`
  - on log: `country_us_override=true`.

Interpretation:

- changing init-time country code to US did not recover non-zero discovery in
  this bounded A/B sample.
- keep override off by default; continue deeper radio/driver readiness
  investigation.

## 11.59 2026-03-05 Scan-Entry Driver-State Probe (Protocol/Country FFI)

Objective:

- verify actual driver state at scan entry (not just compile-time knobs) via
  low-level `esp_wifi_get_protocol` / `esp_wifi_get_country` reads.

Implementation:

- files:
  - `Cargo.toml`: add direct diagnostics dependency `esp-wifi-sys` (`esp32`).
  - `src/firmware/storage/upload/wifi/connect/driver_state.rs`:
    - add compile-time knob
      `MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG` (`0` default).
    - add `maybe_log_scan_entry_driver_state()` called at scan entry.
    - logs protocol bitmap and country fields (`cc/schan/nchan/max_tx_power/policy`).
  - `src/firmware/storage/upload/wifi/connect/mod.rs`:
    - register new module.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`:
    - call probe before `scan_entry_readiness`.

Runs (bounded, same profile, `rounds=1`, listener off):

- diag on, country default (CN path):
  `logs/wifi_discovery_debug_step65_driverstate_diag_offcountry_1r_20260305_115535.log`
- diag on, country US override:
  `logs/wifi_discovery_debug_step65_driverstate_diag_oncountry_1r_20260305_120043.log`

Observed:

- both runs still failed discovery in bounded sample:
  - off-country: `scan_zero=46`, `scan_nonzero=0`
  - on-country: `scan_zero=45`, `scan_nonzero=0`
- driver-state probe succeeded at scan entry in both runs:
  - `protocol_rc=0`, `protocol_bitmap=0x07` (b/g/n) consistently.
  - country reflected runtime config:
    - off-country: `cc=CN.`, `max_tx_power=20`
    - on-country: `cc=US.`, `max_tx_power=30`
  - both: `schan=1`, `nchan=13`, `policy=1`.

Interpretation:

- driver protocol/country state is readable and matches configured branch at
  scan entry; failure is not explained by stale/mismatched protocol bitmap or
  inability to apply country config.
- next root-cause focus should move deeper than protocol/country setup.

## 11.60 2026-03-05 Direct IDF Scan Comparator at Scan Entry

Objective:

- determine whether discovery-empty is caused by our staged scan path/wrapper
  or by deeper radio/driver scan results by running a direct IDF scan before
  staged scan work in the same cycle.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi/connect/idf_scan_compare.rs`
    - add compile-time knob
      `MEDITAMER_WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG` (`0` default).
    - at scan entry, call direct IDF path:
      `esp_wifi_scan_start(NULL, true)` +
      `esp_wifi_scan_get_ap_num` +
      bounded `esp_wifi_scan_get_ap_records`.
    - log direct AP count/target visibility/top AP.
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
    - register module.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
    - invoke direct comparator before staged scan execution.

Runs (bounded, same profile, `rounds=1`, listener off):

- `logs/wifi_discovery_debug_step65_idfcompare_diag_1r_20260305_121115.log`
- `logs/wifi_discovery_debug_step65_idfcompare_diag_repeat_1r_20260305_121434.log`

Observed:

- both runs remained discovery-empty:
  - run 1: `scan_zero=39`, `scan_nonzero=0`
  - run 2: `scan_zero=43`, `scan_nonzero=0`.
- direct comparator signal matched staged scan failure:
  - `scan_entry_idf_compare outcome=ok ... ap_num=0 records_returned=0`.
  - staged scan logs in same runs were all-zero:
    `scan_stage end ... outcome=ok ... result_count=0`.
- driver-state probe in same runs remained stable:
  `protocol_bitmap=0x07`, `cc=CN`, `policy=1`.

Interpretation:

- this rejects a staged-wrapper-only hypothesis: direct IDF scan path also
  reports zero APs under the failing state.
- root cause remains deeper radio/driver readiness behavior.

## 11.61 2026-03-05 Expanded Runtime-State Probe at Scan Entry

Objective:

- verify broader Wi-Fi runtime state while discovery is failing, beyond
  protocol/country:
  - mode
  - channel
  - power-save mode
  - max TX power
  - event mask
  - driver default scan parameters.

Implementation:

- file:
  - `src/firmware/storage/upload/wifi/connect/driver_state.rs`
    - extend `MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG` output to include:
      `esp_wifi_get_mode/get_channel/get_ps/get_max_tx_power/get_event_mask/get_scan_parameters`
      in addition to existing protocol/country fields.

Validation run (bounded, `rounds=1`, listener off):

- `logs/wifi_discovery_debug_step65_runtimeprobe_diag_1r_20260305_122658.log`

Observed:

- run remained discovery-empty:
  - `scan_zero=43`, `scan_nonzero=0`.
- scan-entry runtime probe was stable across restart cycles:
  - `mode_rc=0 mode=1` (STA)
  - `channel_rc=0 primary=1 second=0`
  - `ps_rc=0 ps=0` (power save off)
  - `max_tx_power_rc=0 max_tx_power=80`
  - `event_mask_rc=0 event_mask=0x00000001`
  - `protocol_rc=0 protocol_bitmap=0x07`
  - `country_rc=0 cc=CN. schan=1 nchan=13 policy=1`
  - `scan_defaults_rc=0 scan_active_min=0 scan_active_max=120 scan_passive=360 scan_home_dwell=30`.

Interpretation:

- no obvious runtime-state incoherence was observed at scan entry during
  discovery-empty cycles.
- root cause remains deeper than common mode/channel/PS/scan-default state.


_Continued in [Part 11, continuation 2](./part-11-02.md)._
