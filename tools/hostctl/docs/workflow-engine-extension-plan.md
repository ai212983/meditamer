# Hostctl Workflow Engine Extension Plan

## Goal
Move more orchestration policy from Rust workflow runtimes into scenario YAML under
`tools/hostctl/scenarios/`, while keeping Rust focused on primitives:

- subprocess execution and watchdogs
- serial I/O and parsing
- artifact and path handling
- command-specific validation

## Current Engine Baseline

Current executable task surface:

- `call`
- `do`
- `set`
- `switch`
- `try`

Relevant implementation points:

- `tools/hostctl/src/scenarios/engine.rs`
- `tools/hostctl/src/scenarios/engine_support.rs`
- `tools/hostctl/src/scenarios/conditions.rs`

Current gaps that block YAML-first orchestration:

- `switch` has no explicit `default` or `end`.
- Condition syntax only supports simple comparisons.
- String interpolation only works for full-value expressions.

## Phase 1: Retry

### Objective
Make retry a workflow concern instead of a Rust runtime concern.

### Why This Phase First

- `troubleshoot` keeps flash/probe retry policy in Rust.
- `wifi-acceptance` keeps readiness recover-and-retry policy in Rust.
- This is the largest immediate reduction in orchestration code.

### Current Pressure Points

- `tools/hostctl/src/workflows/troubleshoot/runtime_steps.rs`
- `tools/hostctl/src/workflows/wifi/acceptance/runtime_core/network.rs`

### Steps

- [ ] Add executable support for `retry` on `call` tasks.
- [x] Add executable support for `catch.retry` on `try` tasks.
- [x] Define retry fields the engine will support initially.
  - `limit.attempt.count`
  - `delay`
  - optional `when`
  - optional `exceptWhen`
  - hostctl metadata overrides for retry count and delay
- [x] Bind the last error into workflow context before retry evaluation.
- [x] Add engine tests for:
  - [x] successful retry
  - [x] retry exhaustion
  - [x] conditional retry
  - [x] retry skipped because condition is false
- [x] Refactor `troubleshoot` flash retry from Rust into YAML.
- [ ] Refactor `troubleshoot` probe retry from Rust into YAML.
- [ ] Refactor Wi-Fi readiness retry orchestration from Rust into YAML where possible.

### Example Target Surface

```yaml
- flash_firmware:
    try:
      - flash_firmware_once:
          call: "flash_firmware_once"
    catch:
      as: "flash_error"
      retry:
        limit:
          attempt:
            count: 0
        delay:
          milliseconds: 0
    metadata:
      hostctl:
        retry:
          count: ".flash_retry_count"
          delayMs: ".flash_retry_delay_ms"
```

### Exit Criteria

- [x] `troubleshoot` no longer owns flash retry loops in `runtime_steps.rs`
- [x] YAML expresses retry count and retry order directly for `troubleshoot` flash
- [x] engine test coverage exists for retry behavior

## Phase 2: Loops

### Objective
Make repeated orchestration steps expressible in YAML without gate actions and counter-only helpers.

### Why This Phase Second

- multiple workflows emulate loops with counters plus gate actions
- these are sequencing concerns, not primitive action concerns

### Current Pressure Points

- `tools/hostctl/scenarios/runtime-modes-smoke.sw.yaml`
- `tools/hostctl/src/workflows/runtime_modes/runtime.rs`
- `tools/hostctl/scenarios/wifi-discovery-debug.sw.yaml`

### Steps

- [x] Choose the first loop surface to implement.
  Implemented as `metadata.hostctl.repeat` on `do` tasks because the upstream
  untagged parser does not reliably distinguish YAML `for` tasks from plain `do` tasks.
- [x] Support nested loop bodies using existing task maps.
- [x] Support loop-local context updates through `set`.
- [x] Add guardrails for runaway loops.
- [x] Add engine tests for:
  zero-iteration loop, fixed-count loop, condition-controlled loop, nested loop.
- [x] Remove `set_post_upload_status_gate` orchestration from `runtime_modes`.
- [x] Remove `set_post_upload_timeset_gate` orchestration from `runtime_modes`.
- [x] Replace counter-gate patterns in YAML with native loops.

### Example Target Surface

```yaml
- post_upload_status_loop:
    do:
      - run_status_probe:
          call: "state_get"
          with:
            expect_upload: "on"
    metadata:
      hostctl:
        repeat:
          in: ".post_upload_status_repeats"
          at: "post_upload_status_index"
```

### Exit Criteria

- [x] `runtime-modes-smoke` no longer uses gate-only actions for repeated probes
- [x] loop structure is visible directly in YAML
- [x] Rust no longer computes loop booleans just to drive workflow control flow

## Phase 3: Call Result Binding

### Objective
Let workflow tasks capture action outputs into context instead of forcing actions to mutate context directly.

### Why This Phase Third

- current action contracts are biased toward side effects
- that keeps orchestration glue in Rust

### Current Pressure Points

- `tools/hostctl/src/workflows/runtime_modes/runtime.rs`
- `tools/hostctl/src/workflows/troubleshoot/runtime_core.rs`
- `tools/hostctl/src/workflows/wifi/common/context.rs`

### Steps

- [x] Extend `WorkflowRuntime` action handling to support structured action output.
- [x] Add engine support for binding returned values to a context path.
- [x] Decide whether binding semantics should:
  support both replacement and merge through `metadata.hostctl.result`.
- [x] Add tests for:
  scalar result binding, object result binding, nested-path result binding, merge behavior.
- [x] Refactor one existing runtime to use result binding instead of direct `ctx_set_*`.
- [ ] Refactor additional workflows only after the binding contract is stable.

### Example Target Surface

```yaml
- state_probe:
    call: "init_post_upload_checks"
    metadata:
      hostctl:
        result:
          merge: true
```

### Exit Criteria

- [x] at least one workflow uses returned action data instead of action-side context mutation
- [x] orchestration-only `ctx_set_*` usage is reduced

## Phase 4: Explicit Switch Semantics

### Objective

Remove implicit task-order dependence from scenario YAML.

### Why This Phase Matters

- current no-match behavior falls through to the next task
- this forces ordering hacks in YAML

### Current Pressure Point

- `tools/hostctl/scenarios/wifi-discovery-debug.sw.yaml`

### Steps

- [ ] Add explicit `default` branch support for `switch`.
- [ ] Add explicit `end` semantics or equivalent no-op terminal behavior.
- [ ] Preserve backward compatibility for existing workflows during migration.
- [ ] Add tests for:
  - matching branch
  - default branch
  - no-match end behavior
  - legacy fallthrough compatibility, if retained temporarily
- [ ] Remove order-dependent comments and layouts from existing scenario YAMLs.

### Example Target Surface

```yaml
- result_gate:
    switch:
      - fail:
          when: "${ .run_passed != true }"
          then: "fail_run"
      - pass:
          then: "done"
```

### Exit Criteria

- [ ] `wifi-discovery-debug` no longer depends on task ordering to avoid failure on success
- [ ] switch behavior is explicit in YAML

## Phase 5: Richer Expressions

### Objective

Reduce synthetic context flags by making YAML conditions and values more expressive.

### Why This Phase Comes Last

- it is useful, but less immediately valuable than retry and loop support
- expression surface area increases engine complexity fastest

### Current Pressure Points

- `tools/hostctl/src/scenarios/conditions.rs`
- `tools/hostctl/src/scenarios/engine_support.rs`

### Steps

- [ ] Add boolean operators:
  - `&&`
  - `||`
  - `!`
- [ ] Add null/presence checks.
- [ ] Add simple arithmetic for counters.
- [ ] Add string interpolation inside larger strings.
- [ ] Add tests for precedence, nesting, and mixed path/literal expressions.
- [ ] Remove boolean scratch fields that only exist to satisfy current expression limits.

### Exit Criteria

- [ ] scenario conditions express combined gates directly
- [ ] workflows need fewer temporary booleans and counters

## Refactor Order After Engine Work

- [ ] Refactor `runtime-modes-smoke` first.
  - remove repeated probe gate actions
- [ ] Refactor `troubleshoot` second.
  - move flash/probe/soak retry policy into YAML
- [ ] Refactor `wifi-acceptance` third.
  - move readiness retry and fallback orchestration into YAML
- [ ] Refactor `wifi-discovery-debug` fourth.
  - remove switch-order dependence and simplify round flow

## What Stays in Rust

Keep these in Rust primitives:

- serial polling loops
- subprocess supervision and watchdogs
- esptool/espflash command construction
- HTTP transfer internals
- log parsing and device-output classification

## Final Success Criteria

- [ ] scenario YAMLs own retry order and loop structure
- [ ] Rust runtimes mostly dispatch primitive actions
- [ ] workflow-specific orchestration glue shrinks materially
- [ ] YAML no longer depends on implicit fallthrough behavior
- [ ] hostctl behavior remains covered by engine and workflow tests
