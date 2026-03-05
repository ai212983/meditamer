# Upload Throughput History (Part 13)

## 2026-03-05: strict per-round scan evidence surfaced real control-channel collapse

Change under test:

- strict scan evidence enforcement in discovery workflow:
  `require_scan_evidence_each_round=true`.
- scan evidence source expanded to include `METRICS WIFI` deltas
  (`scan_runs_delta`) in addition to raw `scan_done`/SSID log markers.
- strict profile forced fresh reconnect per round:
  `force_stop_before_round=true`.

Bounded gate run:

- `logs/wifi_regression_gate_20260305_142307/report.json`
  (`soak=0`, strict discovery profile).

Observed:

- rounds `1..7` showed scan evidence, but round `8` hit control-channel
  instability (`NET STOP/RECOVER/LISTENER OFF/START` ack loss) and produced
  no scan evidence.
- discovery summary failed strict evidence gate:
  `scan_evidence_rounds=7` / `8`.
- this replaced prior "ready-only without scan counters" false-pass risk with a
  concrete, actionable failure class.

Conclusion:

- strict scan evidence gating is effective and should stay enabled for bounded
  discovery regression.
- next fix target is host control-channel ack-loss handling (fail-fast + retry
  recovery), not scan-evidence parsing.

## 2026-03-05: host control ack-loss guard restored strict bounded gate pass

Change implemented:

- host discovery runtime now runs guarded control commands for pre-round path:
  `NET STOP` (when enabled), `NET RECOVER`, `NET LISTENER OFF`, `NET START`.
- on first ack loss, host performs short serial recovery (`preflight`,
  conditional `NET RECOVER`) and retries once; unrecovered loss is classified as:
  `HOST_FAILURE class=host_transport_control_ack_loss`.
- this prevents long drift in unstable rounds and makes failures explicit.

Validation:

- short strict probe (`rounds=2`) passed:
  `logs/wifi_discovery_debug_ack_guard_2r_20260305_145518.log`.
- full bounded regression gate passed with strict evidence profile:
  `logs/wifi_regression_gate_20260305_145541/report.json`.

Gate outcome (`run_id=20260305_145541`):

- discovery: pass (`ready_rounds=8`, `scan_evidence_rounds=8`,
  `total_scan_runs_delta=9`).
- acceptance `1` cycle: pass (`throughput_kib_s=128.45`).
- acceptance `3` cycle: pass (`avg_kib_s=131.53`, `upload_metrics_guard` clean).
- soak stage skipped (`HOSTCTL_NET_SOAK_CYCLES=0`).

Conclusion:

- strict discovery evidence and bounded regression gate are both green with the
  new host ack-loss guard.
- this closes the immediate discovery/readiness instability blocker for
  throughput-phase continuation.

## 2026-03-05: transport-reset fast retry hardened upload outlier tail

Problem observed:

- under AP contention, first-attempt direct `PUT /upload` occasionally failed
  with host transport reset (`Connection reset by peer` / receiver-gone class).
- existing retry path waited for health recovery before retry, which preserved
  eventual success but inflated per-cycle tail latency.

Change implemented (host uploader):

- add bounded fast-retry policy for transport-reset class:
  - `HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY=1` (default on)
  - `HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY_STREAK=2` (default)
- behavior:
  - on early reset streaks (`<=2`), rebuild client + short backoff retry
    without long health-recovery wait.
  - retain existing recovery wait path for longer streaks.
- diagnostics now log:
  `transport_reset_streak` and `skip_transport_reset_health_recovery`.

Validation runs:

- pre-change baseline:
  `logs/adhoc_acceptance_diag10_20260305_1529.log` +
  `logs/adhoc_acceptance_diag10_20260305_1529.log.hostdiag`
- post-change fast-retry:
  `logs/adhoc_acceptance_diag10_fastretry_20260305_1533.log` +
  `logs/adhoc_acceptance_diag10_fastretry_20260305_1533.log.hostdiag`
- strict bounded regression gate follow-up (`soak=10`) after merge:
  `logs/wifi_regression_gate_20260305_153302/report.json`

Observed:

- both runs reproduced one transport-reset retry (`attempts=2`) and completed
  all `10` cycles.
- baseline cycle outlier: `cycle 8 upload_ms=21778`.
- fast-retry cycle outlier on reproduced reset: `cycle 4 upload_ms=4505`.
- run summaries:
  - baseline: `avg_upload_s=5.72`, `avg_kib_s=119.98`
  - fast-retry: `avg_upload_s=3.96`, `avg_kib_s=129.64`
- host send diagnostics also tightened spread:
  - baseline `send_ms` stddev `254.4` (`min=3401`, `max=4201`)
  - fast-retry `send_ms` stddev `182.5` (`min=3409`, `max=3931`)
- strict follow-up gate passed all stages (`discovery`, acceptance `1/3`, soak
  `10`) with soak summary `avg_upload_s=3.89`, `avg_kib_s=131.68`.

Conclusion:

- bounded fast-retry materially hardens per-cycle transport-reset tail latency
  without introducing new failure classes in matched 10-cycle diagnostics.
- keep fast-retry enabled by default; continue monitoring for multi-reset
  streaks where fallback recovery wait still applies.

## 2026-03-05: direct-mode repeated-reset now degrades to same-cycle chunked upload

Problem:

- fast retry hardens single-reset outliers, but direct-only mode still had a
  residual failure risk if transport resets repeat beyond retry budget.

Change:

- add repeated-reset degradation guard in host uploader:
  - `HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK=1` (default on)
  - `HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK_STREAK=2` (default)
- when direct `PUT /upload` reset streak exceeds limit, uploader now emits
  marker context (`transport_reset_chunk_fallback_trigger`) and switches in the
  same cycle to chunked upload path:
  `/upload_begin` -> `/upload_chunk` -> `/upload_commit`.
- host logs now emit explicit fallback marker:
  `host_upload_transport_fallback: mode=direct reason=transport_reset_streak ...`

Validation:

- hostctl unit tests passed with new marker detection case:
  `workflows_storage::upload::client::tests`.
- direct-mode `10`-cycle smoke run completed without failures:
  `logs/adhoc_directfallback10_20260305_1602.log`
  and host sidecar:
  `logs/adhoc_directfallback10_20260305_1602.log.hostdiag`.
- this run did not reproduce repeated-reset streaks, so fallback marker did not
  fire live; behavior remains gated to the repeated-reset class.

Conclusion:

- direct mode now has bounded degradation behavior under repeated transport
  resets instead of hard cycle failure.
- next validation focus should be contention-targeted repro that exercises the
  fallback marker path live (for example longer AP-contention soak or forced
  transport-fault injection).
