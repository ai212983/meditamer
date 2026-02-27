# Telemetry Control (Agent-First)

This guide defines the UART contract for runtime telemetry verbosity control.
Primary audience: automation/LLM agents. Secondary: humans.

## Goal

- Reduce runtime logging pressure (formatting + UART output) without reflashing.
- Enable only the diagnostic domains needed for the current investigation.
- Keep metrics counters (`METRICS*`) available regardless of log domain toggles.

## UART Contract

All commands use `UART0` at `115200` baud, CRLF terminated.

### Query Current Telemetry Mask

Command:

```text
TELEM
```

Alias:

```text
TELEM STATUS
```

Response format (single line):

```text
TELEM mask=0x<hex> wifi=<on|off> reassoc=<on|off> net=<on|off> http=<on|off> sd=<on|off>
```

Machine parse regex:

```text
^TELEM mask=0x([0-9a-fA-F]+) wifi=(on|off) reassoc=(on|off) net=(on|off) http=(on|off) sd=(on|off)$
```

### Set Telemetry Domains

Command family:

```text
TELEMSET <args>
```

Supported forms:

```text
TELEMSET DEFAULT
TELEMSET NONE
TELEMSET ALL
TELEMSET ALL ON
TELEMSET ALL OFF
TELEMSET WIFI ON|OFF
TELEMSET REASSOC ON|OFF
TELEMSET NET ON|OFF
TELEMSET HTTP ON|OFF
TELEMSET SD ON|OFF
```

Domain aliases:

- `REASSOC`: `SCAN`, `WIFI_SCAN`
- `NET`: `NETWORK`
- `SD`: `STORAGE`

Response format (single line):

```text
TELEMSET OK mask=0x<hex> wifi=<on|off> reassoc=<on|off> net=<on|off> http=<on|off> sd=<on|off>
```

Machine parse regex:

```text
^TELEMSET OK mask=0x([0-9a-fA-F]+) wifi=(on|off) reassoc=(on|off) net=(on|off) http=(on|off) sd=(on|off)$
```

On invalid syntax, firmware returns:

```text
CMD ERR
```

## Domain Semantics

- `wifi`: high-level station lifecycle logs (credentials/config/connect/disconnect/watchdog).
- `reassoc`: scan/reassociation internals (scan results, auth/channel rotations, disconnect reasons).
- `net`: DHCP/listener/accept-network-path logs.
- `http`: request-level HTTP logs (`/health`, request method/path, request errors, connection accepted).
- `sd`: SD operation logs emitted from HTTP handlers (for example mkdir traces).

## Recommended Profiles

### Minimal Pressure

```text
TELEMSET NONE
```

Use when running throughput or soak tests where logs are not needed.

### Wi-Fi Bring-up

```text
TELEMSET NONE
TELEMSET WIFI ON
TELEMSET NET ON
```

### Reassociation Debug

```text
TELEMSET NONE
TELEMSET WIFI ON
TELEMSET REASSOC ON
TELEMSET NET ON
```

### HTTP API Debug

```text
TELEMSET NONE
TELEMSET HTTP ON
TELEMSET NET ON
TELEMSET SD ON
```

## Agent Runbook (Deterministic)

1. Send `TELEMSET NONE`.
2. Send only required domain toggles for current task.
3. Send `TELEM` and assert expected on/off state.
4. Run test sequence (`MODE`, `WIFISET`, upload, etc).
5. Always collect `METRICSNET` and `METRICS` snapshots, independent of log toggles.
6. Restore default verbosity after debug:

```text
TELEMSET DEFAULT
```

## Notes

- Telemetry domain mask is runtime state (not persisted).
- `METRICS`/`METRICSNET` counters are not disabled by `TELEMSET`.
