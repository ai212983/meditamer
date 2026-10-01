# RFC: Upload Throughput Next Phase (Part 12)

## 12.1 2026-03-05 Acceptance `net_wait_ready` Recover-Retry Hardening

Objective:

- reduce pre-upload false-abort risk in acceptance when readiness stalls
  transiently (`ListenerWait`/`DhcpWait` with `ipv4=0.0.0.0`) by allowing one
  bounded host recover-retry before terminal fail.

Implementation:

- file:
  - `tools/hostctl/src/workflows_wifi_acceptance/runtime_core.rs`
    - `handle_net_wait_ready(...)` now loops with bounded retries controlled by
      `HOSTCTL_NET_WAIT_READY_RECOVER_RETRIES` (default `1`).
    - on retryable readiness errors, host executes:
      - `handle_net_recover_once()`
      - `handle_net_start()`
      - then re-enters `wait_ready(...)`.
    - retry classifier:
      `should_retry_wait_ready_after_recover(...)`
      (listener-not-ready / listener-timeout / dhcp-no-ipv4 signatures only).
    - add unit tests for retryable/non-retryable classification.

Validation:

- live quick-bounded acceptance run:
  - `logs/wifi_acceptance_waitretry_quick_20260305_173625.stdout.log`
  - `logs/wifi_acceptance_waitretry_quick_20260305_173625.log`
- observed host retry marker:
  - `net_wait_ready: retryable readiness failure; recover retry 1/1 ...`
- final result remained terminal `listener_not_ready` after retry.

Interpretation:

- hardening is effective for control-path resilience (single bounded retry now
  deterministic and classed), but it does not resolve the dominant underlying
  firmware-side readiness collapse.

Next focus:

- firmware DHCP/listener lease-path root cause under AP contention:
  - why `dhcp_ready` transitions still expose `ipv4=0.0.0.0` in status and
    remain stuck through listener wait.
  - add targeted firmware instrumentation around lease assignment + listener arm
    preconditions (attempt-scoped timestamps and lease validity snapshots).

## 12.2 2026-03-05 Lease-Sync Instrumentation for `NET_STATUS` Consistency

Objective:

- remove ambiguity between true DHCP lease loss vs telemetry/status drift by
  synchronizing `NET_STATUS.ipv4` with live stack lease during connected
  progression.

Implementation (firmware code landed):

- file:
  - `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
    - synchronize telemetry listener snapshot with live lease:
      `telemetry::set_upload_http_listener(snapshot.upload_http_listening, lease_ipv4)`
      while listener gate is enabled.
    - add `dhcp_ready` diagnostic precondition line (lease IP vs telemetry IP
      snapshot) on transition into `ListenerWait`.
- compile verification:
  - `cargo check` passed in workspace.

Validation:

- flashed and discovery-path validated:
  - `logs/wifi_discovery_leasecheck_postflash_1r_20260305_175254.log`
  - shows diagnostic marker:
    `upload_http: dhcp_ready lease_ipv4=192.168.114.105 telemetry_ipv4=0.0.0.0 listener_enabled=false`
- strict post-flash acceptance repro captured failing listener-enabled window:
  - `logs/wifi_regression_gate_20260305_175630/acceptance_1_cycle.log`
  - repeated `dhcp_ready` transitions while `listener_enabled=true`:
    `upload_http: dhcp_ready lease_ipv4=192.168.114.105 telemetry_ipv4=0.0.0.0 listener_enabled=true`
  - subsequent `ListenerWait` statuses preserve non-zero lease IP:
    `NET_STATUS {"state":"ListenerWait","link":true,"ipv4":"192.168.114.105","listener":false,...}`
  - terminal transition remains listener timeout path.

Interpretation:

- lease-loss is no longer the dominant explanation for this failure shape.
- dominant open class is listener-arm stall while lease is already valid.

## 12.3 2026-03-05 Listener-Gate Stall Diagnostics (HTTP Server Loop)

Objective:

- classify why listener arming remains false during `ListenerWait` despite
  lease-ready state.

Implementation:

- file:
  - `src/firmware/storage/upload/http/server_loop.rs`
    - add `transfers_paused`/`transfers_resumed` diagnostics with app-state
      context (`phase`, `diag_kind`, `diag_targets`, `upload_enabled`,
      listener gate seq).
    - add `listener_gate reason=<wifi_down|link_down|no_ipv4>` transition logs
      with `wifi_connected`, `link_up`, `config_ipv4`, listener gate state.
    - add `listener_gate clear ... wait_ms=...` log when DHCP gate resolves.
- helper:
  - `src/firmware/storage/upload/http/diagnostics.rs`
    - `net_pipeline_gate_reason_str(...)`.

Next validation:

- run strict gate with this firmware and confirm which gate reason dominates
  immediately before `listener_timeout` in failing rounds.
- strict soak-skipped gate rerun completed on flashed build:
  - `logs/wifi_regression_gate_20260305_180911/report.json` (`final_status=passed`)
  - diagnostics emitted as expected:
    - `upload_http: transfers_paused ...`
    - `upload_http: listener_gate reason=wifi_down ...`
    - `upload_http: transfers_resumed ...`
  - in this run the failure window did not reproduce in acceptance (`1` and `3`
    cycle stages passed), so dominant pre-timeout reason classification remains
    open pending a failing round.
- timeboxed strict 3-run follow-up:
  - run1 (`logs/wifi_regression_gate_20260305_182227`) entered prolonged
    AP-contention discovery churn (`reason=2`/`reason=201`) and was terminated
    on timebox before report emit; no listener-timeout acceptance stage reached.
  - run2 and run3 both passed full strict gate:
    - `logs/wifi_regression_gate_20260305_182902/report.json`
    - `logs/wifi_regression_gate_20260305_183347/report.json`
  - no `listener_not_ready` recurrence captured in passed acceptance stages.

## 12.4 2026-03-05 Listener Re-Arm Mitigation (Accept Gate-Aware Polling)

Objective:

- eliminate repeated `ListenerWait -> listener_timeout` loops caused by stalled
  listener re-arm under reconnect churn.

Failure capture used for root-cause:

- strict batch run captured sustained acceptance churn:
  - `logs/wifi_regression_batch_20260305_185110/run_1/acceptance_1_cycle.log`
- observed pattern:
  - repeated `dhcp_ready lease_ipv4=192.168.114.105 ... listener_enabled=true`
  - repeated `ListenerWait -> Recovering trigger=listener_timeout`
  - terminal `failure_class="listener_not_ready"` after attempt budget.

Implementation:

- file:
  - `src/firmware/storage/upload/http/socket_cycle.rs`
- change:
  - replace monolithic blocking `accept()` wait with bounded poll loop
    (`500 ms`) that re-checks listener/transfer/connectivity gates.
  - on gate-loss (`transfers disabled`, listener gate disabled, or
    DHCP/connectivity gate loss), abort current accept socket and return to
    outer loop for clean re-arm.
  - preserve existing accept success/error telemetry paths.

Validation:

- build + flash:
  - `cargo check` passed.
  - flashed with `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/device/flash.sh debug`.
- strict gate reruns (soak skipped) on patched firmware:
  - `logs/wifi_regression_gate_20260305_190833/report.json` (`passed`)
  - `logs/wifi_regression_gate_20260305_191234/report.json` (`passed`)
- acceptance-log comparison:
  - pre-fix failing run:
    - `listener_timeout` events: `15`
    - `failure_class="listener_not_ready"` entries: `20`
    - file: `logs/wifi_regression_batch_20260305_185110/run_1/acceptance_1_cycle.log`
  - post-fix strict gates:
    - `listener_timeout` events: `0` across all acceptance stage logs
    - `listener_not_ready` entries: `0` across all acceptance stage logs
    - files:
      - `logs/wifi_regression_gate_20260305_190833/acceptance_1_cycle.log`
      - `logs/wifi_regression_gate_20260305_190833/acceptance_3_cycle.log`
      - `logs/wifi_regression_gate_20260305_191234/acceptance_1_cycle.log`
      - `logs/wifi_regression_gate_20260305_191234/acceptance_3_cycle.log`

Interpretation:

- mitigation addresses the reproduced listener re-arm stall class in strict
  bounded gates.
- next step is broader AP-contention confirmation to harden closure confidence.

## 12.5 2026-03-05 Listener Timeout Loop Hardening (Streak + Internal-Free Guard)

Objective:

- reduce prolonged listener-timeout retry churn and cap per-iteration internal
  memory drift before terminal attempt-budget exhaustion.

Implementation:

- files:
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
    - add task-state guard fields:
      - `listener_timeout_streak`
      - `listener_timeout_streak_start_internal_free`
  - `src/firmware/storage/upload/wifi/connect/success.rs`
    - add listener-timeout guard logic:
      - hard-recover threshold by streak (`6`)
      - hard-recover threshold by internal-free drop (`1024 B`)
    - on hard-recover trigger:
      - `disconnect_and_stop_with_timeout(...)`
      - clear candidate/hint/auth progression fields
      - restart post-hard-recover watchdog with
        `listener_timeout_hard_recover`
    - reset guard on healthy/non-listener-timeout transitions.
  - `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
    - wire listener-timeout branch to the new guarded recovery helper.
  - `src/firmware/storage/upload/wifi/connect/success/success_recovery.rs`
    - split DHCP no-IPv4 recovery helper to keep file-size policy bounded.

Validation:

- compile + flash:
  - `cargo check` passed.
  - flashed with `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/device/flash.sh debug`.
- strict gate (soak skipped):
  - `logs/wifi_regression_gate_20260305_193136/report.json` (`passed`)
  - acceptance logs:
    - `listener_timeout` events: `0`
    - `listener_not_ready` entries: `0`
    - guard marker (`listener_timeout guard streak=...`) did not appear in this
      healthy run (expected; path is failure-containment logic).

Interpretation:

- hardening is in place and non-regressive on first strict validation.
- AP-contention confirmation remains open to exercise and quantify guard-path
  behavior under failing conditions.

## 12.6 2026-03-05 AP-Contention Confirmation Batch (Step 74 Closure)

Objective:

- close step-74 validation by stress-checking listener stability tails under the
  strict profile after listener re-arm + loop hardening.

Validation run:

- batch root:
  - `logs/wifi_apcontention_confirm_20260305_193824`
- runs:
  - `run_1`: `report.json` passed
  - `run_2`: `report.json` passed
  - `run_3`: `report.json` passed
- summary artifact:
  - `logs/wifi_apcontention_confirm_20260305_193824/summary.tsv`

Observed tails (all runs):

- `listener_timeout=0`
- `listener_not_ready=0`
- `accept_link_reset=0`
- listener-wait internal-free drift:
  - `lw_drift_a1=0`
  - `lw_drift_a3=0`
- guard marker (`listener_timeout guard streak=...`) did not appear in this
  healthy batch.

Decision:

- step 74 is closed as hardened for the listener-timeout failure class.
- keep guard trigger-path validation as non-blocking follow-up, not a gate
  blocker, because the batch did not enter failure churn.
