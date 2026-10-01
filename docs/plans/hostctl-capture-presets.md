Status: Proposed
Last-reviewed: 2026-09-30

# Hostctl capture presets

## Goal

Run repeatable serial measurements by choosing a preset and supplying parameters.
Hostctl owns serial handling, deadlines, operator signals, and run evidence.
Serverless Workflow YAML owns the order of actions, branches, and cleanup.

The motivating IMU capture script repeats attachment, readiness, START/STOP,
counter reset, ACQWINDOW, metrics collection, and scheduler restoration. Its
renewal is scheduled at the capture's end, STOP applies only before recording,
and pattern waits discard other events. Move these responsibilities into tested
host tooling so each experiment supplies its measurement intent.

## Existing foundation

| Component | Reuse |
| --- | --- |
| [SerialConsole](../../tools/hostctl/src/serial_console/mod.rs) | Retained lines, receive marks, command waits, passive attachment, complete Inkplate metrics dumps |
| [Scenario runner](../../tools/hostctl/src/scenarios/mod.rs) | YAML loading, context, calls, branches, repeat, and error handling |
| [Trace capture](../../tools/hostctl/src/workflows/trace_capture.rs) | Fresh output directory, port lock, operator markers, separate cleanup errors |
| [Multicore capture](../../tools/hostctl/src/workflows/thermal_aba.rs) | Scheduler, acquisition, counter reset, and operator actions |
| [Launcher](../../scripts/hostctl.sh) | Host build environment and port selection |

The current CLI selects an experiment-specific Rust runtime and its YAML file.
Add a shared capture runtime using the existing runner. Follow the
[workflow authoring guide](../../tools/hostctl/DEVELOPMENT.md) for strategy
placement and condition-controlled operator loops.

## Preset storage and loading

Store defaults in `tools/hostctl/presets/<name>.toml` and sequences in
`tools/hostctl/scenarios/<name>.sw.yaml`. Several profiles may share a workflow.
Use typed Rust configuration and the existing TOML/YAML dependencies.

First preset, `tools/hostctl/presets/imu-rebase.toml`:

```toml
version = 1
description = "Capture IMU timing with sustained acquisition demand"
workflow = "scenarios/imu-rebase.sw.yaml"

[defaults]
scheduler = "interactive"
seconds = 20
operator_start = true
ready_timeout_ms = 65000
operator_timeout_ms = 600000
command_timeout_ms = 3000
metrics_timeout_ms = 15000
renew_every_ms = 20000
renew_margin_ms = 2000
poll_max_ms = 100
```

These files and command forms are proposed additions:

```bash
scripts/hostctl.sh capture list
scripts/hostctl.sh capture imu-rebase --seconds 40 --check
scripts/hostctl.sh capture imu-rebase --seconds 40 --output logs/imu-rebase/run-01
```

Resolve names from the preset directory. Resolve `workflow` from
`tools/hostctl/`, with the selected file contained in `scenarios/`.
Resolve output paths from the repository root, matching the launcher.
Apply explicitly supplied CLI values over TOML defaults; expose scheduler,
duration, and `--operator-start` / `--immediate` overrides.

Loading produces typed effective parameters and the parsed workflow. Validate
the metadata, parameter ranges, supported tasks, action arguments,
context references, and transition targets before creating artifacts or opening
the port. Reject unknown fields and unsupported actions with their file and
field/task name. `--check` prints the effective parameters and validation result;
`list` reads preset names and descriptions through the same loader.

## Runtime contract

Use one serial connection and one receive buffer for the whole run. Reuse
SerialConsole's line handling and port lock. Add cancellation and clock access
at this shared boundary so every capture action uses the same deadlines.
Bound each polling turn by `poll_max_ms`, including under continuous logging.

| Action | Inputs and result |
| --- | --- |
| `wait_ready` | Readiness deadline; PING and current SCHEDPROFILE status establish `runtime_ready=on`; returns the initial scheduler override |
| `set_scheduler` | Requested profile; validates active/override fields and records that restoration may be needed before sending |
| `operator_poll` | START/STOP and operator deadline; receives serial data and returns start/cancel state |
| `reset_counters` | TOUCHSCHEDRESET; waits for its acknowledgement |
| `acquire_window` | ACQWINDOW; validates `active_ms=30000` and returns send time, acknowledgement time, and conservative lease deadline |
| `begin_recording` | Duration; records start/end deadlines and publishes RECORDING |
| `observe_tick` | Recording and lease deadlines; receives data and returns completed, cancelled, renewal-due, or lease-expired state |
| `collect_metrics` | Dump deadline and required record types; returns a complete dump and extracted records |
| `restore_scheduler` | Saved override; sends AUTO or the saved profile and verifies the returned override |
| `finish` | Capture and cleanup outcomes; writes final artifacts and sets exit status |

Each action declares typed arguments and named context outputs. Use the runner's
existing context-value resolution and result binding. Preflight walks nested
tasks and validates arguments and references against a small declared capture
context. Runtime fields are optional until their action completes; consuming
actions check required fields. Fake-runtime tests exercise branches and cleanup.

Command transactions keep one request in flight, establish a receive mark, and
inspect success and error responses while retaining unrelated lines. Readiness
uses current status as well as retained events. Match scheduler responses to the
requested fields. Receive marks track host observations; the current protocol
has no general request IDs. After an ambiguous timeout, fail the measurement and
record uncertain command state; YAML chooses the cleanup path. Retry decisions
remain explicit in the workflow and use each command's completion contract.

Metrics collection reuses the complete-dump helper: wait for NET_ACCEPT, then
select all required records from that dump's mark. Missing records, BUSY, ERR,
and an expired dump deadline produce distinct failures. Preserve dump evidence
on failure. The records represent one completed request; individual firmware
counters may be sampled at different times during that request.

## First workflow and timing

The IMU workflow performs these steps:

1. Establish readiness and save the scheduler override.
2. Publish READY; a condition-controlled YAML loop waits for START when enabled.
3. Select the requested scheduler and reset counters.
4. Acquire a window; its acknowledgement also fences the preceding reset,
   since the firmware processes these commands in order.
5. Publish RECORDING and observe until the recording deadline.
6. Collect IMU_DEADLINE and ACQUISITION kind=imu from one complete metrics dump.
7. Restore the saved scheduler override and write the result.

During observation, YAML repeats `observe_tick` and branches to
`acquire_window` when renewal is due. Check demand coverage before accepting
completion. Renew only when the current lease ends before the recording deadline;
check completion before sending another request.
The Rust actions track time and device lease facts; YAML selects the renewal
interval, command order, and error branches. Use absolute monotonic deadlines
so command time counts toward the recording duration.

The firmware publishes a demand deadline 30 seconds ahead before sending the ACK;
the IMU task consumes that demand later. Track conservative expiry from command
send time. Host timing validates acknowledged requests covering the recording
interval; sampling behavior uses the preset's metrics and device qualification.
Preflight requires positive durations and:

```text
renew_every_ms + command_timeout_ms + poll_max_ms + renew_margin_ms < 30000
```

At runtime, cap each renewal wait by the remaining lease budget. A late renewal
or host scheduling pause that exhausts the margin fails the active-demand
measurement. Record scheduled and actual send/ACK times. The recording deadline
also bounds renewal waits. Metrics collection follows the observation period.
Any remaining acquisition demand expires through the firmware's finite window;
record its conservative expiry in the result.

## Operator control, cleanup, and evidence

Create a fresh output directory atomically and hold the port lock through cleanup.
READY means setup can accept START; RECORDING means measurement setup completed.
The operator begins physical input after RECORDING. START/STOP files apply only
to that run's directory. Publish an atomic `status.json` for current state.

```bash
touch logs/imu-rebase/run-01/START
# Wait for RECORDING before physical input; STOP ends the run early.
touch logs/imu-rebase/run-01/STOP
```

STOP, SIGINT, and SIGTERM request cancellation during readiness, operator waits,
commands, observation, and metrics collection. YAML catches cancellation and
enters cleanup; cleanup uses its own bounded deadline while serial reception
continues. The latched cancellation request stays visible while cleanup proceeds.
Keep capture and restore errors separately. Mark possible scheduler
mutation before sending so lost acknowledgements still lead to restoration.
Restoration runs after success, cancellation, or failure whenever the initial
override was saved and a mutation was attempted. Process termination by SIGKILL
or host failure leaves evidence incomplete; record progress throughout the run.

| Artifact | Contents |
| --- | --- |
| `serial.log` | Received normalized firmware lines, using SerialConsole's existing format |
| `events.jsonl` | Receive line numbers and timestamps, command sends/results, timer events, phase changes, and errors |
| `preset.toml`, `workflow.sw.yaml`, `effective.json` | Loaded sources and resolved parameters, plus hostctl revision/build identity |
| `status.json` | Run ID, phase, elapsed time, deadline, and cancellation state |
| `metrics.json` | Required records from the completed dump, with source line numbers |
| `result.json` | Complete/cancelled/failed outcome, actual duration, lease timing, and restoration outcome |

Use elapsed monotonic time for scheduling and UTC for human-readable timestamps.
Flush serial and event evidence as it arrives. A successful capture requires
the requested duration, required records, acknowledged demand coverage, and verified
restoration. Measurement quality assertions remain specific to each preset.

## Delivery and validation

1. Add preset loading, CLI overrides, action contracts, and host-only preflight.
   Test parameter precedence, paths, unsupported tasks, and nested workflows.
2. Extend the launcher's build-and-exec path to capture commands so SIGINT/SIGTERM
   reach the process owning the serial port. Extract shared serial/deadline/operator
   primitives and add the IMU workflow.
   Use fake serial input and an injected clock to cover interleaved readiness,
   fragmented lines, slow ACKs, complete dumps, continuous logs, renewal near
   expiry and the recording boundary, STOP in each phase, and lost mutation ACKs.
   Verify YAML cleanup paths and distinct capture/restore failures,
   including cancellation during cleanup.
3. Run `scripts/host-test.sh test hostctl`. Validate the new preset with
   `--check`, then capture on the attached Inkplate with immediate and operator
   start, 20-second and longer-than-30-second windows, and an operator STOP.
   Send SIGTERM to the public launcher and verify bounded cleanup and process exit.
   Compare configured duration with event times and inspect restoration status.
4. Update the existing workflow authoring guide with preset authoring and usage.
   Reuse shared helpers in trace/multicore captures where behavior matches,
   retaining their experiment-specific qualification and artifact contracts.
   Run their host regression tests after each migration.

Validate documentation with
`scripts/ci/check_markdown_links.sh docs/plans/hostctl-capture-presets.md` and
the Markdown LOC advisory. Device results cover
serial orchestration; physical input behavior uses its own qualification gates.
