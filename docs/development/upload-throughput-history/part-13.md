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
