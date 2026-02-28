# Hostctl Workflow Authoring

This project runs host instrumentation tests via declarative Serverless Workflow
YAML files under:

- `tools/hostctl/scenarios/*.sw.yaml`

The `hostctl` Rust code should expose primitive actions, while orchestration
(retry loops, branch gates, recovery order) should live in YAML.

## Current Runner Scope

`tools/hostctl/src/scenarios.rs` currently supports:

- `call`
- `do`
- `switch`
- `if` guards on tasks
- `then` transitions

Condition parser supports comparisons against literals and context paths:

- `==`, `!=`, `>`, `>=`, `<`, `<=`
- examples: `.health_ok == true`, `.upload_attempt < 3`, `.upload_attempt <= .operation_retries`

Unsupported DSL task kinds currently fail fast.

## Authoring Pattern

1. Keep actions primitive and idempotent where practical.
- Good: `upload_once`, `verify_upload`, `recover_listener_flap`.
- Avoid: a single action that contains nested retry/recovery loops.

2. Keep strategy in YAML.
- Model retries and branch flow with `switch` + context flags/counters.
- Store counters in context (`health_attempt`, `upload_attempt`).

3. Use TOML for workflow-specific strategy profiles when thresholds become large.
- Keep orchestration graph in `*.sw.yaml`.
- Keep tuneable thresholds (round counts, timeout budgets, pass/fail gates) in a
  small TOML profile file loaded by runtime actions.

4. Use explicit fail actions.
- Example: `fail_health`, `fail_upload` should emit final actionable error text.

5. Keep context contract explicit.
- For each action, define inputs and outputs (which context keys it reads/writes).

## Data Patterns

1. Counter/gate loops in YAML.
- Runtime writes counters/flags into context (`post_upload_status_index`,
  `run_post_upload_status_probe`), and YAML loops with `switch`.

2. Template variables for command-heavy suites.
- Workflows like `sdcard-hw` pass command strings with placeholders
  (`{base_path}`, `{file_a}`, `{verify_lba}`).
- Runtime resolves placeholders before serial command execution.

3. Keep each step atomic.
- `run_step` handles one command/ack/SDREQ/SDWAIT assertion.
- Burst tests split into `burst_batch_start` and `burst_batch_assert`.

## How To Add/Refactor A Workflow

1. Create or update a scenario YAML in `tools/hostctl/scenarios/`.
2. Implement matching primitive actions in the runtime `invoke` match arm.
3. Wire the command in `tools/hostctl/src/main.rs` and a thin shell wrapper in `scripts/`.
4. Add/adjust tests:
- unit tests for action helpers
- workflow execution tests for branch/retry behavior

## Validate Host Workflows

Use host-only validation path (avoids embedded default target/toolchain issues):

```bash
scripts/tests/host/test_hostctl_host.sh
```

## Running

Example:

```bash
scripts/tests/hw/test_wifi_acceptance.sh
```

This executes:

```bash
hostctl test wifi-acceptance
```

with orchestration from `tools/hostctl/scenarios/wifi-acceptance.sw.yaml`.
For network workflows, gate progression on structured firmware lines (`NET_STATUS {...}`), not ad-hoc monitor-tail text matching.
