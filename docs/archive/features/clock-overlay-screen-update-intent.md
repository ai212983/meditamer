# Clock Overlay Screen Update Intent Plan

- Status: Done — physical clock-overlay behavior confirmed on the Inkplate panel
- Last-reviewed: 2026-08-31
- Archived: 2026-08-31
- Decision: [ADR-0025](../../architecture/0025-screen-update-policy.md)

## Objective

Implement the first `ScreenUpdateIntent` slice for the Ambient Home clock overlay.

## Scope

1. Add `ScreenUpdateIntent::{Fast, Clean}` and an update request with optional damage at the
   renderer-independent UI boundary. Map `RefreshIntent::FullRepaint` to Clean with `damage: None`
   and retire the global full-repaint flag. Static `RefreshHint` surface metadata stays separate.
2. Render clock admission, text update, removal, and scheduled arc/circle movement through this
   request. Collapse all co-ready Ambient changes into Clean when it is executable. While Clean
   waits, clock admission and update remain eligible for strict partial refresh.
3. Assign Ambient Home outcomes explicitly:
   - clock admission and clock text changes: `Fast`;
   - every path that removes the clock overlay, including timeout and Back: `Clean`;
   - scheduled arc/circle movement: `Clean`, preserving its current behavior;
   - environment/footer observations: unchanged `Fast` behavior.
4. Add a side-effect-free partial-readiness query and a strict board operation that rechecks
   readiness, stays within partial/no-change, and reports not-ready or error when Clean recovery is
   required. While upload defers Clean, admit Fast rendering only after a ready result. Retain the
   semantic Fast action and its originating surface-instance token, then retry automatically after
   recovery or eligibility changes. Execute it while the token remains current and the overlay
   remains scheduled for presentation, and fetch current wall-clock data at that time. A queued
   removal supersedes unpresented Fast work; Fast work already visible remains an earlier
   presentation. If the strict operation unexpectedly becomes not-ready after rendering, retain that
   frame behind the recovery barrier. The existing fallback-capable operation remains available when
   Clean is allowed.
5. Stage clock removal as a Clean UI transaction. If Clean is prohibited, queue Back or timeout
   before changing coordinator, widget, capture, or framebuffer state. Fetch current wall-clock data
   when a deferred timeout becomes executable. Then hide the overlay in a Clean render while
   retaining its shell ownership and modal capture, and scan the staged background frame. On success,
   commit removal, destroy the overlay, and release capture. On scan failure, keep the staged frame
   and ownership behind the recovery barrier. A teardown failure after a successful scan remains an
   existing retryable cleanup fault; keep input blocked and the overlay hidden while retrying.
6. Any failed rendered panel transaction enters a recovery barrier: retain the current framebuffer
   as pending Clean recovery, hold later UI mutation and rendering, and retry from the existing
   framebuffer. Extend the one-second recovery deadline to post-startup failures so retries use a
   stable cadence and run independently of later events. Resume when Clean is allowed, and clear the
   barrier after a successful clean scan.
7. Retire the global automatic partial-refresh limit: remove its declaration and import, the
   automatic-limit selection branch, the partial counter, and counter-only logs and tests. Startup
   and transaction failure continue to request clean recovery directly.
8. Update the display-refresh reference with the implemented contract.

## Boundaries

- Ghosting scores, spatial debt maps, periodic cleanup rules, and new waveforms remain future
  evidence-led work.
- Other screens and overlays retain their current refresh behavior.
- LVGL's L8-to-I1 conversion, dirty-area calculation, and normal safety fallback remain unchanged.

## Acceptance

- Host tests cover intent and damage pairing, `damage: None`, and co-ready work collapsed into Clean.
- Upload tests cover strict partial execution, preflight refusal with unchanged framebuffer and idle
  panel, automatic retry with current wall-clock data, source-instance expiry, and clock admission
  ahead of a deferred arc Clean request.
- Tests cover Clean removal superseding unpresented Fast work and following Fast work that already
  reached the panel.
- Back and timeout tests cover retained modal ownership, the staged background frame, exactly-once
  commit after scan success, and retryable scan or teardown failure.
- Fast and Clean failures both retry from the recovery barrier at the one-second cadence, including
  retries with zero new damage.
- Refresh-selection tests cover startup and failure recovery after removing the automatic limit and
  partial counter.
- Device evidence shows flash-free clock presentation, clean Back and timeout restoration, accurate
  requested/actual logs, and visually clean panel patterns.
