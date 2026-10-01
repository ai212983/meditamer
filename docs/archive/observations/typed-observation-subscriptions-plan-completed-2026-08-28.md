# Typed Observation Subscriptions Plan

- Status: Complete — R1–R3 passed; physical rendering confirmed by the user.
- Last-reviewed: 2026-08-28
- Decisions: [ADR-0018](../architecture/0018-typed-observation-subscriptions.md), [ADR-0019](../architecture/0019-provider-owned-periodic-demand-overrides.md)
- Results: [Evidence index](typed-observation-subscriptions-ledger.md)

## Scope and completion

Deliver the shared typed observation contract for SHTC3/ADC on Medinote and BME688/BQ27441 on
Inkplate, with product-owned consumers and provider-owned acquisition. No required follow-ups remain.
The ledger records evidence and limitations; the [frozen prior plan](../archive/observations/typed-observation-subscriptions-plan-2026-08-28.md)
records the superseded phase structure.

The migration is implemented and has named host regressions and device evidence below. The bounded
reconciliation found no additional concrete subscription implementation defect. This is not a claim
of exhaustive correctness or physical fault coverage.

**Completed: R1–R3 passed, relevant existing regressions passed, and no confirmed subscription
regression remains unresolved.** Reuse unchanged-path evidence; a new image alone does not invalidate
it. When code changes affect a recorded result, name that result and rerun its relevant check.
R2/R3 cover the memory-ownership changes made during this work.

The user's E-0051 scope amendment defers ADR-0018's general resource-promotion gate for this
milestone. Its specific both-target linked-size/runtime-headroom checks still apply to capacity changes.

## Completed implementation and recorded validation

| Capability | Implementation and validation | Evidence |
| --- | --- | --- |
| Shared typed contract | Per-field demand/freshness, three initial policies, delivery intervals, health/retained values, identity/restart, bounded admission and expiry implemented; named core tests present | E-0032/E-0039/E-0044; `platform/observation` suite |
| Product consumers | Committed-owner entry/removal/rollback, fresh-entry requests, stale-result rejection, persistent battery trace and batched presentation paths implemented; production adapter tests present | E-0031/E-0040/E-0041/E-0047; `medinote` and `touch-core` suites |
| Provider controls and drivers | Correlated acknowledgements, retained resume, failed-quiescence panel exclusion, driver failure logging/retention and cancellation regressions present | E-0024–E-0031/E-0040; `touch-core` and board suites |
| Real acquisition on both boards | All four providers have fresh fixture and ordinary periodic sample evidence | E-0043–E-0045 |
| Expiring periodic overrides | Provider-owned expiry/cancellation/control closure restore latest live demand; host cases operate without UI polling; Medinote expiry/cancellation exercised on device | E-0046 |
| Inkplate panel lifecycle | Both providers close active overrides and restore demand/fresh acquisition after a normal panel cycle | E-0047 |
| Medinote sleep lifecycle | Deep timer wake, both synthetic cleanup rejections and active-override restoration exercised on identified images | E-0049/E-0050 |
| Inkplate upload coexistence (R1) | Fixed upload-priority starvation; fresh BME688/BQ27441 acquisitions during an unfinished HTTP body, complete content readback and cleanup passed; canonical Wi-Fi gate passed | E-0054 |
| External SD runner (R2) | Actual startup factories passed injected allocation rejection and unpolled-runner cleanup on device; cancelled upload removed its temporary file, then a new upload/readback passed on normal firmware | E-0055 |
| External UI ownership (R3) | Initialization cleanup fixed; actual allocation rejection, cleanup, callback lifetime, startup, navigation and repaint checks passed. User confirmed physical rendering looked correct | E-0056 |
| Storage and stack regression controls | Fixed capacities and linked sections/task pools measured separately for both boards; Inkplate BLE floor preserved | E-0036–E-0050; [RAM reference](../reference/dram/dram-budget.md) |

These rows state what was implemented or exercised. They do not imply that host tests proved hardware
faults, physical display responsiveness or continuous runtime memory peaks.

## Closure

On 2026-08-28 the user confirmed startup, screen transitions and repaint looked correct on the
normal E-0056 Inkplate BLE image. This closes R3 and the plan. No required follow-ups remain.
No further resource qualification or unchanged-path rerun is required for this milestone;
routine repository gates still apply when landing changes.

## Contract and checks to preserve

- The UI owns consumer policy/presentation; providers own acquisition, hardware sequencing and retry.
  Keep one authority, current owner/generation routing, credited ingress, and latest state delivery.
- Failures retain successful values/timestamps and report concrete acquisition errors with backoff.
  Freshness thresholds are not delivery deadlines; driver conversion waits remain driver-owned.
- Fixed capacities are two subscriptions and two total outstanding one-shot requests per provider;
  fixtures share those bounds. Expiry/control closure restore latest live demand without UI polling.
- Keep correlated control and failed-quiescence panel exclusion. A two-second acknowledgement wait
  bounds its caller, not all peripheral work; five sequential panel clients can take ten seconds.
  Inkplate acquisition has no whole-operation cancellation guarantee. Medinote sleep shares one deadline.
- Run relevant host tests/builds for changes. Existing suites are `observation`, `medinote`, `touch-core`,
  `inkplate-tempera` and, for fixture tooling, `hostctl`, through `scripts/host-test.sh`.
- For memory-affecting changes, compare affected-board linked sections, task pools and significant
  frames. Keep internal RAM, stack, heap and PSRAM distinct and the Inkplate BLE floor at 33900 bytes.
  Capacity changes retain ADR-0018's both-target measurements. Keep placement safety constraints;
  linked reservation is not runtime headroom.
- Preserve the [UI-coordinator handoff](shared-ui-coordination.md#typed-observation-provider-handoff).
  Future UI restructuring adopts the existing subscription authority rather than adding another one.
