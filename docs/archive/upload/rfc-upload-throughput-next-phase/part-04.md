## 11.17 2026-03-03 Host Transport A/B for Ingress Isolation (`direct` vs `chunked`)

Change:

- added host upload mode selector in hostctl:
  - `HOSTCTL_UPLOAD_MODE=auto|direct|chunked`
  - `auto` keeps existing behavior (try `PUT /upload`, fallback to chunked flow).
  - `direct` forces `PUT /upload` only.
  - `chunked` forces `/upload_begin` + `/upload_chunk` + `/upload_commit`.

Runs:

- direct (`HOSTCTL_UPLOAD_MODE=direct`, cycles=10):
  - `logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`
  - summary: `avg_upload_s=6.27`, `avg_kib_s=82.64`
- chunked (`HOSTCTL_UPLOAD_MODE=chunked`, cycles=10):
  - `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`
  - summary: `avg_upload_s=17.88`, `avg_kib_s=28.93`

Metric comparison method:

- primary comparison used `METRICS UPLOAD_PHASE` deltas (first vs last sample
  in each run) to avoid serial-line sampling bias under high request volume.
- both runs transferred the same payload volume (`5.0 MiB` across 10 cycles).

`METRICS UPLOAD_PHASE` delta results (normalized):

- direct (`reqs_per_512KiB=1.0`):
  - `body_ms`: `2457.3 ms/512KiB`
  - `sd_ms`: `1563.8 ms/512KiB`
  - `req_ms`: `3156.3 ms/512KiB`
- chunked (`reqs_per_512KiB=8.0`):
  - `body_ms`: `1685.0 ms/512KiB`
  - `sd_ms`: `1056.6 ms/512KiB`
  - `req_ms`: `2923.6 ms/512KiB`

Interpretation:

- forcing chunked transport lowers per-byte server-side request/body timing.
- despite that, end-to-end throughput collapses (`82.64 -> 28.93 KiB/s`) due
  multi-request orchestration overhead on the host/device path.
- this rejects forced chunking as the optimization path for current default
  upload flow.

Specific next root-cause target:

- keep direct `PUT /upload` as the performance path.
- focus ingress optimization inside direct upload:
  - investigate sender pacing / TCP ingress cadence that manifests as high
    `read_wait_ms` with mostly empty pre-read queues.
  - retain `HOSTCTL_UPLOAD_MODE` as an A/B lever for future validation.

## 11.18 2026-03-03 Direct Upload RX Buffer A/B (`65_536` vs `131_072`)

Change:

- added compile-time HTTP RX socket buffer tuning for PSRAM upload builds:
  - preferred env: `MEDITAMER_HTTP_RX_BUF_TARGET`
  - fallback: `HTTP_RX_BUF_TARGET`
  - accepted range: `8192..262144` (default `65536`)

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_NET_CYCLES=10`):

- baseline (`HTTP_RX_BUF_TARGET=65_536`, default build):
  - `logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`
  - runtime confirmation: `upload_http: http_rx buffer placement=Psram bytes=65536`
- variant (`MEDITAMER_HTTP_RX_BUF_TARGET=131072`):
  - `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`
  - runtime confirmation: `upload_http: http_rx buffer placement=Psram bytes=131072`

`upload_http: upload stats` comparison (`n=10` each):

- baseline:
  - `read_wait_ms avg=2398.9`
  - `req_ms avg=3093.5`
  - `ingress_pre_read_q_total avg=36347.6` (`~413.0 bytes/read`)
  - `ingress_read_wait_over_50ms avg=7.8` (`8.9%` of reads)
- variant:
  - `read_wait_ms avg=2802.6`
  - `req_ms avg=3491.6`
  - `ingress_pre_read_q_total avg=58205.2` (`~937.3 bytes/read`)
  - `ingress_read_wait_over_50ms avg=16.8` (`27.1%` of reads)

`METRICS UPLOAD_PHASE` delta comparison (equal `5.0 MiB` transferred):

- baseline:
  - `body_ms=2398.9 ms/512KiB`
  - `sd_ms=1566.3 ms/512KiB`
  - `req_ms=3093.5 ms/512KiB`
- variant:
  - `body_ms=2802.6 ms/512KiB`
  - `sd_ms=1684.6 ms/512KiB`
  - `req_ms=3491.4 ms/512KiB`

Host summary throughput:

- baseline: `avg_kib_s=97.83`
- variant: `avg_kib_s=80.26`

Decision:

- keep HTTP RX buffer target default at `65_536`.
- reject `131_072`; it worsens request latency and throughput in this direct-path
  bounded run.

Next step:

- keep direct upload path and focus on ingress pacing/jitter not solved by RX
  buffer growth:
  - instrument host-side send cadence (request write phase timing and burst/idle
    pattern) and correlate against firmware `read_wait_ms` spikes.

## 11.19 2026-03-03 Host Send Diagnostics + Retry-Class Probes (Direct Path)

Changes:

- host direct-upload instrumentation in hostctl:
  - per-upload timing line: `host_upload_send_diag`
  - retry classification line: `host_upload_retry_diag` (`transport_reset`,
    `sd_busy`, `timeout`, `transient`)
  - sidecar persistence default: `<HOSTCTL_NET_LOG_PATH>.hostdiag`
- host retry hardening:
  - rebuild client on `transport_reset` retry path
  - require configurable consecutive health passes before retrying:
    `HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH`

Primary correlation run (`HOSTCTL_UPLOAD_MODE=direct`, cycles=10):

- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log`
- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log.hostdiag`

Aggregate (`n=10`):

- firmware:
  - `read_wait_ms avg=2475.9`
  - `req_ms avg=3150.0`
- host:
  - `send_ms avg=3326.4`
  - `avg_attempts=2.00`
  - correlation: `corr(send_ms, read_wait_ms)=0.944`

Retry-class probe runs:

- pool A/B:
  - off: `logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`
  - on: `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`
  - delta (`on` vs `off`): `read_wait_ms 2439.6 -> 2395.0`, `req_ms 3116.2 -> 3084.8`
- connection-close A/B:
  - off: `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`
  - on: `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`
  - delta (`on` vs `off`): `read_wait_ms 2541.3 -> 2419.0`, `req_ms 3204.3 -> 3080.0`
  - retries increased (`retry_count 1 -> 3`)
- fresh-client A/B:
  - off: `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`
  - on: `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`
  - near-neutral latency deltas; retries unchanged in this sample (`3` vs `3`)

Interpretation:

- send-side timing remains strongly coupled with firmware `read_wait_ms`.
- no host transport toggle consistently removes `transport_reset` first-attempt
  retries while preserving clear ingress wins.

## 11.20 2026-03-03 Direct Upload `TCP_NODELAY` A/B (`1` vs `0`)

Change:

- added `HOSTCTL_UPLOAD_TCP_NODELAY` (`1` default) in host upload client.

Runs (`HOSTCTL_UPLOAD_MODE=direct`, cycles=5):

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

- keep default `HOSTCTL_UPLOAD_TCP_NODELAY=1`.
- disabling `TCP_NODELAY` regressed both throughput and request timing in this
  bounded A/B.

## 11.21 2026-03-03 Ingress Wait Split Telemetry (Empty vs Non-Empty RX Queue)

Change:

- added firmware ingress wait decomposition:
  - `ingress_read_wait_empty_q_ms`
  - `ingress_read_wait_nonempty_q_ms`

Validation run:

- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`
- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log.hostdiag`

Aggregate (`n=3`):

- `read_wait_ms avg=2355.7`
- `ingress_read_wait_empty_q_ms avg=2351.3`
- `ingress_read_wait_nonempty_q_ms avg=4.3`
- empty-queue share of read-wait: `~99.8%`

Interpretation:

- direct-path ingress wait is almost entirely no-data waiting (socket queue
  empty), not delayed reads against already-buffered data.

Specific next root-cause target:

- keep direct upload + `TCP_NODELAY=1` baseline.
- shift optimization focus to upstream ingress pacing (network/AP/radio path)
  rather than HTTP socket buffer sizing or host client pooling toggles.

