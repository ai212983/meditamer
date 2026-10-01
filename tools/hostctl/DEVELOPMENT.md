# Hostctl workflow development

Run commands from the repository root. Put orchestration (retries, branching,
recovery order) in `tools/hostctl/scenarios/*.sw.yaml`; Rust actions perform
primitive I/O and assertions. Threshold-heavy strategy can use a small TOML
profile, leaving the graph in YAML.

Examples: [flash capture](scenarios/flash-capture.sw.yaml),
[troubleshooting](scenarios/troubleshoot.sw.yaml),
[network acceptance](scenarios/wifi-acceptance.sw.yaml).

## Runner contract

[src/scenarios/](src/scenarios/mod.rs) supports `call`, `do`, `set`, `switch`,
`try`/`catch`, task `if` guards, `then` transitions, and
`metadata.hostctl.retry`, `.repeat`, `.result`. Unsupported task kinds fail fast.
Conditions support literal/context comparisons (`==`, `!=`, `>`, `>=`, `<`, `<=`),
`&&`, `||`, `!`, `present(...)`, and `exists(...)`:

```text
.health_ok == true
.upload_attempt < 3
.upload_attempt <= .operation_retries
```

Every switch needs an explicit default branch. Give each action an explicit
input/output context contract and keep it atomic, preferably idempotent.
For example, split upload and verification, or burst start and assertion;
do not hide retry/recovery loops inside one action.

## State and errors

- Use repeat/set for workflow counters and gates.
- Return setup/status data through `invoke_with_result`, binding it with
  `metadata.hostctl.result`; `merge: true` merges objects and `path:` scopes data.
- For a failing action that must publish state, raise `WorkflowActionError` with
  a context patch. The engine merges the patch before catch/retry evaluation.
  Successful bookkeeping uses ordinary result binding instead.
- Use explicit fail actions with actionable messages.
- Command suites can use placeholders such as `{base_path}` and `{verify_lba}`;
  runtime resolves them before serial execution.

For flash, expose separate full/app-only, capture and post-command primitives;
YAML decides their order. Network readiness uses structured NET_STATUS frames,
not monitor-tail matching.

## Operator waits

Use a do task with `metadata.hostctl.repeat.while`, omitting `in`, for a
condition-controlled wait. It checks the condition before every iteration;
the body polls/waits for external progress and updates the condition.
There is no iteration expiry in this form. `each`/`at` require `in`.
Bounded repeats and task-map jump cycles retain their 4096-item/transition guards.

[Trace capture](scenarios/trace-capture.sw.yaml) waits for operator STOP or
buffer-full this way. Keep polling, branching and cleanup in YAML; device I/O
remains primitive calls.

## Add or refactor a workflow

1. Add/update the scenario YAML.
2. Implement primitive runtime actions and their context contracts.
3. Wire the command in `src/main.rs`. Add a shell entry point only for policy
   that belongs outside the Rust command.
4. Check action helpers and executable workflow branch/retry/cleanup behavior.

```bash
scripts/host-test.sh test hostctl
```
