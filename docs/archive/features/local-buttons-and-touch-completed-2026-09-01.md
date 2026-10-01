# Local buttons and touch

- Status: Done — implementation, automated checks, and both physical-device sessions passed
- Last-reviewed: 2026-09-01
- Archived: 2026-09-01
- Contract and UI ownership: [UI runtime finalization](ui-runtime-finalization.md)

## Scope

Integrate Waveshare KEY/BOOT and Inkplate WAKE/touch with the existing UI owner.
This plan owns local source adapters, product bindings and their target acceptance.
BLE devices have a [separate plan](ble/02-device-drivers.md); local input work and
qualification proceed independently of BLE readiness.

Use the shared contract's independent press/release edges, optional recognition
and capture/cancellation rules. Touch retains Down/Move/Up/Cancel for consumers
needing contact edges without tap or gesture recognition. Reuse existing tasks and
delivery paths; preserve acquisition, recognition timing and current actions.

## Existing sources and behavior to preserve

| Source | Existing path | Integration requirement |
| --- | --- | --- |
| Waveshare KEY/BOOT | [Recognizer and capture gate](../../products/medinote/src/input.rs), [runtime](../../targets/medinote-waveshare/src/runtime_ui/mod.rs) | Keep actions on Pressed, including navigation, Hourglass rotation and BOOT sleep. Product policy owns bindings; the target retains GPIO sampling and sleep execution. |
| Inkplate WAKE | [GPIO36 classifier](../../products/meditamer/src/firmware/input/gpio36.rs), [frontlight action](../../products/meditamer/src/firmware/display/gpio36_feedback.rs) | Route accepted press/release events and retain frontlight behavior. GPIO36 remains jointly classified with touch; ambiguous assertions are not reliable button edges. |
| Inkplate touch | [Pipeline](../../products/meditamer/src/firmware/touch/tasks/pipeline.rs), [LVGL intake](../../products/meditamer/src/firmware/ui/lvgl/backend/frame.rs) | Preserve individual Down/Up delivery to LVGL, cancellation and multitouch. Contact transitions remain usable independently of gesture actions. |

KEY wake remains suppressed until release; BOOT retains its ROM download behavior.
Define WAKE cancellation when touch acquisition faults or is suspended, retaining
GPIO36 classification rather than adding an independent sampler. New button gestures
or Settings shortcuts are separate product decisions; existing bindings remain.

## Progress

Implementation and automated checks are complete:

- Medinote owns the host-tested recognizer, capture gate and KEY/BOOT policy. Capture loss
  cancels held input without a synthetic release or click and suppresses it until physical release.
- Inkplate routes WAKE through the existing GPIO36 classifier and frontlight owner. Touch
  acquisition suspension and faults cancel pending classification.
- Touch preserves contact edges without replaying recognizer actions into LVGL. Exclusive modal
  capture resets the pointer input device so an in-flight press cannot leak through the new modal.
- Host tests, touch replay, focused lint and both target release builds passed. Shared evidence is
  recorded in [hardware-test-matrix §2K](../reference/hardware-test-matrix.md#2k-ui-runtime-finalization----host-and-build-evidence-only-no-physical-session-yet).

## Closure

- Medinote artifact SHA-256
  `1221304f605a3a77846ee4646375c01f37e153e5bb6cc53ba6d2f46a82875721` passed physical
  KEY/BOOT actions, capture across a held key, deep-timer sleep/wake, and fresh post-wake input.
- Inkplate artifact SHA-256
  `9a1bb22e98df27e058e993fcc3410e3475dccbed449ae63e06d72b7d671f075d` passed WAKE/touch
  classification and modal capture. Qualification exposed a separate binary full-refresh row-band
  failure after a successful partial update.
- The repaired Inkplate artifact SHA-256
  `384c94b0c9ccb9c7ac7da98fc0f7e9e260b0c0e6883ff5d5b9aed1037d6d7810` restores the
  qualified 12-cycle binary-full source-clock hold and 1 us row margin. One initial and five repeated
  physical partial-to-full clock-overlay cycles remained clean; partial timing stays at 48 cycles.
- Final source formatting changed embedded line metadata, producing ELF
  `952639f2f413b51d88b679ed0ced69c8b64b717d94e7fe205a25c3abbe01eade` and app image
  `a783d26b129362d2c6003e3bfdb35271039913f316ab82a7eea1cdae32099daf`. That exact final
  artifact passed a further clean partial-to-full physical cycle.
- Serial logs, the failure photograph, and exact firmware artifacts are retained under
  `logs/medinote_phase8_local_input_20260901/`,
  `logs/inkplate_phase8_row_band_recovery_20260901/`, and
  `logs/inkplate_phase8_full_after_partial_fix_20260901/`. Final-artifact evidence is in
  `logs/inkplate_phase8_full_after_partial_final_20260901/`.

The broader UI finalization plan remains active for its non-local-input Phase 8 work. BLE device
qualification remains independently owned by its driver plan.

## Acceptance

- Host tests cover button press/release observed across the debounce samples, long
  holds, edge-only consumers, optional recognition without duplicate actions, and
  touch Down/Move/Up/Cancel without tap recognition. The current 30 Hz level sampler
  cannot promise delivery of a pulse that begins and ends between samples.
- Exercise modal entry/dismissal during a hold, surface removal, stale input,
  overflow, acquisition fault and sleep/reset. Verify cancellation without stuck
  controls, inherited releases, synthetic clicks or actions leaking to an underlying screen.
- Replay existing touch fixtures and retain touch/LVGL edge delivery and multitouch
  cancellation checks alongside product binding tests.
- Build both target release configurations with and without their BLE integration
  to check local-input independence. Measure changed queue/task storage against each
  target's [DRAM budget](../reference/dram/dram-budget.md).
- On identified artifacts, check KEY/BOOT, WAKE/touch classification, modal routing
  and supported sleep/wake under display load. Record serial evidence separately
  from physical input and visible behavior.

**Done when:** local adapters pass their host and applicable target checks,
press/release and contact-edge consumption work, and current product actions remain
intact. Report each target separately; BLE device qualification belongs to its own plan.
