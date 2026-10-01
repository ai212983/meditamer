## 11.50 2026-03-05 Host-Recover Stale Guard-State Reset Fix + Live Check

Code fix:

- file:
  `src/firmware/storage/upload/wifi/connect/prepare/prepare_preconditions.rs`
- on `NetControlCommand::Recover`, firmware now resets:
  - `discovery_sweep_exhausted_streak = 0`
  - `zero_discovery_hard_guard_restarts = 0`
  - `force_full_channel_probe_next_scan = false`
- rationale:
  prevent stale pre-connect zero-discovery guard state from carrying across an
  explicit host recover cycle.

Live validation sequence (same flashed firmware):

- step A (drive terminal zero-discovery):
  - log:
    `logs/wifi_discovery_debug_step65_postpatch_seqA_norecover_listeneroff_1r_20260305_091317.log`
  - summary:
    `scan_zero=40`, `scan_nonzero=0`, `failure_class=discovery_empty`
- step B (host-recover round immediately after A):
  - log:
    `logs/wifi_discovery_debug_step65_postpatch_seqB_recover_listeneroff_1r_20260305_091624.log`
  - key markers:
    - `post-hard-recover-watchdog start reason=host_recover`
    - repeated `scan_zero_discovery_driver_restart` with
      `scan_rounds=1..4`, `zero_scan_rounds=1..4`, `connect_begins=0`
    - terminal only at the expected guard cap:
      `scan_zero_discovery_guard_terminal` after round 4.

Interpretation:

- host recover now starts a fresh discovery guard cycle instead of inheriting
  terminal guard progress from the prior failed cycle.
- root blocker remains unchanged: every scan stage still returns zero results
  (`scan_nonzero=0`).

## 11.51 2026-03-05 Scan-Stage Lifecycle Diagnostics (Step 65)

Objective:

- determine whether scan starvation is caused by scan-stage timeout/error paths
  or by successful scan completions that still return zero AP results.

Implementation:

- file:
  `src/firmware/storage/upload/wifi/scan.rs`
- added per-stage lifecycle logs:
  - `scan_stage begin ...`
  - `scan_stage end ... outcome=ok|err|timeout elapsed_ms=...`
  with policy and candidate-state context.

Live repro:

- log:
  `logs/wifi_discovery_debug_step65_scanstage_diag_1r_20260305_092439.log`
- profile:
  `recover_before_round=false`, `disable_listener_during_probe_rounds=true`,
  `rounds=1`.

Observed:

- run summary stayed failing:
  - `ready=false`, `zero_discovery=true`, `scan_zero=44`, `scan_nonzero=0`.
- stage outcomes:
  - no `outcome=timeout`
  - no `outcome=err`
  - all observed stages ended `outcome=ok` with `result_count=0`,
    `candidate_count_after=0`, `saw_nonzero_after=false`.
- elapsed profile (from `scan_stage end` lines):
  - `active_broad`: `n=4`, `avg=7877 ms` (timeout budget `15000 ms`)
  - `active_directed`: `n=4`, `avg=7878 ms` (timeout budget `8000 ms`)
  - `passive`: `n=4`, `avg=19570 ms` (timeout budget `27000 ms`)
  - `probe`: `n=32`, `avg=639 ms` (timeout budget `6000 ms`)

Interpretation:

- starvation is not currently a scan timeout/error path.
- scan calls are completing successfully but returning empty result sets across
  all stages/channels in this environment.
- next targeted mitigation should test post-start scan readiness/settle timing
  (driver-start -> first-scan window) rather than extending scan timeouts.

## 11.52 2026-03-05 Post-Start Settle Timing Probe (`800 ms` vs `2000 ms`)

Objective:

- test whether extending the driver-start -> first-scan settle window restores
  non-zero scan results under the same failing discovery profile.

Implementation:

- file:
  `src/firmware/storage/upload/wifi.rs`
- added compile-time knob:
  `MEDITAMER_WIFI_POST_START_SETTLE_MS` (fallback
  `WIFI_POST_START_SETTLE_MS`, default `800`, bounded `100..10000`).

Runs (`recover_before_round=false`, listener off):

- default settle (`800 ms`) reference:
  `logs/wifi_discovery_debug_step65_scanstage_diag_1r_20260305_092439.log`
  - `scan_zero=44`, `scan_nonzero=0`
  - stage profile:
    `active_broad n=4 avg=7877 ms`,
    `active_directed n=4 avg=7878 ms`,
    `passive n=4 avg=19570 ms`,
    `probe n=32 avg=639 ms`
- override settle (`2000 ms`) probe:
  `logs/wifi_discovery_debug_step65_scanstage_diag_settle2000_1r_20260305_093324.log`
  - `scan_zero=35`, `scan_nonzero=0`
  - stage profile:
    `active_broad n=4 avg=7877 ms`,
    `active_directed n=4 avg=7878 ms`,
    `passive n=4 avg=19570 ms`,
    `probe n=30 avg=639 ms`
  - still no `outcome=err`/`outcome=timeout`, and no stage with
    `result_count > 0`.

Interpretation:

- longer post-start settle reduced cycle count within the bounded round but did
  not recover non-zero scans.
- keep default settle (`800 ms`) and continue root-cause focus elsewhere in the
  scan/radio path.

## 11.53 2026-03-05 Raw-Broad-Scan Bypass Split (`start_raw_scan_diag`)

Objective:

- isolate staged scan-pipeline/config effects from deeper radio/driver state by
  issuing a plain broad scan right after `start_async()` and comparing counts to
  staged scan counts in the same run.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi/driver.rs`
    - added `raw_broad_scan_config()` (default broad config; no ssid/bssid hints).
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`
    - added optional startup raw scan diag after `start_async()`.
  - `src/firmware/storage/upload/wifi.rs`
    - added compile-time knob:
      `MEDITAMER_WIFI_START_RAW_SCAN_DIAG` (`0` default, `1` enable).
- raw-scan log marker:
  `upload_http: start_raw_scan_diag outcome=... result_count=...`.

Live repro (`diag on`):

- flashed with:
  `MEDITAMER_WIFI_START_RAW_SCAN_DIAG=1`
- log:
  `logs/wifi_discovery_debug_step65_rawscan_diag_on_1r_20260305_094620.log`
- profile:
  `recover_before_round=false`, `disable_listener_during_probe_rounds=true`,
  `rounds=1`.

Observed:

- round summary remained failing:
  - `scan_zero=46`, `scan_nonzero=0`.
- raw broad scan samples:
  - `n=4`, `avg_count=0.00`, `positive_runs=0`
  - each sample `outcome=ok`, `elapsed_ms~220`, `result_count=0`.
- staged scan samples in same run:
  - `active_broad`: `n=4`, `avg_count=0.00`
  - `active_directed`: `n=4`, `avg_count=0.00`
  - `passive`: `n=4`, `avg_count=0.00`
  - `probe`: `n=31`, `avg_count=0.00`

Interpretation:

- staged scan config/pipeline is not the primary cause of all-zero results.
- dominant issue is deeper radio/driver readiness/state under this AP-dense
  environment (raw broad scan is also empty).

## 11.54 2026-03-05 Driver-Start Readiness Timing Probe (`start_readiness_probe`)

Objective:

- verify whether scans are being invoked before driver-start readiness settles.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi.rs`:
    `MEDITAMER_WIFI_START_READINESS_PROBE` (`0` default, `1` enable).
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`:
    `start_readiness_probe` checkpoints (`0/200/800 ms`).
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`:
    `scan_entry_readiness` timing log.

Live repro (`diag on`):

- flashed with `MEDITAMER_WIFI_START_READINESS_PROBE=1`.
- log:
  `logs/wifi_discovery_debug_step65_startreadiness_diag_on_1r_20260305_095649.log`.

Observed:

- readiness checkpoints: all `started=true` (`12/12` lines).
- scan entry timing: `start_ok_age_ms` ~`1848..1851` (`n=4`, avg `1848.8`).
- all scan stages still `result_count=0` in the same run.

Interpretation:

- scan entry is not occurring too early relative to `start_ok`.
- primary blocker remains deeper radio/driver-side all-zero scanning, not
  immediate post-start readiness lag.

## 11.55 2026-03-05 Driver-Start Hard-Reset A/B (`force_stop_before_start`)

Objective:

- test whether forcing a pre-start stop/disconnect before each `start_async()`
  recovers non-zero scan results.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi.rs`:
    `MEDITAMER_WIFI_FORCE_STOP_BEFORE_START` (`0` default, `1` enable).
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`:
    optional pre-start `disconnect_and_stop_with_timeout(...)` + short settle.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`:
    `scan_entry_readiness` now logs `force_stop_before_start`.

Runs (`recover_before_round=false`, listener off, rounds=1):

- baseline (`force_stop_before_start=0`):
  `logs/wifi_discovery_debug_step65_forcestop_ab_off_1r_20260305_100906.log`
  - summary: `scan_zero=41`, `scan_nonzero=0`.
- variant (`force_stop_before_start=1`):
  `logs/wifi_discovery_debug_step65_forcestop_ab_on_1r_20260305_101331.log`
  - summary: `scan_zero=32`, `scan_nonzero=0`.
  - additional signal:
    `force_stop_before_start stop timeout=5000ms` repeated (`4` samples).

Observed:

- both runs kept staged scan `result_count=0` across all stages.
- no positive (`>0`) scan-result stages in either run.
- scan-entry age from `start_ok` remained equivalent (`~826 ms`) across off/on.

Interpretation:

- forcing pre-start stop/disconnect did not recover discovery (`scan_nonzero`
  stayed `0`) and introduced extra stop-timeout churn.
- keep this knob non-default and continue focusing on deeper radio/driver
  behavior rather than pre-start hard-reset forcing.

## 11.56 2026-03-05 Scan-Entry Event-Age Introspection (Radio Event Timing)

Objective:

- verify whether scan entry is racing immediately after `sta_start`/`sta_stop`
  or on a stale `scan_done` edge.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi.rs`:
    added atomics for last `sta_start`, `sta_stop`, `scan_done` timestamp/id/count/status.
  - `src/firmware/storage/upload/wifi/connect/events.rs`:
    update those atomics in event handlers.
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`:
    extended `scan_entry_readiness` with event-age fields.

Live repro:

- log:
  `logs/wifi_discovery_debug_step65_eventage_diag_1r_20260305_102456.log`
  (`recover_before_round=false`, listener off).

Observed (`scan_entry_readiness`, `n=4`):

- `start_ok_age_ms`: `~826..832`
- `sta_start_age_ms`: `~850..851`
- `sta_stop_age_ms`: first entry `-1` (no prior stop), then `~3452..3463`
- `last_scan_done_age_ms`: first entry `-1`, then `~3663..3671`
- `last_scan_done_count/status`: always `0/0`
- no entries with `last_scan_done_age_ms < 1000`.

Outcome:

- scan entry is not happening on an immediate post-stop or stale-immediate
  `scan_done` edge in this repro.
- all stage results remained zero (`result_count=0`), so failure remains a
  deeper all-zero radio/driver scan condition.
