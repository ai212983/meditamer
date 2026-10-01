# Network and upload contracts

[Upload workflow](../guides/network/asset-upload.md) and
[network validation](../guides/network/validation.md) own procedures.
Inkplate includes Wi-Fi uploads by default; Medinote/S3 requires `wifi-storage`.

## Configuration and credentials

`NETCFG GET` queries configuration; `NETCFG SET <json>` applies it. The
[policy template](../../tools/hostctl/scenarios/wifi-policy.default.json) owns
field defaults. `ssid`/`password` provision credentials; timeout, scan, retry,
rotation and recovery fields tune runtime policy.

Credential SET writes/verifies a versioned plaintext record in dedicated internal
flash `wifi_config` (8 KiB, two-sector recovery, CRC integrity). Boot restores the
newest valid generation before radio startup. Erased/corrupt means unprovisioned,
radio off. Legacy SD credentials and build-time credentials are ignored.
Existing Inkplate devices need one complete USB flash to install the partition;
an app-only bundle cannot change the table. Normal partition-aware updates retain
credentials; full-chip erase clears them. Records are not encrypted.

## Network control

```text
NET START
NET STOP
NET RECOVER
NET STATUS
NET LISTENER ON
NET LISTENER OFF
```

Readiness/failure classification uses structured `NET_STATUS {...}` and
`NET_EVENT {...}`. Inkplate application enable is `STATE SET upload=on|off`;
Medinote uses NET START/STOP or its Wi-Fi app. Medinote starts Wi-Fi off on cold
boot and restores its previous enabled setting after deep-sleep wake.

Inkplate START/STOP acknowledges after persistence and display application, with
a 150-second firmware deadline; hostctl sends once and waits 155 seconds. Missing
ACK is an unknown outcome: inspect capture before sending another state command.
STOP alone does not grant BLE ownership; require correlated
`RADIO_HANDOFF_ACK kind=quiesced state=off_confirmed`.

## HTTP and tokens

Listener port is `8080`, after DHCP. Shared HTTP v1 routes are `/health`, `/stat`,
`/mkdir`, `/rm`, direct `/upload`, and chunked `/upload_begin`, `/upload_chunk`,
`/upload_commit`, `/upload_abort`. Mutations are restricted to `/assets`.
`/health` is unauthenticated. With a token configured, all other endpoints require
an exact `x-upload-token` header. S3 requires a nonempty build token before
exposing the listener; Inkplate permits an optional token.

Build token: `MEDITAMER_UPLOAD_HTTP_TOKEN`, fallback `UPLOAD_HTTP_TOKEN`.
Host token: `HOSTCTL_UPLOAD_TOKEN` (prefer over CLI `--token` to avoid disclosure
through arguments/transcripts). Keep both in untracked environments.
Inkplate retains a pending TCP accept across 500 ms service/link polls; gate
closure or acceptance error aborts it before re-arming.

## Buffer and client controls

| Variable | Range / default |
| --- | --- |
| `MEDITAMER_SD_UPLOAD_CHUNK_MAX` (fallback `SD_UPLOAD_CHUNK_MAX`) | Inkplate PSRAM build: 4096–65536 bytes |
| `MEDITAMER_HTTP_RX_BUF_TARGET` (fallback `HTTP_RX_BUF_TARGET`) | Inkplate PSRAM build: 8192–262144, default 65536 |
| `HOSTCTL_UPLOAD_CHUNK_SIZE` | Chunked fallback size, default 65536 |
| `HOSTCTL_UPLOAD_MODE` | `auto` (direct, then chunked fallback), `direct`, `chunked` |
| `HOSTCTL_UPLOAD_SEND_DIAG` | `0`; host timing/retry diagnostics |
| `HOSTCTL_UPLOAD_SEND_DIAG_DEEP` | `0`; intrusive cadence instrumentation for short runs |
| `HOSTCTL_UPLOAD_SEND_DIAG_PATH` | Sidecar override; default `<HOSTCTL_NET_LOG_PATH>.hostdiag` |

S3 buffers are fixed at 8 KiB per transfer buffer and 64 KiB per RX socket;
Inkplate build tuning does not apply. `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS`
is a non-default diagnostic knob, not a promoted production policy.
