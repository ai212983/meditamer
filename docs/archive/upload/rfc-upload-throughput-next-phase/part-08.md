## 11.39 2026-03-04 Listener-Readiness Follow-Up Soak (`20` cycles)

Objective:

- extend post-step-58 validation from soak=`10` to soak=`20` under the same
  AP-dense environment and capture whether `listener_not_ready` churn recurs.

Execution:

- initial gate run:
  - `logs/wifi_regression_gate_link_gate_relax_soak20_20260304_193643/report.json`
  - failed at `acceptance_1_cycle` (`failure_class=listener_not_ready`)
- immediate rerun after clean reflash:
  - `logs/wifi_regression_gate_link_gate_relax_soak20_rerun_20260304_194429/report.json`
  - full pass across `discovery_debug`, `acceptance_1_cycle`,
    `acceptance_3_cycle`, `acceptance_soak(20)`

Passing rerun summary:

- host summary (`acceptance_soak`): `avg_upload_s=3.50`, `avg_kib_s=146.65`
- firmware upload stats (`n=20`):
  - `read_wait_ms avg=2368.6`, `p95=2612`, `max=3075`
  - `ingress_read_wait_empty_q_ms avg=2363.1`, `p95=2607`, `max=3070`

Decision:

- keep step-58 host+firmware listener-readiness hardening as current default.
- treat this as bounded soak=`20` closure evidence (with one transient failed
  first run preserved in artifacts).

## 11.40 2026-03-04 Host `TCP_NODELAY` Recheck (Direct Path A/B; No Promotion)

Objective:

- retest `HOSTCTL_UPLOAD_TCP_NODELAY` after the latest readiness/discovery
  fixes to ensure earlier conclusions still hold on current baseline.

Profile:

- `HOSTCTL_UPLOAD_MODE=direct`
- `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
- `HOSTCTL_NET_CYCLES=10`

Runs:

- ON (default): `logs/wifi_nodelay_ab_on_direct10_20260304_1959.log`
  - host summary: `avg_upload_s=3.57`, `avg_kib_s=143.43`
  - upload stats: `req_ms avg=3109.9`, `p95=3248`
  - upload stats: `read_wait_ms avg=2420.8`,
    `ingress_read_wait_empty_q_ms avg=2415.1`
- OFF: `logs/wifi_nodelay_ab_off_direct10_20260304_2000.log`
  - host summary: `avg_upload_s=3.60`, `avg_kib_s=142.54`
  - upload stats: `req_ms avg=3103.3`, `p95=3235`
  - upload stats: `read_wait_ms avg=2416.5`,
    `ingress_read_wait_empty_q_ms avg=2411.3`

Interpretation:

- results are effectively near-parity; disabling `TCP_NODELAY` does not provide
  a meaningful ingress wait or throughput gain.
- both runs remained guard-clean (`req_read_body_reset delta=0`).

Decision:

- keep `HOSTCTL_UPLOAD_TCP_NODELAY=1` default unchanged.
- no promotion/change for this knob.

Specific next optimization target:

- focus on ingress empty-queue tail hardening (variance, not mean):
  add upload-level wait-tail histogram telemetry and drive the next A/B on
  `req_ms p95/p99` reduction under `20`-cycle AP-dense runs.

## 11.41 2026-03-04 Ingress Wait-Tail Histogram Telemetry (Implemented)

Objective:

- make ingress-outlier shape explicit in per-upload telemetry before
  outlier-focused tuning runs.

Firmware change:

- file: `src/firmware/storage/upload/http/connection/body.rs`
- added new `upload_http: upload stats` fields:
  - `ingress_read_wait_over_100ms`
  - `ingress_read_wait_empty_q_over_10ms`
  - `ingress_read_wait_empty_q_over_50ms`
  - `ingress_read_wait_empty_q_over_100ms`
  - `ingress_read_wait_empty_q_max_ms`
  - `ingress_read_empty_streak_ms_max`

Validation:

- build: `scripts/build/build.sh debug` passed.
- flashed: `scripts/device/flash.sh debug` on `/dev/cu.usbserial-510`.
- sanity run:
  - `logs/wifi_ingresstailtelemetry_sanity1_postflash_20260304_2001.log`
  - verified new fields are present in `upload_http: upload stats`.

Next:

- run `20`-cycle outlier-focused validation and tune using the new telemetry,
  targeting `req_ms p95/p99` reduction without mean-throughput loss.

## 11.42 2026-03-04 Outlier Baseline with New Tail Telemetry (`20` cycles)

Profile:

- `HOSTCTL_UPLOAD_MODE=direct`
- `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
- `HOSTCTL_NET_CYCLES=20`
- log: `logs/wifi_ingresstailbaseline20_direct_20260304_2002.log`

Host summary:

- `avg_upload_s=3.57`, `avg_kib_s=143.64`
- all cycles guard-clean (`req_read_body_reset delta=0`)

Upload stats tail baseline (`n=20`):

- `req_ms avg=3095.8`, `p95=3283`, `max=3377`
- `read_wait_ms avg=2403.7`, `p95=2586`
- `ingress_read_wait_empty_q_ms avg=2398.4`, `p95=2584`
- new tail fields:
  - `ingress_read_wait_over_100ms avg=7.3`
  - `ingress_read_wait_empty_q_max_ms avg=165.2`, `p95=194`
  - `ingress_read_empty_streak_ms_max avg=366.8`, `p95=466`, `max=533`

Interpretation:

- ingress empty-queue wait remains dominant and tail-heavy even when retries are
  absent; tuning should target tail compression (`p95/p99`) rather than mean.

Next:

- run threshold-tuning A/B on ingress fairness (`bytes/reads`) and compare
  against this baseline for tail reduction with throughput parity.

Continuation for sections `11.43+` moved to:

- `docs/development/rfc-upload-throughput-next-phase/part-09.md`
