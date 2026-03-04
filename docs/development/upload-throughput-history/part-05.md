## 2026-03-03: host transport A/B for ingress isolation (`direct` vs `chunked`)

Host-tooling change:

- added `HOSTCTL_UPLOAD_MODE` in hostctl upload flow:
  - `auto` (default): try direct `PUT /upload`, fallback to chunked
  - `direct`: force direct `PUT /upload` only
  - `chunked`: force `/upload_begin` + `/upload_chunk` + `/upload_commit`

10-cycle acceptance runs:

- direct mode:
  - `logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`
  - summary: `avg_upload_s=6.27`, `avg_kib_s=82.64`
- chunked mode:
  - `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`
  - summary: `avg_upload_s=17.88`, `avg_kib_s=28.93`

Normalization method:

- used `METRICS UPLOAD_PHASE` delta (last-first) in each run to compare equal
  transferred bytes while avoiding serial-line sampling bias.
- both runs transferred `5.0 MiB` total (`10` acceptance uploads).

`METRICS UPLOAD_PHASE` delta comparison (per `512 KiB` payload):

- direct (`reqs_per_512KiB=1.0`):
  - `body_ms=2457.3`
  - `sd_ms=1563.8`
  - `req_ms=3156.3`
- chunked (`reqs_per_512KiB=8.0`):
  - `body_ms=1685.0`
  - `sd_ms=1056.6`
  - `req_ms=2923.6`

Interpretation:

- chunked transport reduces server-side per-byte timing inside individual
  requests, but total upload throughput is much worse because the multi-request
  orchestration dominates wall-clock time.
- optimization should remain on direct `PUT /upload` and target ingress pacing
  within that path.
- `HOSTCTL_UPLOAD_MODE` is retained as a deterministic A/B switch for future
  experiments.

## 2026-03-03: direct-path HTTP RX buffer A/B (`65_536` vs `131_072`)

Firmware change:

- added compile-time HTTP RX socket buffer tuning (PSRAM upload builds):
  - preferred env: `MEDITAMER_HTTP_RX_BUF_TARGET`
  - fallback env: `HTTP_RX_BUF_TARGET`
  - accepted range: `8192..262144` (default `65536`)

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_NET_CYCLES=10`):

- baseline (`65_536`, default build):
  - `logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`
  - runtime confirms: `upload_http: http_rx buffer placement=Psram bytes=65536`
- variant (`131_072`):
  - `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`
  - runtime confirms: `upload_http: http_rx buffer placement=Psram bytes=131072`

`upload_http: upload stats` aggregates (`n=10` each):

- baseline:
  - `read_wait_ms avg=2398.9`
  - `req_ms avg=3093.5`
  - `ingress_pre_read_q_total avg=36347.6` (`~413.0 bytes/read`)
  - `ingress_read_wait_over_50ms avg=7.8` (`8.9%` reads)
- variant:
  - `read_wait_ms avg=2802.6`
  - `req_ms avg=3491.6`
  - `ingress_pre_read_q_total avg=58205.2` (`~937.3 bytes/read`)
  - `ingress_read_wait_over_50ms avg=16.8` (`27.1%` reads)

`METRICS UPLOAD_PHASE` delta normalization (equal `5.0 MiB` transferred):

- baseline:
  - `body_ms=2398.9 ms/512KiB`
  - `sd_ms=1566.3 ms/512KiB`
  - `req_ms=3093.5 ms/512KiB`
- variant:
  - `body_ms=2802.6 ms/512KiB`
  - `sd_ms=1684.6 ms/512KiB`
  - `req_ms=3491.4 ms/512KiB`

Host throughput summary:

- baseline: `avg_kib_s=97.83`
- variant: `avg_kib_s=80.26`

Decision:

- retain default `HTTP_RX_BUF_TARGET=65_536`.
- do not promote `131_072`; this bounded direct-mode A/B worsens latency and
  throughput.

Next focus:

- ingress pacing root-cause in direct upload (host send cadence / burst-idle
  pattern correlation against firmware `read_wait_ms`).

## 2026-03-03: host send diagnostics + retry-class probe matrix (direct path)

Host-tooling changes:

- added host direct-upload diagnostics:
  - `host_upload_send_diag` (attempts, send/total ms, optional body cadence)
  - `host_upload_retry_diag` (retry class flags including `transport_reset`)
- added sidecar log persistence:
  - default `<HOSTCTL_NET_LOG_PATH>.hostdiag`
  - optional `HOSTCTL_UPLOAD_SEND_DIAG_PATH`
- added retry hardening knob:
  - `HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH`

Primary correlation run (`direct`, cycles=10):

- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log`
- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log.hostdiag`

Aggregate (`n=10`):

- firmware:
  - `read_wait_ms avg=2475.9`
  - `req_ms avg=3150.0`
- host:
  - `send_ms avg=3326.4`
  - `avg_attempts=2.00`
  - `corr(send_ms, read_wait_ms)=0.944`

Retry-class probes:

- pool A/B:
  - off: `logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`
  - on: `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`
  - `read_wait_ms avg`: `2439.6 -> 2395.0`
  - `req_ms avg`: `3116.2 -> 3084.8`
- connection-close A/B:
  - off: `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`
  - on: `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`
  - `read_wait_ms avg`: `2541.3 -> 2419.0`
  - `req_ms avg`: `3204.3 -> 3080.0`
  - retry count increased in this sample (`1 -> 3`)
- fresh-client A/B:
  - off: `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`
  - on: `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`
  - near-neutral latency deltas; retries unchanged (`3 -> 3`)

Interpretation:

- host send timing remains tightly coupled with firmware `read_wait_ms`.
- no host transport toggle consistently eliminates `transport_reset` first-attempt
  retries.

## 2026-03-03: `HOSTCTL_UPLOAD_TCP_NODELAY` A/B (`1` vs `0`)

Change:

- added host knob `HOSTCTL_UPLOAD_TCP_NODELAY` (`1` default).

Runs (`direct`, cycles=5):

- `TCP_NODELAY=1`:
  - `logs/wifi_acceptance_nodelayab_on_direct5_20260303_195857.log`
  - summary: `avg_upload_s=5.56`, `avg_kib_s=93.23`
- `TCP_NODELAY=0`:
  - `logs/wifi_acceptance_nodelayab_off_direct5_20260303_200015.log`
  - summary: `avg_upload_s=5.91`, `avg_kib_s=87.59`

`upload_http: upload stats` aggregates (`n=5`):

- `TCP_NODELAY=1`:
  - `read_wait_ms avg=2289.4`
  - `req_ms avg=2960.0`
  - `ingress_pre_read_q_total avg=31602.2`
- `TCP_NODELAY=0`:
  - `read_wait_ms avg=2397.6`
  - `req_ms avg=3060.4`
  - `ingress_pre_read_q_total avg=35916.2`

Decision:

- keep `HOSTCTL_UPLOAD_TCP_NODELAY=1` default; disabling it regressed bounded
  throughput and request timing.

## 2026-03-03: ingress wait split telemetry (`empty_q` vs `nonempty_q`)

Firmware change:

- added upload stats fields:
  - `ingress_read_wait_empty_q_ms`
  - `ingress_read_wait_nonempty_q_ms`

Validation run:

- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`
- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log.hostdiag`

Aggregate (`n=3`):

- `read_wait_ms avg=2355.7`
- `ingress_read_wait_empty_q_ms avg=2351.3`
- `ingress_read_wait_nonempty_q_ms avg=4.3`
- empty-queue share of `read_wait_ms`: `~99.8%`

Conclusion:

- current ingress bottleneck is almost entirely no-data wait at socket read
  points; this is not a buffered-data drain delay.

