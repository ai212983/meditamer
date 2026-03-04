## 11.11 2026-03-03 FAT Append Diagnostics + 3x Soak Correlation

- Instrumentation commit:
  - `64f6da6` (`feat(upload): add fat append chunk timing diagnostics`)
  - new `sd_upload: write_metrics` fields for SD-task chunk boundaries:
    - `chunk_total_ms_*`, `chunk_ensure_ready_ms_*`,
      `chunk_payload_lock_ms_*`
    - `chunk_append_ms_*`, `chunk_append_capacity_ms_*`,
      `chunk_append_write_data_ms_*`
    - `chunk_overhead_ms_*` and outlier counters (`*_over_200ms`, `*_over_400ms`)

- 3x `36 MHz` bounded soak runs:
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`
  - all three passed full gate (discovery, `1-cycle`, `3-cycle`, soak=10).

- Correlation result:
  - `req_ms > 3400`: `0/30`.
  - `chunk_max_ms > 400`: `5/30` samples (`420`, `629`, `477`, `449`, `407`).
  - append-path timings stayed tight:
    - `chunk_append_ms_avg` per upload: `126.7..127.4 ms`
    - `chunk_append_capacity_ms_avg`: `38.0..38.5 ms`
    - `chunk_append_write_data_ms_avg`: `87.6..87.9 ms`
    - `chunk_append_ms_max` observed ceiling: `145 ms`
  - outlier example:
    - `chunk_max_ms=629` while same-upload `chunk_append_ms_max=134`.
  - conclusion: current upper-tail `chunk_max_ms` spread is not dominated by
    `fat::append_session_write` execution time.

## 11.12 2026-03-03 Queue-Boundary Diagnostics + 3x Soak Correlation

- Instrumentation commit:
  - `e85f2a7` (`feat(upload): add chunk queue-boundary residual diagnostics`)
  - `SdUploadRequest` now carries `enqueued_at_ms`; chunk responses include
    `chunk_queue_wait_ms` and `chunk_handler_ms`.
  - `upload_http: upload stats` now emits:
    - `sd_task_queue_wait_ms`
    - `sd_task_handler_ms`
    - `sd_task_residual_ms`
  - `sd_upload: write_metrics` now emits queue/residual fields:
    - `chunk_queue_wait_ms_*`
    - `chunk_non_append_ms_*`
    - `chunk_residual_ms_*`

- 3x `36 MHz` bounded soak runs:
  - selected set:
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`
  - excluded run:
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r2_20260303_170602`
      (`acceptance_1_cycle` failed with `net_wait_ready: listener timeout`).

- Correlation result (selected 30 uploads):
  - `req_ms avg=3060.8`, range `2933..3752`, `req_ms > 3400`: `1/30`.
  - `chunk_max_ms avg=364.7`, range `318..666`, `chunk_max_ms > 400`: `6/30`.
  - `chunk_append_ms_avg` remained stable at `126.2 ms`.
  - queue/handler decomposition:
    - `sd_task_queue_wait_ms avg=47.0 ms`
    - `sd_task_handler_ms avg=1017.6 ms`
    - `sd_task_residual_ms avg=1239.0 ms`
  - high-`chunk_max_ms` samples aligned with elevated residual, not handler growth:
    - example: `chunk_max_ms=666` with `sd_task_handler_ms=1027`,
      `sd_task_residual_ms=1845`.
  - conclusion: current upper tail is not dominated by queue wait or handler
    execution; post-handler residual wait remains the dominant unexplained term.

## 11.13 Next Step (Specific)

- Shift root-cause focus to post-handler residual path:
  - stamp chunk completion-to-response timing explicitly (publish edge in SD task
    and receive edge in SD bridge) to split `sd_task_residual_ms` into:
    - SD-task post-handler pre-publish delay
    - response transit/receive delay
  - correlate these new subcomponents against `chunk_max_ms > 400` uploads.
  - rerun bounded 3x `36 MHz` soak after this split-residual instrumentation.

## 11.14 2026-03-03 Post-Handler Residual Split Instrumentation

- Firmware instrumentation update:
  - SD task now stamps chunk handler completion and publish edge for chunk
    responses.
  - SD bridge now stamps receive edge and computes publish-to-receive delay.
  - `upload_http: upload stats` now includes split residual fields:
    - `sd_task_post_handler_ms`
    - `sd_task_publish_to_receive_ms`
    - `sd_task_residual_other_ms`
  - `sd_task_residual_ms` is retained for continuity and now decomposes into:
    `post_handler + publish_to_receive + residual_other`.

- Smoke verification (1-cycle acceptance):
  - run: `logs/wifi_acceptance_split_residual_smoke_20260303_173818.log`
  - observed upload stats sample:
    - `sd_task_residual_ms=1291`
    - `sd_task_post_handler_ms=1`
    - `sd_task_publish_to_receive_ms=1290`
    - `sd_task_residual_other_ms=0`
  - preliminary signal: current residual in this sample is almost entirely in
    publish-to-receive delay, not SD-task post-handler delay.

- Next measurement step:
  - run bounded 3x `36 MHz` regression gates with this split-residual
  instrumentation and correlate `chunk_max_ms > 400` uploads against the new
  split fields.

## 11.15 2026-03-03 Split-Residual Correlation (3x Bounded Soak)

Run set used for split-residual correlation:

- bounded soak runs (`HOSTCTL_NET_CYCLES=10`, `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`):
  - `logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`
  - `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`
  - `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`

Notes:

- full regression-gate attempts in this window were unstable for reasons not
  tied to upload chunk timing (acceptance pre-stage listener/boot-discovery
  gate failures), so split-residual correlation used the direct bounded soak
  acceptance runs above.
- one stats line in `r3` was serial-concatenated/truncated and excluded from
  aggregate parsing.

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
- residual split shares:
  - `post_handler`: `~0.0%`
  - `publish_to_receive`: `~100.0%`
  - `other`: `~0.0%`
- outliers (`chunk_max_ms > 400`) remained publish-to-receive dominated:
  - `sd_task_residual_ms avg=1429.2`
  - `sd_task_publish_to_receive_ms avg=1429.0`

Conclusion:

- post-handler residual is not SD-task post-handler execution time.
- the dominant leg is bridge-side publish-to-receive delay.

Next focused root-cause target:

- investigate why chunk result receive is deferred during pipelined body ingest
  (bridge receive cadence/drain timing), and evaluate whether this delay is:
  - expected overlap accounting (diagnostic artifact), or
  - avoidable receive-lag that materially inflates request tail latency.

## 11.16 2026-03-03 Bridge Drain Mitigation (Non-Blocking Inflight Poll)

Change:

- added non-blocking chunk-result drain in HTTP body ingest loop:
  - bridge now attempts `try_receive` for inflight SD chunk result between body
    reads and drains completed chunks early without waiting for queue-boundary
    flush.
  - this reduces deferred publish-to-receive accumulation while preserving
    pipeline overlap behavior.

Validation runs:

- `logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`
- `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`

Comparison vs pre-fix split-residual soak set:

- pre-fix (`n=29`, r1-r3):
  - `req_ms avg=3135.1`
  - `chunk_max_ms avg=401.4` (`>400`: `9`)
  - `sd_task_residual_ms avg=1259.9`
  - `sd_task_publish_to_receive_ms avg=1259.5`
- post-fix (`n=20`, r1-r2):
  - `req_ms avg=3175.6`
  - `chunk_max_ms avg=172.4` (`>400`: `0`)
  - `sd_task_residual_ms avg=40.0`
  - `sd_task_publish_to_receive_ms avg=39.7`

Interpretation:

- bridge publish-to-receive deferral was the dominant residual contributor and
  is substantially mitigated by early non-blocking drain.
- request mean stayed in the same band in this small sample; extended gate
  validation is still required.

Next step:

- rerun full regression-gate campaign (1/3/soak profile) on this mitigation and
  confirm:
  - discovery stability
  - no panic/reboot regressions
  - upload request timing remains stable or better under bounded soak.

