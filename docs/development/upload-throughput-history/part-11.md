# Upload Throughput History (Part 11)

## 2026-03-05: full-gate closure reruns blocked by discovery-empty recurrence

Goal:

- close promoted-default gate step by re-running full regression gate + soak on
  `16 KiB`/`32` defaults.

Attempts:

- `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun_20260305_0753/discovery_debug.log`
- `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun2_20260305_0756/discovery_debug.log`

Observed (both attempts):

- discovery stage entered repeated zero-result scan loops:
  - `scan_done status=0 count=0` repeated (`16` and `9` samples).
- recurring discovery classing before acceptance stages:
  - `NET_STATUS ... "failure_class":"discovery_empty"` repeated (`49` and `25`
    samples).
- no `report.json` emitted because runs were terminated after prolonged
  discovery-stage churn (no progress to acceptance stages).

Interpretation:

- gate closure is currently blocked by discovery stability under this
  environment; this does not invalidate ingress-threshold promotion evidence.

Next:

- prioritize discovery-empty root-cause isolation and recovery hardening, then
  re-run full regression gate to close step 64.

## 2026-03-05: pre-connect zero-discovery short-circuit landed

Implementation:

- file changed:
  `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
- if a full scan round returns zero APs, firmware now:
  - classifies immediately as `discovery_empty` (`201`)
  - enters `scan_zero_discovery_driver_restart`
  - performs hard recover without attempting association first.

Validation snapshot:

- run:
  `logs/wifi_discovery_debug_step65_preconnect_guard_20260305_0808.log`
- compared with:
  `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun2_20260305_0756/discovery_debug.log`

Key deltas:

- `connect_begin`: `1` -> `0`
- `event sta_disconnected reason=201`: `1` -> `0`
- `scan_zero_discovery_driver_restart`: `0` -> `30`

Status:

- classing/recovery path hardened.
- guard parity patch added terminal cap (`scan_zero_discovery_guard_terminal`) to
  avoid unbounded pre-connect restart loops; terminal-path live repro is pending.
- root RF zero-scan condition still present; gate closure remains blocked pending
  further step-65 mitigation.

## 2026-03-05: discovery split test confirms host pressure is not primary

A/B setup (`rounds=3`, permissive thresholds):

- minimal-host-pressure profile:
  `logs/wifi_discovery_debug_step65_split_minpressure3r_20260305_0822.log`
- control profile:
  `logs/wifi_discovery_debug_step65_split_control3r_20260305_0832.log`

Observed:

- both runs: `ready_rounds=0`, `zero_discovery_rounds=3`,
  `total_scan_nonzero_events=0`.
- repeated `post-hard-recover-connect-stall` in both profiles.

Interpretation:

- reducing host orchestration pressure alone does not recover discovery.
- dominant blocker remains firmware/radio-side scan starvation in this
  environment; next work should focus on scan/recovery internals, not host loop
  pacing.

## 2026-03-05: watchdog causality split confirms watchdog is secondary

Question:

- does post-hard-recover watchdog churn cause discovery failure, or does it only
  amplify an already-failing scan path?

Comparison:

- recover-before-round reference:
  `logs/wifi_discovery_debug_step65_watchdogdiag_1r_20260305_0852.log`
  - watchdog started (`reason=host_recover`), then terminated after
    `scan_rounds=4`, `zero_scan_rounds=4`, `connect_begins=0`,
    `elapsed_ms=176526`.
- no-recover probe:
  `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_1r_20260305_085746.log`
  - no watchdog start/clear lines.
  - run still failed on `scan_zero_discovery_guard_terminal` with
    `scan_zero=7`, `scan_nonzero=0`, `failure_class=discovery_empty`.

Conclusion:

- watchdog churn is secondary and host-recover-contingent.
- primary root cause remains discovery scan starvation (`scan_nonzero=0`) even
  when watchdog path is not entered.

## 2026-03-05: listener-pressure no-recover pair still all-zero

Setup:

- fixed `recover_before_round=false`, then toggled only listener pressure.

Runs:

- listener on:
  `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_1r_20260305_085746.log`
  - summary: `scan_zero=7`, `scan_nonzero=0`, `failure_class=discovery_empty`.
- listener off:
  `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_listeneroff_1r_20260305_090156.log`
  - summary: `scan_zero=38`, `scan_nonzero=0`, `failure_class=discovery_empty`.
  - repeated `scan_zero_discovery_driver_restart`; watchdog starts with
    `reason=scan_zero_discovery_driver_restart` and records `connect_begins=0`.

Conclusion:

- listener pressure changes failure shape/noise, but not the core outcome.
- primary bottleneck remains scan starvation (`scan_nonzero=0`) in both
  no-recover variants.

## 2026-03-05: fixed stale zero-discovery guard carryover on host recover

Implementation:

- file changed:
  `src/firmware/storage/upload/wifi/connect/prepare/prepare_preconditions.rs`
- `NetControlCommand::Recover` now resets:
  `discovery_sweep_exhausted_streak`,
  `zero_discovery_hard_guard_restarts`,
  `force_full_channel_probe_next_scan`.

Why:

- explicit host recover should start from a clean discovery guard state;
  previous behavior could carry prior guard progress into the next cycle.

Post-flash validation sequence:

- A (no-recover, listener off) terminal run:
  `logs/wifi_discovery_debug_step65_postpatch_seqA_norecover_listeneroff_1r_20260305_091317.log`
  (`scan_zero=40`, `scan_nonzero=0`).
- B (recover-before-round, listener off) immediately after A:
  `logs/wifi_discovery_debug_step65_postpatch_seqB_recover_listeneroff_1r_20260305_091624.log`
  shows fresh `host_recover` watchdog cycle with
  `scan_rounds=1..4` before guard-terminal.

Result:

- stale guard-state carryover is addressed.
- discovery starvation remains primary (`scan_nonzero=0`).

## 2026-03-05: scan-stage lifecycle diagnostics show successful-empty scans

Implementation:

- file changed:
  `src/firmware/storage/upload/wifi/scan.rs`
- added `scan_stage begin/end` diagnostics for each stage with outcome and
  elapsed timing.

Repro run:

- `logs/wifi_discovery_debug_step65_scanstage_diag_1r_20260305_092439.log`
  (`recover_before_round=false`, listener disabled during probe round).

Observed:

- overall round still failed: `scan_zero=44`, `scan_nonzero=0`.
- no `scan_stage ... outcome=timeout`.
- no `scan_stage ... outcome=err`.
- all stage completions were `outcome=ok` with `result_count=0`.
- elapsed profile:
  - `active_broad`: `n=4`, `avg=7877 ms`
  - `active_directed`: `n=4`, `avg=7878 ms`
  - `passive`: `n=4`, `avg=19570 ms`
  - `probe`: `n=32`, `avg=639 ms`

Conclusion:

- discovery-empty is not a scan timeout/error failure in this repro.
- the dominant behavior is successful scan completion with empty results across
  all stages.

## 2026-03-05: post-start settle `2000 ms` probe did not restore discovery

Implementation:

- added compile-time start-settle knob in `src/firmware/storage/upload/wifi.rs`:
  `MEDITAMER_WIFI_POST_START_SETTLE_MS` (default `800`, bounded `100..10000`).

Comparison (`recover_before_round=false`, listener off):

- baseline:
  `logs/wifi_discovery_debug_step65_scanstage_diag_1r_20260305_092439.log`
  (`scan_zero=44`, `scan_nonzero=0`).
- override:
  `logs/wifi_discovery_debug_step65_scanstage_diag_settle2000_1r_20260305_093324.log`
  (`scan_zero=35`, `scan_nonzero=0`).

Observed:

- no `scan_stage end ... outcome=timeout|err` in either run.
- no `scan_stage end ... result_count>0` in either run.
- stage elapsed signatures remained effectively unchanged; only probe-stage count
  reduced (`32` -> `30`) due longer settle overhead inside the bounded round.

Conclusion:

- increasing post-start settle to `2000 ms` did not recover non-zero scan
  results.
- keep default settle `800 ms`; starvation root-cause remains elsewhere.

## 2026-03-05: raw broad scan bypass confirms deeper radio/driver-side starvation

Implementation:

- added optional startup raw broad scan diagnostics:
  - `src/firmware/storage/upload/wifi/driver.rs`:
    `raw_broad_scan_config()`
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`:
    `start_raw_scan_diag` call right after `start_async()`
  - `src/firmware/storage/upload/wifi.rs`:
    compile-time knob `MEDITAMER_WIFI_START_RAW_SCAN_DIAG` (`0` default).

Validation run:

- flashed with `MEDITAMER_WIFI_START_RAW_SCAN_DIAG=1`.
- log:
  `logs/wifi_discovery_debug_step65_rawscan_diag_on_1r_20260305_094620.log`
  (`recover_before_round=false`, listener off).

Observed:

- overall summary still failed: `scan_zero=46`, `scan_nonzero=0`.
- raw broad scan diagnostics:
  - `n=4`, `avg_count=0.00`, `positive_runs=0`.
- staged scan diagnostics in same run:
  - `active_broad`: `n=4`, `avg_count=0.00`
  - `active_directed`: `n=4`, `avg_count=0.00`
  - `passive`: `n=4`, `avg_count=0.00`
  - `probe`: `n=31`, `avg_count=0.00`

Conclusion:

- all-zero discovery is not specific to staged scan configuration.
- primary blocker is deeper radio/driver readiness/state in this environment.

## 2026-03-05: start-readiness timing probe showed scans are not early

Implementation:

- added startup readiness probe knob
  `MEDITAMER_WIFI_START_READINESS_PROBE` (default off), with logs at
  `0/200/800 ms` after `start_ok`, plus `scan_entry_readiness`.

Validation run:

- `logs/wifi_discovery_debug_step65_startreadiness_diag_on_1r_20260305_095649.log`
  (`recover_before_round=false`, listener off, readiness probe on).

Observed:

- probe checkpoints: `started=true` at every sample (`12/12`).
- `scan_entry_readiness`:
  `start_ok_age_ms=1848..1851` (`n=4`, avg `1848.8`).
- staged scan outcomes in same run remained all-zero (`result_count=0`).

Conclusion:

- scans are not entering too early after driver start.
- remaining blocker stays in deeper radio/driver all-zero scan behavior.
