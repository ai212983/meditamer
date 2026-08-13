# Hostctl Workflow Authoring

This project runs host instrumentation tests via declarative Serverless Workflow
YAML files under:

- `tools/hostctl/scenarios/*.sw.yaml`

The `hostctl` Rust code should expose primitive actions, while orchestration
(retry loops, branch gates, recovery order) should live in YAML.

Current examples:

- `tools/hostctl/scenarios/flash-capture.sw.yaml`
- `tools/hostctl/scenarios/troubleshoot.sw.yaml`
- `tools/hostctl/scenarios/wifi-acceptance.sw.yaml`

## Current Runner Scope

`tools/hostctl/src/scenarios.rs` currently supports:

- `call`
- `do`
- `set`
- `switch`
- `try` / `catch`
- `if` guards on tasks
- `then` transitions
- `metadata.hostctl.retry`
- `metadata.hostctl.repeat`
- `metadata.hostctl.result`
- structured error context patches via `WorkflowActionError`

Condition parser supports comparisons against literals and context paths:

- `==`, `!=`, `>`, `>=`, `<`, `<=`
- `&&`, `||`, `!`, `present(...)`, `exists(...)`
- examples: `.health_ok == true`, `.upload_attempt < 3`, `.upload_attempt <= .operation_retries`

Unsupported DSL task kinds currently fail fast.

## Authoring Pattern

1. Keep actions primitive and idempotent where practical.
- Good: `upload_once`, `verify_upload`, `recover_listener_flap`.
- Avoid: a single action that contains nested retry/recovery loops.

2. Keep strategy in YAML.
- Model retries and branch flow with `switch`, `try`, `metadata.hostctl.retry`, and `metadata.hostctl.repeat`.
- Store counters in context (`health_attempt`, `upload_attempt`) or return them through `metadata.hostctl.result`.
- Prefer this split even for flash flows: Rust should provide primitives like
  `flash_full`, `flash_app_only`, `capture_boot`, `capture_stream`, and
  `capture` and `post_command`, while the YAML decides when they run.

3. Use TOML for workflow-specific strategy profiles when thresholds become large.
- Keep orchestration graph in `*.sw.yaml`.
- Keep tuneable thresholds (round counts, timeout budgets, pass/fail gates) in a
  small TOML profile file loaded by runtime actions.

4. Use explicit fail actions.
- Example: `fail_health`, `fail_upload` should emit final actionable error text.

5. Keep context contract explicit.
- For each action, define inputs and outputs (which context keys it reads/writes).
- Every `switch` must define an explicit `default` branch. Do not rely on
  implicit fallthrough or an unlabeled catch-all case.

## Data Patterns

1. Counter/gate loops in YAML.
- Prefer `metadata.hostctl.repeat` and `set` for workflow-owned loops.

2. Template variables for command-heavy suites.
- Workflows like `sdcard-hw` pass command strings with placeholders
  (`{base_path}`, `{file_a}`, `{verify_lba}`).
- Runtime resolves placeholders before serial command execution.

3. Result binding for setup/status actions.
- If an action's main job is to produce context state, return structured data
  from `invoke_with_result` and bind it with `metadata.hostctl.result`.
- Use `merge: true` for status objects and `path:` for nested/scoped data.

4. Error context patches for failing actions.
- If an action must fail and also publish workflow-visible state, raise
  `WorkflowActionError` with a context patch instead of mutating context first.
- The engine merges that patch before `catch` / retry condition evaluation, so
  YAML can branch on fields like `failure_class` or `flash_ok` even on errors.
- Keep this for true failure-path state only. Use normal result binding for
  successful setup, status, and bookkeeping outputs.

5. Keep each step atomic.
- `run_step` handles one command/ack/SDREQ/SDWAIT assertion.
- Burst tests split into `burst_batch_start` and `burst_batch_assert`.

## How To Add/Refactor A Workflow

1. Create or update a scenario YAML in `tools/hostctl/scenarios/`.
2. Implement matching primitive actions in the runtime `invoke` match arm.
3. Wire the command in `tools/hostctl/src/main.rs`; add a public shell entry point
   only when the workflow needs policy that does not belong in the Rust command.
4. Add/adjust tests:
- unit tests for action helpers
- workflow execution tests for branch/retry behavior

## Validate Host Workflows

Use host-only validation path (avoids embedded default target/toolchain issues):

```bash
scripts/host-test.sh test hostctl
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
