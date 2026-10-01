# Inkplate buzzer diagnostics and qualification

- Status: Active; bell tuning parked
- Last-reviewed: 2026-10-01

## Goal and existing baseline

Finish repeatable diagnostics and qualify reliable playback on Inkplate 4
TEMPERA. Mapping repair, transport failure handling, the shared sequencer,
firmware score runner, nominal WAV preview and host-only RTTTL compiler are
implemented. The standalone probe has completed notification and melody
playback; a familiar melody was recognized with clear pitches and rhythm.
Notification pleasantness and preset selection remain open. No bell preset
is selected, and isolated playback does not qualify production timing or sleep.

Hardware facts belong in the [sound reference](../references/hardware/inkplate/sound.md).
Software contracts are documented with the [board driver](../../boards/inkplate-tempera/src/buzzer.rs),
[runner](../../platform/audio/buzzer/src/runner.rs) and
[RTTTL compiler](../../tools/buzzer_preview/src/rtttl.rs).
The [listening guide](../guides/audio/buzzer-listening.md) owns authoring and playback.

## Remaining work

### 1. Finish repeatable diagnostics

Wire the existing [bounded diagnostic operations](../../platform/audio/buzzer/src/diag.rs)
into a serial service with one owner of board control and the shared expander
cache. Support raw codes, powered pitch changes, bursts, power cycles, sweeps,
short scores and explicit stop. Validate inputs before playback and report
requested/completed timestamps, codes and errors.

Implement the matching hostctl primitives beneath the pending
[buzzer diagnostic scenario](../../tools/hostctl/scenarios/buzzer-diagnostic.sw.yaml).
Keep orchestration and capture flow in workflow YAML. The existing boot-time
probe remains sufficient for short listening comparisons; serial diagnostics
must enable repeated measurements without reflashing each candidate.

### 2. Measure startup, rests and pitch

Start with codes 0, 5, 63 and 127 plus several mapped requests. Identify the
fundamental independently of the strongest acoustic harmonic. Measure power-on
settling/default pitch, the first successful pitch write, power-off behaviour,
and code restoration after short and long rests. Retain the current 1 ms
startup wait until measurements justify changing it.

Use electrical capture where accessible and acoustic recordings with fixed
gain/geometry and AGC disabled. Keep board identity, build, settings and raw
captures with repo-relative artifact paths. Expand to an all-code survey or a
fitted acoustic preview only when a specific unresolved question requires it.

### 3. Qualify runtime behaviour

- Measure deadline lateness and touch/IMU scheduling with panel refresh active.
  Set supported playback limits from the results. Rapid powered switching at
  50/100/200/400 updates per second is experimental until qualified.
- Exercise explicit stop, cooperative cancellation and recoverable faults,
  including failed shutdown reporting. Resolve a shutdown fault before sleep
  or reporting successful completion; do not abandon a playing future.
- Qualify sleep coordination, current draw and whole-device energy per short
  score, including wake when applicable.
- Before adding production tasks, buffers or channels, budget and measure them
  in each supported profile using the
  [DRAM budget](../references/memory/meditamer-inkplate/budget.md).

### 4. Resolve notification selection

Assess the existing bounded notification candidates for pleasantness and
quiet-room usefulness. Select at most two static presets, or record that none
is suitable. Keep operator preference separate from measured pitch, execution
success and runtime qualification. Product UI integration is separate work.

## Parked scope

Bell approximation, rail-gated decay and pitch-multiplexing tuning remain
parked after unsuccessful listening comparisons. Do not restart speculative
parameter sweeps. Reopening requires a specific unresolved question supported
by new acoustic/electrical evidence or an explicit operator request.

Use ordinary notes/rests for notification work. Specialized gestures, runtime
RTTTL parsing, hardware changes, GPIO waveform generation and other-board audio
are deferred. Bell quality is not a completion gate for reliable playback.

## Completion

Close when serial diagnostics and hostctl can repeat the measurement workflow,
startup/rest behaviour is measured, supported timing and shutdown/sleep are
qualified, and notification selection has an explicit disposition. Update
hardware measurements in the sound reference, software behaviour in module/API
documentation and operating steps in the guide. Run appropriate host, target
and memory checks for implementation changes, plus Markdown link and LOC checks.
