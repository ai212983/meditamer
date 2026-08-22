# Guides

How you *work on* Meditamer: set up a host, build, flash, drive the device over
UART, and prove a change did not break anything. How the system *is* lives in
[`reference/`](../reference/); why it is that way lives in
[`architecture/`](../architecture/).

Each topic below has exactly one owner. If two guides describe the same
workflow, one of them is wrong — fix the duplicate, do not maintain both.

## Start here

| I want to… | Read |
| --- | --- |
| Set up a host and the git hooks | [development-setup.md](development-setup.md) |
| Build, flash, or attach a monitor | [build-and-flash.md](build-and-flash.md) |
| Install a signed image on a device | [firmware-update.md](firmware-update.md) |
| Work out why the board is broken | [troubleshooting.md](troubleshooting.md) |
| Change what the device is doing, over UART | [service-modes.md](service-modes.md) |
| Read counters, timing, or run a hardware test | [runtime-metrics.md](runtime-metrics.md) |
| Push assets to the SD card over Wi-Fi | [wifi-asset-upload.md](wifi-asset-upload.md) |
| Land a Wi-Fi, network, or upload change | [wifi-regression-gate.md](wifi-regression-gate.md) |
| Add or edit a hostctl workflow | [hostctl-workflow-authoring.md](hostctl-workflow-authoring.md) |

## By topic

**Host setup** — [development-setup.md](development-setup.md): toolchain,
`lefthook` hooks, lint/coverage/SonarQube commands, the rust-analyzer baseline,
and `logs/` artifact cleanup.

**Device workflow** — [build-and-flash.md](build-and-flash.md) covers the
day-to-day loop (build profiles, `scripts/device/flash.sh`, port selection,
flash troubleshooting, monitor, defmt).
[firmware-update.md](firmware-update.md) covers the ADR-0014 factory updater:
complete USB flash, bundle build/sign/inspect, SD staging, updater status lines,
and ROM recovery.

**Runtime control and observation** — [service-modes.md](service-modes.md) owns
the commands that *change* state (`STATE`, `DIAG`, `PSRAM`, `PSRAMALLOC`).
[runtime-metrics.md](runtime-metrics.md) owns the ones that *read* it
(`METRICS`, `SCHEDPROFILE`, `TELEM`) plus the SD-card hardware test.

**Wi-Fi and upload** — [wifi-asset-upload.md](wifi-asset-upload.md) is the
feature: the HTTP upload server, `NET*`/`NETCFG` provisioning, credentials, and
the `hostctl upload` client. [wifi-regression-gate.md](wifi-regression-gate.md)
is the gate: discovery debug, the panic-first regression run, and every
`HOSTCTL_NET_*` guardrail. Run the gate before landing Wi-Fi, network, or
upload changes.

**Failure investigation** — [troubleshooting.md](troubleshooting.md) owns the
`hostctl test troubleshoot` workflow and its env knobs, boot/refresh soak
scripts, and the UI-lifecycle evidence run.

**Tooling** — [hostctl-workflow-authoring.md](hostctl-workflow-authoring.md):
the Serverless Workflow DSL subset the runner supports and the rules for
splitting strategy (YAML) from primitives (Rust).

## `agents/`

Runbooks written for automation and LLM agents rather than humans:
deterministic procedures, machine-parse regexes, failure classifications, and
required reporting fields. They are a separate audience, not a separate topic —
each one names the human guide that owns the surrounding workflow.

| Runbook | Contract it pins down |
| --- | --- |
| [network-acceptance.md](agents/network-acceptance.md) | `HOSTCTL_NET_*` env contract and the `NET`/`NETCFG` UART commands for acceptance runs |
| [telemetry-control.md](agents/telemetry-control.md) | `TELEM`/`TELEMSET` command family, response regexes, and domain profiles |
| [troubleshoot.md](agents/troubleshoot.md) | Preconditions, failure-class triage map, and required agent reporting fields |
