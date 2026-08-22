# SD Asset Upload Over Wi-Fi

Upload server (STA + HTTP) for pushing assets to the SD card without removing
it.

## Build and configure

Build/flash:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/flash.sh debug
```

Notes:

- optional compile-time credentials are still supported via `MEDITAMER_WIFI_SSID` / `MEDITAMER_WIFI_PASSWORD`
  (fallback `SSID` / `PASSWORD`).
- upload chunk size is compile-time tunable for PSRAM upload builds:
  - preferred env: `MEDITAMER_SD_UPLOAD_CHUNK_MAX`
  - fallback env: `SD_UPLOAD_CHUNK_MAX`
  - accepted range: `4096..65536` (bytes)
- upload HTTP RX socket buffer target is compile-time tunable for PSRAM upload builds:
  - preferred env: `MEDITAMER_HTTP_RX_BUF_TARGET`
  - fallback env: `HTTP_RX_BUF_TARGET`
  - accepted range: `8192..262144` (bytes), default `65536`
- upload service must be enabled at runtime (`STATE SET upload=on`).
- hard-cut runtime network control now uses `NET*` UART commands only.
- server listens on port `8080` after DHCP lease.
- when an upload token is configured, all HTTP endpoints except `/health` require an `x-upload-token` header;
  requests without a valid token are rejected.
- if neither `MEDITAMER_UPLOAD_HTTP_TOKEN` nor `UPLOAD_HTTP_TOKEN` is set at build time, authentication is
  disabled and non-`/health` endpoints accept requests without an `x-upload-token` header.
- configure the token at build time with `MEDITAMER_UPLOAD_HTTP_TOKEN` (fallback: `UPLOAD_HTTP_TOKEN`).
- mutating endpoints (`/mkdir`, `/rm`, `/upload*`) are limited to the `/assets` subtree.

## Runtime network control (UART)

Runtime network policy/config provisioning over UART:

```text
NETCFG SET {"ssid":"<ssid>","password":"<password>","connect_timeout_ms":30000,"dhcp_timeout_ms":20000,"pinned_dhcp_timeout_ms":45000,"listener_timeout_ms":25000,"scan_active_min_ms":600,"scan_active_max_ms":1500,"scan_passive_ms":1500,"retry_same_max":2,"rotate_candidate_max":2,"rotate_auth_max":5,"full_scan_reset_max":1,"driver_restart_max":1,"cooldown_ms":1200,"driver_restart_backoff_ms":2500}
```

Read current runtime config:

```text
NETCFG GET
```

Start/stop/recover/status:

```text
NET START
NET STOP
NET RECOVER
NET STATUS
NET LISTENER ON
NET LISTENER OFF
```

Credential persistence:

- `NETCFG SET` with `ssid` persists credentials to SD file `/config/wifi.cfg`.
- On boot, firmware attempts to load `/config/wifi.cfg` before waiting for runtime `NETCFG SET`.
- This survives reboot and firmware reflashes (as long as SD card content is retained).

## Host-side credentials

Local Wi-Fi credentials for hardware scripts:

1. Copy `.env.example` to `.env.local`.
2. Set `HOSTCTL_NET_SSID` and `HOSTCTL_NET_PASSWORD` in `.env.local`.
3. Keep `.env.local` untracked (already gitignored).

The Wi-Fi hardware wrappers auto-load `.env.local`:

```bash
cp .env.example .env.local
scripts/tests/hw/test_wifi_acceptance.sh
```

Wi-Fi acceptance helper (hard-cut, explicit overrides):

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_LOG_PATH=./logs/wifi_acceptance_manual.log \
scripts/tests/hw/test_wifi_acceptance.sh
```

## HTTP endpoints

Health check:

```bash
curl "http://<device-ip>:8080/health"
```

Create directory (authenticated endpoint):

```bash
UPLOAD_TOKEN=<your-upload-token>
curl -X POST \
  -H "x-upload-token: ${UPLOAD_TOKEN}" \
  "http://<device-ip>:8080/mkdir?path=/assets/images"
```

Delete file or empty directory (authenticated endpoint):

```bash
curl -X DELETE \
  -H "x-upload-token: ${UPLOAD_TOKEN}" \
  "http://<device-ip>:8080/rm?path=/assets/old.bin"
```

## Uploading with hostctl

Upload an assets directory:

```bash
scripts/hostctl.sh upload --host <device-ip> --src assets --dst /assets
```

Upload a single file:

```bash
scripts/hostctl.sh upload --host <device-ip> --src ./path/to/file.bin --dst /assets
```

Relative `--src` paths are resolved from the repository root, independent of the caller's working
directory and the launcher's isolated Cargo working directory.

Optional upload helper tuning:

- `HOSTCTL_UPLOAD_CHUNK_SIZE` controls chunk size in bytes for `/upload_chunk` fallback flow (default `65536`).
- `HOSTCTL_UPLOAD_MODE` selects upload transport behavior:
  - `auto` (default): try direct `PUT /upload`, fallback to chunked flow on failure
  - `direct`: force direct `PUT /upload` only (no chunked fallback)
  - `chunked`: force chunked flow (`/upload_begin` + `/upload_chunk` + `/upload_commit`)
- `HOSTCTL_UPLOAD_SEND_DIAG` (`0` default): emit host-side upload timing diagnostics (`host_upload_send_diag`) and retry classing (`host_upload_retry_diag`) for direct uploads.
- `HOSTCTL_UPLOAD_SEND_DIAG_DEEP` (`0` default): enable deep body-read cadence instrumentation in host upload diagnostics (more intrusive; use only for short targeted runs).
- `HOSTCTL_UPLOAD_SEND_DIAG_PATH` (optional): explicit path for host send-diagnostic sidecar log; defaults to `<HOSTCTL_NET_LOG_PATH>.hostdiag`.

Delete paths (relative to `--dst`, or absolute under `/assets`):

```bash
scripts/hostctl.sh upload --host <device-ip> --dst /assets --rm old.bin --rm unused/
```

## Suggested runtime flow

1. `STATE SET upload=on`
2. `NETCFG SET {...}`
3. `NET START`
4. poll `NET STATUS` until `state="Ready"` and non-zero IPv4
5. Upload files over HTTP
6. `STATE SET upload=off`

## Acceptance workflow and A/B knobs

Wi-Fi acceptance workflow:

```bash
scripts/tests/hw/test_wifi_acceptance.sh
```

- runs via `hostctl test wifi-acceptance` behind the script wrapper.
- strategy execution is declarative (`tools/hostctl/scenarios/wifi-acceptance.sw.yaml`) with primitive hostctl actions.
- consumes only `HOSTCTL_NET_*` environment contract.
- readiness uses structured firmware frames (`NET_STATUS {...}`), not monitor-tail text parsing.
- before any new A/B knob run, perform novelty preflight to avoid duplicate experiments:
  - `docs/archive/wifi/wifi-upload-decision-ledger.md` (quick decision check)
  - `rg -n "<knob>|<value>" docs/archive/upload/rfc-upload-throughput-next-phase docs/archive/upload/upload-throughput-history`
- acceptance/regression wrappers enforce a novelty guard for known decided knobs:
  - set `HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE=1` only for explicit reconfirmation reruns
- upload chunk pipeline A/B: default build now enables the pipeline feature.
- adaptive ingress fairness A/B (firmware build-time knob):
  - `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=1` (default `0`)
  - latest decision: keep non-default diagnostics only (not promoted)
- additional Wi-Fi diagnostics, blackout probes, and regression-gate notes now
  live in `docs/archive/wifi/blackout-diagnostic-knobs.md`.
- to force baseline (pipeline off) for comparison, use:
  - `CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=asset-upload-http scripts/build/build.sh debug`
  - `CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=asset-upload-http scripts/device/flash.sh debug`

## Verifying a change

Discovery debug, the panic-first Wi-Fi/upload regression gate, and the
`HOSTCTL_NET_*` guardrail env vars are documented once, in the
[Wi-Fi Regression Gate](wifi-regression-gate.md). Run that gate before landing
anything that touches Wi-Fi, the network stack, or upload.

The `HOSTCTL_NET_*` env contract and UART command contract used by the
acceptance workflow itself are in
[Network Acceptance Workflow](agents/network-acceptance.md).
