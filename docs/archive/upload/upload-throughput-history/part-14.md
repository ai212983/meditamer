# Upload Throughput History (Part 14)

## 2026-03-05: acceptance `net_wait_ready` bounded recover-retry hardening

Problem:

- acceptance startup still showed recurring pre-upload readiness collapse in
  AP-dense conditions:
  `ListenerWait` + `link=true` + `ipv4=0.0.0.0` ending in
  `failure_class=listener_not_ready`.
- prior startup hardening (step 70) improved first-stage recovery but did not
  remove this failure mode in all runs.

Change:

- host acceptance `net_wait_ready` now supports bounded recover-retry on
  retryable readiness failures (default `HOSTCTL_NET_WAIT_READY_RECOVER_RETRIES=1`).
- retryable signatures include listener-timeout / listener-not-ready and DHCP
  no-ipv4 stall classes; panic/other hard failures are still terminal.
- implementation:
  - `tools/hostctl/src/workflows_wifi_acceptance/runtime_core.rs`
    - add bounded `wait_ready` retry loop with `handle_net_recover_once()` +
      `handle_net_start()` before final fail.
    - add helper classifier `should_retry_wait_ready_after_recover(...)`.
    - add unit tests for retryable vs non-retryable classification.

Validation:

- quick bounded live run with reduced policy envelope (single cycle):
  - stdout artifact:
    `logs/wifi_acceptance_waitretry_quick_20260305_173625.stdout.log`
  - serial artifact:
    `logs/wifi_acceptance_waitretry_quick_20260305_173625.log`
- host confirmed retry path execution:
  - `net_wait_ready: retryable readiness failure; recover retry 1/1 ...`
- after retry, failure still reproduced:
  - terminal `error: network failure class=listener_not_ready ... state=Some("Failed") ...`
  - serial confirms repeated `DhcpWait/ListenerWait` with `ipv4=0.0.0.0` then
    `NET_EVENT ... trigger="attempt_budget_exhausted"` and terminal failed
    status.

Conclusion:

- host-side readiness handling is now more robust and deterministic (one bounded
  recovery attempt before fail), reducing single-shot startup false negatives.
- primary blocker remains firmware-side DHCP/listener lease-path behavior under
  AP contention, not host retry policy.

## 2026-03-05: firmware lease-sync instrumentation validated in failing window

Problem:

- logs repeatedly show `DhcpWait/ListenerWait` with `ipv4=0.0.0.0` even when
  transition trigger is `dhcp_ready`, making it unclear whether failure is true
  lease loss or status/telemetry drift.

Change:

- in connected progression, telemetry listener snapshot now reuses live stack
  lease IP while listener gate is enabled:
  - file:
    `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
  - behavior:
    `telemetry::set_upload_http_listener(snapshot.upload_http_listening, lease_ipv4)`
    in listener-enabled path.
- added `dhcp_ready` precondition diagnostic line (lease IP vs telemetry IP) at
  transition into `ListenerWait`.
- compile check:
  - `cargo check` passed.

Validation:

- firmware flashed successfully (`scripts/device/flash.sh debug`,
  `ESPFLASH_PORT=/dev/cu.usbserial-510`) and discovery-path validation captured:
  - `logs/wifi_discovery_leasecheck_postflash_1r_20260305_175254.log`
  - includes marker:
    `upload_http: dhcp_ready lease_ipv4=192.168.114.105 telemetry_ipv4=0.0.0.0 listener_enabled=false`
- strict failing-window repro captured:
  - `logs/wifi_regression_gate_20260305_175630/acceptance_1_cycle.log`
  - repeated transition marker:
    `upload_http: dhcp_ready lease_ipv4=192.168.114.105 telemetry_ipv4=0.0.0.0 listener_enabled=true`
  - repeated `ListenerWait` with preserved lease:
    `NET_STATUS {"state":"ListenerWait","link":true,"ipv4":"192.168.114.105","listener":false,...}`
  - repeated `ListenerWait -> Recovering` via `listener_timeout`.

Conclusion:

- this failure shape is not a true lease-loss collapse.
- dominant blocker is listener-arm progression stall while lease is valid.

## 2026-03-05: listener-gate stall diagnostics added for next strict run

Problem:

- existing logs showed lease-ready `ListenerWait` stall but did not classify
  whether HTTP server was paused by transfer-phase gating or by DHCP gate churn
  (`wifi_down/link_down/no_ipv4`) immediately before timeout.

Change:

- instrument HTTP server loop gate transitions:
  - `src/firmware/storage/upload/http/server_loop.rs`
  - new diagnostics:
    - `upload_http: transfers_paused ...`
    - `upload_http: transfers_resumed ...`
    - `upload_http: listener_gate reason=...`
    - `upload_http: listener_gate clear reason=... wait_ms=...`
- helper added:
  - `src/firmware/storage/upload/http/diagnostics.rs`
    - `net_pipeline_gate_reason_str(...)`.

Next:

- rerun strict acceptance/gate on flashed firmware and classify dominant
  pre-timeout gate reason from these traces.
- strict soak-skipped gate rerun executed on flashed diagnostics build:
  - `logs/wifi_regression_gate_20260305_180911/report.json` (all stages passed).
  - traces confirmed instrumentation works (`transfers_paused/resumed`,
    `listener_gate reason=wifi_down`).
  - no listener-timeout acceptance failure reproduced in this pass, so failure
    class attribution remains open for the next failing-window capture.
- timeboxed strict 3-run follow-up:
  - run1 (`logs/wifi_regression_gate_20260305_182227`) reproduced heavy
    discovery churn under AP contention (`reason=2`/`reason=201`) with deep
    internal-free dip (`internal_free` low-water observed down to `752`) and was
    terminated on timebox before report emit.
  - run2 and run3 passed full strict gate:
    - `logs/wifi_regression_gate_20260305_182902/report.json`
    - `logs/wifi_regression_gate_20260305_183347/report.json`
  - across completed runs, no `listener_not_ready` acceptance recurrence was
    captured.

## 2026-03-05: listener re-arm fix via accept gate-aware polling

Problem:

- strict failing window reproduced persistent acceptance listener churn:
  `ListenerWait -> listener_timeout` repeats with valid lease IP and eventual
  `listener_not_ready` terminal state.
- failing artifact:
  - `logs/wifi_regression_batch_20260305_185110/run_1/acceptance_1_cycle.log`
  - counts from this trace:
    - `listener_timeout` events: `15`
    - `failure_class="listener_not_ready"` entries: `20`.

Change:

- firmware HTTP accept path now uses bounded poll + gate rechecks:
  - file: `src/firmware/storage/upload/http/socket_cycle.rs`
  - behavior:
    - poll `accept()` in `500 ms` windows.
    - re-check transfer/listener/connectivity gates between polls.
    - abort stale accept socket and re-arm cleanly when gates drop.

Validation:

- compile + flash:
  - `cargo check` passed.
  - flashed with `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/device/flash.sh debug`.
- strict regression reruns (soak skipped):
  - `logs/wifi_regression_gate_20260305_190833/report.json` (`passed`)
  - `logs/wifi_regression_gate_20260305_191234/report.json` (`passed`)
- post-fix acceptance logs:
  - `logs/wifi_regression_gate_20260305_190833/acceptance_1_cycle.log`
  - `logs/wifi_regression_gate_20260305_190833/acceptance_3_cycle.log`
  - `logs/wifi_regression_gate_20260305_191234/acceptance_1_cycle.log`
  - `logs/wifi_regression_gate_20260305_191234/acceptance_3_cycle.log`
  - all show:
    - `listener_timeout` events: `0`
    - `failure_class="listener_not_ready"` entries: `0`.

Conclusion:

- reproduced listener stall class is mitigated in bounded strict gates.
- keep open for AP-contention confirmation batch before declaring full closure.

## 2026-03-05: listener-timeout loop hardening for internal-memory drift control

Problem:

- even before terminal fail, failing listener runs showed repeated timeout loops
  with monotonic internal-free erosion:
  - `internal_free` `15188 -> 12788` (`-2400 B`)
  - `used` `186540 -> 188940` (`+2400 B`)
  - in:
    `logs/wifi_regression_batch_20260305_185110/run_1/acceptance_1_cycle.log`

Change:

- add listener-timeout guard in connected-progress recovery:
  - streak threshold: `6`
  - internal-free drop threshold: `1024 B`
- when either threshold trips:
  - escalate from disconnect-only retry to stop/start hard recover
  - clear candidate/hint/auth progression state
  - restart hard-recover watchdog (`listener_timeout_hard_recover`).
- code files:
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
  - `src/firmware/storage/upload/wifi/connect/success.rs`
  - `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
  - `src/firmware/storage/upload/wifi/connect/success/success_recovery.rs`

Validation:

- `cargo check` passed.
- flashed firmware and ran strict gate (soak skipped):
  - `logs/wifi_regression_gate_20260305_193136/report.json` (`passed`).
- acceptance logs for this run show:
  - `listener_timeout` events: `0`
  - `listener_not_ready` entries: `0`
  - guard marker absent (healthy-run path, expected).

Conclusion:

- hardening is landed and non-regressive in strict gate.
- next required evidence is AP-contention rerun that triggers guard path to
  confirm containment behavior under real churn.

## 2026-03-05: AP-contention confirmation batch after listener hardening

Objective:

- validate listener/readiness tail stability after:
  - accept gate-aware polling re-arm
  - listener-timeout streak/internal-free-drop hard-recover guard.

Run set:

- batch:
  - `logs/wifi_apcontention_confirm_20260305_193824`
- strict profile, soak skipped, three runs.
- summary:
  - `logs/wifi_apcontention_confirm_20260305_193824/summary.tsv`

Results (run1/run2/run3):

- all `final_status=passed`.
- listener failure tails:
  - `listener_timeout=0` (all)
  - `listener_not_ready=0` (all)
  - `accept_link_reset=0` (all)
- listener-wait internal-free drift:
  - `lw_drift_a1=0`
  - `lw_drift_a3=0`
  for all three runs.

Interpretation:

- no recurrence of the previously dominant listener-timeout loop class in this
  AP-contention confirmation batch.
- mitigation promoted as hardened for this failure class.
- guard trigger path remained unexercised in this healthy batch and is tracked
  as a non-blocking follow-up validation item.


_Continued in [Part 14, continuation 2](./part-14-02.md)._
