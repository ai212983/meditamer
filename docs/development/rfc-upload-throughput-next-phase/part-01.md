# RFC: Upload Throughput Next Phase (Pipeline + Latency Decomposition)

- Status: In Progress (Phases A-B complete; Phase C deferred; chunk-size default `65_536` stable under current bounded soak gate; SD SPI variance A/B complete with `36 MHz` retained; CMD25 + FAT append + queue-boundary diagnostics completed; split-residual root-cause isolated to bridge publish-to-receive leg; mitigation via non-blocking inflight drain implemented and under extended validation; keep-alive fix validated with near-parity throughput; host cross-cycle client reuse retained as opt-in only; unstable `Ready`-but-unreachable edge pinned to stalled upload-body read path and timeout guard added, pending flashed validation)
- Owner: Firmware/Host Tooling
- Date: 2026-03-01
- Last Updated: 2026-03-04
- Scope: firmware upload path + host acceptance harness (no `tools/scene_*`)

## 1. Summary

This RFC proposes the next optimization phase for upload throughput after recent SD and HTTP improvements.

Primary proposal:

1. Add fine-grained upload latency decomposition telemetry.
2. Implement chunk pipeline (overlap HTTP body ingestion with SD write processing).
3. Keep discovery reliability invariants and gate throughput tests behind non-zero discovery proof.

## 2. Problem Statement

We improved throughput significantly over early baselines, but current telemetry still shows meaningful bottlenecks:

- SD wait remains a dominant fixed cost per 512 KiB upload (`sd_ms` commonly ~`1220..1300`).
- Network/body ingestion time is still variable (`body_ms` spikes observed).
- Current firmware path is effectively stop-and-wait by chunk:
  - read body data into chunk buffer
  - send to SD task
  - wait for SD result
  - continue reading

This serialization leaves overlap opportunity unused.

## 3. Current Evidence

From bounded 1-cycle sweep runs (524288-byte payload):

- `24 MHz`: `throughput_kib_s=178.58`, `upload_ms=2867`
- `30 MHz`: `throughput_kib_s=173.21`, `upload_ms=2956`
- `36 MHz`: `throughput_kib_s=194.97`, `upload_ms=2626`
- `40 MHz`: `throughput_kib_s=182.08`, `upload_ms=2812`

Device write metrics remained stable:

- `cmd24_sectors=40`
- `cmd25_attempt_sectors=1024`
- `cmd25_success_sectors=1024`
- `cmd25_fallback_bursts=0`

Interpretation:

- Payload path is healthy CMD25 multi-block.
- Metadata overhead is still present but stable.
- Largest remaining opportunity is reducing serialization and jitter, not retrying SD write-mode changes.

## 4. Goals

1. Increase sustained upload throughput with no discovery reliability regressions.
2. Preserve zero-fallback CMD25 behavior.
3. Reduce per-upload latency variance across 1-cycle, 3-cycle, and bounded soak runs.
4. Make next bottleneck obvious via telemetry without log forensics.

## 5. Non-Goals

1. Rewriting the full SD FAT layer.
2. Changing Wi-Fi association strategy in this RFC.
3. Any work under `tools/scene_*`.

## 6. Proposal

## 6.1 Phase A: Latency Decomposition Telemetry

- Status: Completed (2026-03-03)

Add explicit metrics/log fields for each upload request:

- socket read wait time
- payload copy time
- sd-bridge queue/send wait
- sd-task processing wait
- commit/rename phase time
- per-chunk p50/p95/max (bounded counters)

Expected output should support a one-line decomposition per upload and summary counters in existing `METRICS UPLOAD_*` serial reporting.

## 6.2 Phase B: Chunk Pipeline (Double Buffer)

- Status: Completed (implemented and enabled by default after 2026-03-03 A/B pass)

Implement a two-buffer producer/consumer flow:

- Buffer A is being written to SD task while buffer B is filled from socket.
- Ensure strict ordering semantics for chunk sequence.
- Preserve existing error handling and abort logic on first failed stage.

Constraints:

- Reuse PSRAM-backed buffers when available.
- Keep internal RAM pressure bounded and observable (`internal_free`, low-water).
- Maintain compatibility with existing `/upload` and `/upload_chunk` behavior.

## 6.3 Phase C: Metadata Cost Tightening

- Status: Deferred (post-pipeline evidence shows commit/metadata is not dominant)

After pipeline lands, profile remaining fixed cost.
If commit-phase metadata is still dominant:

- optimize append-session flush/rename ordering where safe
- avoid redundant metadata updates during active chunk stream
- keep crash-consistency guarantees unchanged

## 7. Reliability Guardrails

The following are mandatory for this RFC rollout:

1. Discovery proof before throughput profiling:
   - bounded `wifi-discovery-debug` round after boot
   - non-zero scan evidence and target SSID observed
2. Throughput runs must not proceed if discovery/connectivity is failing.
3. Upload listener disable-mode for discovery probes remains available for diagnosis.
4. Any throughput optimization that reintroduces zero-discovery regressions is rejected.

## 8. Validation Plan

Run sequence:

1. `wifi-discovery-debug` bounded round after boot.
2. `wifi-acceptance` 1-cycle.
3. `wifi-acceptance` 3-cycle.
4. bounded soak.

Capture and compare:

- `throughput_kib_s`, `upload_ms`
- `body_ms`, `sd_ms`, `req_ms`
- `cmd25_fallback_bursts` (must remain 0)
- discovery counters (`scan_zero`, `scan_nonzero`, `ssid_seen`)

Acceptance criteria:

1. No zero-discovery regression in validation sequence.
2. CMD25 fallback remains zero for normal acceptance payloads.
3. Throughput improvement is measurable in both:
   - 3-cycle average
   - bounded soak average
4. No new memory-pressure failure mode (`NoMem`) in scan/start paths.

## 9. Rollout and Rollback

Rollout:

1. Land telemetry first.
2. Land pipeline behind a compile-time or runtime toggle for A/B.
3. Enable by default only after passing full validation sequence.

Rollback:

1. Disable pipeline toggle.
2. Keep telemetry in place.
3. Revert only optimization layer; preserve diagnostics.

## 10. Risks

1. Increased buffer concurrency may increase internal RAM pressure.
2. Incorrect chunk ordering can corrupt uploads.
3. Pipeline error handling may produce stale SD results if not drained carefully.
4. Host acceptance harness timeouts can mask firmware improvements if not bounded consistently.

## 11. Execution Checklist for Next Session

1. [x] Confirm baseline commit and run discovery proof (2026-03-01 sequence recorded in throughput history).
2. [x] Add Phase A telemetry (implemented in firmware metrics and upload decomposition reporting).
3. [x] Run 1/3/soak with no behavior changes; record decomposition baseline (recorded in throughput history).
4. [x] Implement Phase B pipeline with guard toggle (`asset-upload-http-pipeline`).
5. [x] Re-run 1/3/soak with pipeline enabled, compare deltas, and check discovery stability (2026-03-03 A/B run).
6. [x] Decide on Phase C only if fixed commit/metadata cost remains dominant after Phase B A/B (2026-03-03: deferred; commit cost not dominant in A/B logs).
7. [x] Update throughput history and regression guardrail docs with final A/B measurements.
8. [x] Re-run regression gate with pipeline enabled in default feature set (`logs/wifi_regression_gate_default_confirm_20260303_121014`).
9. [x] Run next A/B: make upload chunk size build-tunable, then compare `SD_UPLOAD_CHUNK_MAX=49_152` vs `65_536` under the same regression gate (2026-03-03 chunk-size A/B run).
10. [x] Run bounded soak at `SD_UPLOAD_CHUNK_MAX=65_536` before default switch (2026-03-03: failed due runtime panic in acceptance soak).
11. [x] Add stack-headroom diagnostics around upload begin/route + SD begin and capture panic-focused evidence (`feat(telemetry): add stack headroom probes for upload panic triage`).
12. [x] Reduce default touch trace pressure to recover stack headroom and rerun `65_536` bounded soak (`fix(touch): reduce trace channel buffers to reclaim stack headroom` + pass at `logs/wifi_regression_gate_65536_postfix_20260303_141406`).
13. [x] Waive extended soak (24h profile) and proceed with default switch at `SD_UPLOAD_CHUNK_MAX=65_536` (owner risk acceptance, 2026-03-03).
14. [x] Harden upload read-body reset recovery (`54e952f`) with immediate socket abort + `req_read_body_reset` telemetry counter.
15. [x] Re-run default `65_536` regression gate (with soak=10) three times and confirm all stages pass with no panic/reboot markers (`logs/wifi_regression_gate_default65536_connresetfix_r1_20260303_144611`, `logs/wifi_regression_gate_default65536_connresetfix_r2_20260303_144943`, `logs/wifi_regression_gate_default65536_connresetfix_r3_20260303_145315`).
16. [x] Add host acceptance guardrail step `assert_upload_metrics` to fail if `METRICS UPLOAD req_read_body_reset` increases beyond configured delta (`HOSTCTL_NET_REQ_READ_BODY_RESET_MAX_DELTA`, default `0`).
17. [x] Mark `SD_UPLOAD_CHUNK_MAX_DEFAULT=65_536` as stable for bounded soak gate and move to variance-reduction A/B planning.
18. [x] Run SD SPI variance A/B (`MEDITAMER_SD_SPI_DATA_MHZ=36` vs `40`) with full regression gate + soak and compare request-timing spread (`logs/wifi_regression_gate_sdspi36b_20260303_151750`, `logs/wifi_regression_gate_sdspi40_20260303_152151`).
19. [x] Fix host panic classification false-positive on upload metrics lines containing `_abort` (`2b2a3b3`).
20. [x] Add CMD25 burst/ready-wait diagnostics to upload write metrics (`3bb91e0`).
21. [x] Run 3x `36 MHz` bounded soak regression gates with burst diagnostics and correlate request timing with CMD25 wait metrics (`logs/wifi_regression_gate_sdspi36_burstdiag_r1_20260303_161323`, `logs/wifi_regression_gate_sdspi36_burstdiag_r2_20260303_161645`, `logs/wifi_regression_gate_sdspi36_burstdiag_r3b_20260303_162537`).
22. [x] Add per-chunk FAT append timing diagnostics (`ensure_capacity_ms`, `write_data_ms`, chunk boundary totals) to upload write metrics (`64f6da6`).
23. [x] Run 3x `36 MHz` bounded soak regression gates with append diagnostics and correlate high `chunk_max_ms` against append-path timing fields (`logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`, `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`, `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`).
24. [x] Add queue-boundary chunk timing diagnostics (`enqueued_at_ms`, chunk queue-wait/handler timing in response, `sd_task_*` decomposition in HTTP upload stats) (`e85f2a7`).
25. [x] Run 3x `36 MHz` bounded soak regression gates with queue-boundary diagnostics and correlate high `chunk_max_ms` against queue/handler/residual fields (`logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`, `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`, `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`).
26. [x] Add post-handler residual split instrumentation (SD-task publish-edge + bridge receive-edge timing) and expose split fields in `upload_http: upload stats`.
27. [x] Run bounded 3x soak correlation with split-residual instrumentation and identify dominant residual leg (`logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`, `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`, `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`).
28. [x] Implement bridge non-blocking inflight-drain mitigation and validate on bounded soak (`logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`, `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`).
29. [x] Run host transport A/B to isolate ingress behavior (`HOSTCTL_UPLOAD_MODE=direct` vs `chunked`) and compare bounded 10-cycle results plus `METRICS UPLOAD_PHASE` deltas (`logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`, `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`).
30. [x] Run direct-mode HTTP RX buffer A/B (`65_536` vs `131_072`) and compare ingress/read-wait metrics plus `METRICS UPLOAD_PHASE` deltas (`logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`, `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`).
31. [x] Add host direct-upload send diagnostics + retry classing with persisted sidecar logs (`HOSTCTL_UPLOAD_SEND_DIAG*`, `host_upload_send_diag`, `host_upload_retry_diag`).
32. [x] Run host transport retry-class probes (`HOSTCTL_UPLOAD_DISABLE_POOL`, `HOSTCTL_UPLOAD_FORCE_CONN_CLOSE`, `HOSTCTL_UPLOAD_FRESH_CLIENT_PER_UPLOAD`) and compare ingress deltas (`logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`, `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`, `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`, `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`, `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`, `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`).
33. [x] Add host socket `TCP_NODELAY` A/B knob and compare direct-path ingress timing (`logs/wifi_acceptance_nodelayab_on_direct5_20260303_195857.log`, `logs/wifi_acceptance_nodelayab_off_direct5_20260303_200015.log`).
34. [x] Add firmware ingress wait split metrics (`ingress_read_wait_empty_q_ms`/`ingress_read_wait_nonempty_q_ms`) and capture bounded validation run (`logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`).
35. [x] Add Wi-Fi RSSI telemetry context (`METRICS WIFI_LINK` + per-upload RSSI fields), validate discovery recovery, and run direct 10-cycle correlation sample (`logs/wifi_discovery_rssi_recover_20260304_083129.log`, `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log`).
36. [x] Add deep host retry cause-chain diagnostics + direct pre-PUT pacing knob, then run bounded direct A/B (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0` vs `120`) to verify first-attempt failure class behavior (`logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`, `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`).
37. [x] Add firmware `NET_ACCEPT` microsecond accept-arm gap telemetry and capture bounded direct evidence (`logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log`).
38. [x] Validate firmware keep-alive/multi-request socket handling fix (remove forced `Connection: close`) with bounded direct acceptance (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`) after serial-port recovery (2026-03-04; matched direct A/B pairs now recorded in sections 11.24-11.25).
39. [x] Add optional host acceptance cross-cycle upload-client reuse (`HOSTCTL_NET_REUSE_UPLOAD_CLIENT`) and run bounded direct comparisons against forced-close control (2026-03-04; no consistent throughput gain).
40. [x] Focus next optimization on firmware ingress empty-queue `read_wait_ms` dominance (implemented cooperative read-loop fairness yield in firmware and captured bounded validation, 2026-03-04).
41. [x] Re-run extended direct acceptance (`cycles=10`) with ingress fairness yield enabled and tune yield thresholds (`bytes`/`reads`) for throughput vs variance (2026-03-04).
42. [x] Run bounded soak with promoted ingress fairness thresholds to confirm failure-class stability and long-run variance behavior (`logs/wifi_acceptance_ingressfairness_soak10_20260304_113208.log`).
43. [x] Fix listener/DHCP readiness instability caused by stale listener-timeout baseline on long-lived connections; validate with full regression gate including soak (`logs/wifi_regression_gate_listenerfix_20260304_114631/report.json`).
44. [x] Pin post-burst unstable `Ready`-but-unreachable state to stalled `/upload` body-read path and add dedicated upload-body idle timeout guard (`HTTP_UPLOAD_BODY_READ_TIMEOUT_MS=6000`) in connection routes (2026-03-04).
45. [x] Implement reqwest-based direct `PUT /upload` burst sender mode (no raw TCP), run bounded 10-cycle A/B, and decide promotion based on throughput/variance plus ingress wait metrics (`logs/wifi_acceptance_burstab_reqwest_off_20260304_130807.log`, `logs/wifi_acceptance_burstab_reqwest_on_20260304_130910.log`).
46. [x] Tune firmware ingress loop to reduce per-read pipeline polling overhead (cadenced inflight try-drain), then validate bounded direct 10-cycle throughput/ingress metrics (`logs/wifi_acceptance_ingressdrain_tune_direct10_20260304_132054.log`, `logs/wifi_acceptance_ingressdrain_tune_direct10_confirm_20260304_132309.log`).
47. [x] Run ingress try-drain cadence sweep (`2/4/8` reads), compare throughput/variance plus ingress wait metrics, and promote best default (`logs/wifi_acceptance_ingressdraincadence2_direct10_20260304_132736.log`, `logs/wifi_acceptance_ingressdraincadence4_direct10_20260304_133009.log`, `logs/wifi_acceptance_ingressdraincadence8_direct10_20260304_133238.log`).

