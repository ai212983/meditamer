## 2026-03-03: FAT append diagnostics + 3x soak correlation

Instrumentation commit:

- `64f6da6`: `feat(upload): add fat append chunk timing diagnostics`
- adds `sd_upload: write_metrics` chunk-boundary fields:
  - `chunk_total_ms_*`, `chunk_ensure_ready_ms_*`, `chunk_payload_lock_ms_*`
  - `chunk_append_ms_*`, `chunk_append_capacity_ms_*`,
    `chunk_append_write_data_ms_*`
  - `chunk_overhead_ms_*`, plus `chunk_total_over_200ms/_over_400ms` and
    `chunk_append_over_200ms/_over_400ms`

3x `36 MHz` bounded soak artifacts:

- `logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`
- `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`
- `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`

Gate status:

- all three runs passed every stage (`discovery_debug`, `acceptance_1_cycle`,
  `acceptance_3_cycle`, `acceptance_soak`).

Correlation summary:

- `req_ms > 3400`: `0/30`.
- `chunk_max_ms > 400`: `5/30` (`420`, `629`, `477`, `449`, `407`).
- append-path timing stayed tight across all runs:
  - `chunk_append_ms_avg`: `126.7..127.4 ms`
  - `chunk_append_capacity_ms_avg`: `38.0..38.5 ms`
  - `chunk_append_write_data_ms_avg`: `87.6..87.9 ms`
  - observed `chunk_append_ms_max` ceiling: `145 ms`
- representative outlier pair:
  - upload with `chunk_max_ms=629` had `chunk_append_ms_max=134`.
- interpretation:
  - current `chunk_max_ms` upper tail is not primarily caused by
    `fat::append_session_write` execution time.
  - residual wait outside append remains material (`sd_task_ms/chunk -
    chunk_append_ms_avg` roughly `150..191 ms` in these runs).

## 2026-03-03: queue-boundary diagnostics + 3x soak correlation

Instrumentation commit:

- `e85f2a7`: `feat(upload): add chunk queue-boundary residual diagnostics`
- key additions:
  - `SdUploadRequest.enqueued_at_ms`
  - per-chunk response timings: `chunk_queue_wait_ms`, `chunk_handler_ms`
  - `upload_http: upload stats` fields:
    - `sd_task_queue_wait_ms`
    - `sd_task_handler_ms`
    - `sd_task_residual_ms`
  - `sd_upload: write_metrics` fields:
    - `chunk_queue_wait_ms_*`
    - `chunk_non_append_ms_*`
    - `chunk_residual_ms_*`

Runs:

- selected:
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`
- excluded:
  - `logs/wifi_regression_gate_sdspi36_queuebridge_r2_20260303_170602`
    (`acceptance_1_cycle` failed with `net_wait_ready: listener timeout`)

Selected-set summary (`n=30` uploads):

- `req_ms avg=3060.8`, range `2933..3752`, `req_ms > 3400`: `1/30`
- `chunk_max_ms avg=364.7`, range `318..666`, `chunk_max_ms > 400`: `6/30`
- `chunk_append_ms_avg` stayed stable at `126.2 ms`
- queue/handler/residual decomposition (`upload_http: upload stats`):
  - `sd_task_queue_wait_ms avg=47.0`
  - `sd_task_handler_ms avg=1017.6`
  - `sd_task_residual_ms avg=1239.0`
- high-tail sample:
  - `chunk_max_ms=666` with `sd_task_handler_ms=1027`,
    `sd_task_residual_ms=1845`

Interpretation:

- queue wait is present but not dominant.
- handler time is stable and consistent with FAT append timings.
- the dominant unexplained component is post-handler residual wait
  (`sd_task_residual_ms`).

## 2026-03-03: post-handler residual split instrumentation

Implemented split-residual instrumentation in firmware upload path:

- SD task now stamps chunk handler completion and publish edge timing.
- SD bridge stamps receive edge and computes publish-to-receive delay.
- `upload_http: upload stats` now emits:
  - `sd_task_post_handler_ms`
  - `sd_task_publish_to_receive_ms`
  - `sd_task_residual_other_ms`
- existing `sd_task_residual_ms` is preserved for continuity and now decomposes
  into:
  - `sd_task_post_handler_ms`
  - `sd_task_publish_to_receive_ms`
  - `sd_task_residual_other_ms`

Smoke verification:

- run: `logs/wifi_acceptance_split_residual_smoke_20260303_173818.log`
- sample upload stats:
  - `sd_task_residual_ms=1291`
  - `sd_task_post_handler_ms=1`
  - `sd_task_publish_to_receive_ms=1290`
  - `sd_task_residual_other_ms=0`
- interpretation:
  - in this sample, residual is dominated by publish-to-receive delay rather
    than SD-task post-handler pre-publish delay.

Next step:

- run bounded 3x `36 MHz` regression gates with split-residual instrumentation.
- correlate `chunk_max_ms > 400` uploads against the new split fields to
  identify which post-handler leg dominates.

## 2026-03-03: split-residual correlation (3x bounded soak)

Runs used:

- `logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`
- `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`
- `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`

Notes:

- full regression-gate attempts in the same window hit non-upload
  acceptance-stage failures (listener/boot-discovery gating), so split-residual
  correlation used direct bounded soak acceptance runs.
- one `upload_http: upload stats` line in `r3` was serial-concatenated and was
  excluded from aggregate parsing.

Correlation summary (`upload_http: upload stats`, valid `n=29`):

- `req_ms avg=3135.1`
- `chunk_max_ms avg=401.4`
- `chunk_max_ms > 400`: `9/29`
- decomposition means:
  - `sd_task_queue_wait_ms avg=43.7`
  - `sd_task_handler_ms avg=1035.4`
  - `sd_task_residual_ms avg=1259.9`
- residual split means:
  - `sd_task_post_handler_ms avg=0.4`
  - `sd_task_publish_to_receive_ms avg=1259.5`
  - `sd_task_residual_other_ms avg=0.0`
- outlier (`chunk_max_ms > 400`) residual split:
  - `sd_task_residual_ms avg=1429.2`
  - `sd_task_publish_to_receive_ms avg=1429.0`
  - `sd_task_post_handler_ms avg=0.2`
  - `sd_task_residual_other_ms avg=0.0`

Interpretation:

- split confirms post-handler residual is not SD-task post-handler delay.
- residual is almost entirely bridge publish-to-receive delay in this campaign.

Next root-cause focus:

- investigate bridge receive cadence during pipelined body ingest to determine
  how much of publish-to-receive is expected overlap accounting versus avoidable
  receive lag affecting request tail behavior.

## 2026-03-03: bridge non-blocking inflight drain mitigation

Mitigation implemented:

- bridge now tries to drain completed inflight SD chunk results non-blockingly
  during body ingest (between socket reads), instead of waiting only at explicit
  queue-boundary flush points.

Validation runs:

- `logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`
- `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`

Aggregate comparison (pre-fix split-residual set vs post-fix trydrain set):

- pre-fix (`n=29`):
  - `req_ms avg=3135.1`
  - `chunk_max_ms avg=401.4`, `chunk_max_ms > 400`: `9`
  - `sd_task_residual_ms avg=1259.9`
  - `sd_task_publish_to_receive_ms avg=1259.5`
- post-fix (`n=20`):
  - `req_ms avg=3175.6`
  - `chunk_max_ms avg=172.4`, `chunk_max_ms > 400`: `0`
  - `sd_task_residual_ms avg=40.0`
  - `sd_task_publish_to_receive_ms avg=39.7`

Interpretation:

- dominant residual leg (`publish_to_receive`) drops by ~`96.8%` in this sample.
- chunk roundtrip tail (`chunk_max_ms`) collapses accordingly.
- request-time mean remained in the same multi-second band; requires further
  gate-scale validation before concluding net throughput benefit.

Next step:

- run full 1/3/soak regression gates on this mitigation and confirm reliability
  plus request-time behavior under bounded soak.

