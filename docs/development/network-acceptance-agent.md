# Network Acceptance Workflow (Agent-First)

Hard-cut Wi-Fi/network acceptance runs are executed via:

```bash
scripts/test_wifi_acceptance.sh
```

which invokes:

```bash
hostctl test wifi-acceptance
```

with declarative orchestration from:

```text
tools/hostctl/scenarios/wifi-acceptance.sw.yaml
```

## Required Env Contract

- `HOSTCTL_NET_PORT`
- `HOSTCTL_NET_BAUD`
- `HOSTCTL_NET_SSID`
- `HOSTCTL_NET_PASSWORD`
- `HOSTCTL_NET_POLICY_PATH`
- `HOSTCTL_NET_LOG_PATH`

Default policy template:

- `tools/hostctl/scenarios/wifi-policy.default.json`

## UART Contract (Hard Cut)

- `NETCFG SET <json>`
- `NETCFG GET`
- `NET START`
- `NET STOP`
- `NET STATUS`
- `NET RECOVER`

Readiness and failure diagnosis must use structured lines:

- `NET_STATUS {...}`
- `NET_EVENT {...}`

## Deterministic Agent Procedure

1. Set all required `HOSTCTL_NET_*` variables explicitly.
2. Run `scripts/test_wifi_acceptance.sh`.
3. On failure, attach the `HOSTCTL_NET_LOG_PATH` artifact.
4. Classify from `NET_STATUS.failure_class` / `failure_code` first, then from HTTP upload errors.
5. Apply one targeted fix, rerun, and compare cycle summaries.

## Acceptance Gate

1. 1-cycle bounded smoke pass.
2. 3-cycle acceptance pass.
3. 24h bounded soak with:
- zero host reset fallback
- zero unclassified failure class
- deterministic summary.
