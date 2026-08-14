# Log and Artifact Pruning Implementation Ledger

- Status: Done
- Last-reviewed: 2026-08-13
- Archived: 2026-08-14
- Owner: Host Tooling
- Plan: [Log and artifact pruning](log-and-artifact-pruning.md)

This is the execution record for the pruning plan. It records phase state, reviews, validation,
decisions, deviations, and applied-prune evidence. The plan remains the authority for scope and
retention policy; implementation changes that alter either are first reflected there.

## Phase status

| Phase | State | Required input | Completion evidence | Next action |
| --- | --- | --- | --- | --- |
| 0. Inventory and active retention | Passed | Recalculated baseline | E-0001 | None -- inventory implemented, reconciled against `find`/`du`. |
| 1. Artifact thinning | Passed | E-0001 and R-001 | E-0002 | None -- first apply completed and reviewed (R-001/R-002 approved). |
| 2. Run expiry | Passed | E-0002 and R-003 | E-0003 and E-0005 | None -- implementation, fixture verification, and review fixes complete. The live `--runs --apply` remains deliberately deferred. |
| 3. Integration and closeout | Passed | E-0005 | E-0004 and E-0005 | None -- fix-pass aggregate checks and adversarial review are clean. |

Allowed states are `Not started`, `In progress`, `Blocked`, `Passed`, and `Failed`. Only one phase
is `In progress`. A phase passes when its required evidence entry and review boundary are complete.

## Planning baseline

Execution baseline recalculated 2026-08-13 via `scripts/hostctl.sh artifacts inventory`
(E-0001). "Text logs and metadata" is defined here as evidence bytes minus recognized-payload
bytes across every discovered run unit and standalone file; see F-001 for why it reads far
higher than the planning estimate.

| Measure | Planning snapshot | Execution baseline |
| --- | ---: | ---: |
| Total `logs/` size | 1.9 GiB | 1.9 GiB |
| Files | 6,801 | 6,840 |
| Recognized flash payloads | 724 / 1.28 GiB | 669 / 1.2 GiB |
| Recognized payloads older than 7 days | 542 / 894 MiB | 487 / 806.9 MiB |
| Text logs and metadata | 207 MiB | 702.3 MiB (see note above / F-001) |

## Review ledger

| ID | Boundary | State | Required evidence | Result |
| --- | --- | --- | --- | --- |
| R-001 | Implementation diff | Reviewed -- approved | Classifier, retention, dry-run, and fixture tests | User approved 2026-08-13 after review of E-0002's implementation/test evidence. |
| R-002 | First artifact apply | Reviewed -- approved | Live dry-run candidate list and expected reclaimed bytes | User approved 2026-08-13 (487 payloads / 806.9 MiB) in the same turn as R-001; first `--apply` executed immediately after. |
| R-003 | Before run expiry | Superseded by direct instruction | First artifact-only applied-prune report | User chose "Start Phase 2" over a standalone R-003 review turn (2026-08-13); the Phase 1 apply's contents (487 payloads removed, 0 retained-skipped, all plain flash-capture binaries) were already summarized and available to that decision. R-004 below is the concrete pre-apply review boundary for Phase 2's own (larger, less reversible) action. |
| R-004 | First `--runs --apply` | Reviewed -- deferred | Live `--runs` dry-run candidate list and expected reclaimed bytes | Live dry run 2026-08-13: 995 run units (913 inconclusive, 47 passed, 35 failed) + 794 standalone logs, 650.4 MiB total (608.7 MiB units + 41.7 MiB standalone). All candidates are 146-172 days old -- well past both the 30d/90d thresholds, no near-boundary cases. 0 retained-skipped (no retention records exist anywhere live). User approved applying, then chose to skip/defer it after one attempt was blocked by the harness's permission classifier (a whole-directory removal at this scale reads as consequential enough to warrant a direct manual run). Not rejected -- `logs/` still carries this candidate set; `scripts/hostctl.sh artifacts prune --runs --apply` remains available to run manually at any time. |

## Verification ledger

| ID | Phase | Required verification | State | Evidence |
| --- | --- | --- | --- | --- |
| V-001 | 0 | Inventory reconciles with `find` and `du`; outcome and payload classes match fixtures and sampled live units | Passed | E-0001 |
| V-101 | 1 | Dry-run/apply parity; retention-scope and `--ignore-age` behavior; idempotent second apply | Passed -- fixture-level and confirmed live (see E-0002) | E-0002 |
| V-102 | 1 | Recent, expired, retained, partially thinned, standalone, and legacy fixtures | Passed | E-0002 |
| V-201 | 2 | Exact and just-before 30/90-day boundaries for passed, failed, inconclusive, standalone, and prune-report candidates; recent standalone `--ignore-age`; retained units, due reviews, and idempotent second apply | Passed | E-0003 and E-0005 |
| V-202 | 2 | Serialized applied-report fields and partial-apply audit behavior | Passed | E-0005 |
| V-301 | 3 | Host tests/lint, formatting, leaf-command ratchet, and documentation checks | Passed for the initial closeout | E-0004 |
| V-302 | 3 | Relevant aggregate checks after the review fixes | Passed | E-0005 |

## Recorded decisions

| ID | Date | Decision | Reason |
| --- | --- | --- | --- |
| D-001 | 2026-08-13 | Initial thinning recognizes canonical flash-capture payload roles only. | Diagnostic dumps, fixtures, and legacy binaries have different evidence value despite sharing extensions. |
| D-002 | 2026-08-13 | Immediate children of `logs/` are the run or standalone expiry units. | Whole-unit expiry is simpler and clearer than partial directory cleanup. |
| D-003 | 2026-08-13 | Classification belongs to the hostctl pruning utility and uses existing run output. | Producer-wide cleanup metadata and tracked-plan discovery are outside this plan. |
| D-004 | 2026-08-13 | Applied pruning writes one timestamped report and leaves original evidence bundles unchanged. | The report provides an audit without making cleanup state part of each bundle. |
| D-005 | 2026-08-13 | Reviews occur at R-001 through R-003. | Each review is tied to a concrete implementation or deletion decision. |
| D-006 | 2026-08-13 | `--ignore-age` suppresses minimum ages without changing retention or apply semantics. | Manual cleanup sometimes needs to include recent output while keeping one selection model. |
| D-007 | 2026-08-13 | Payload age eligibility uses each payload file's own mtime, not the run unit's inferred outcome/age. | Payload thinning is a per-payload operation independent of run outcome; outcome-based run age is reserved for Phase 2 run expiry. |
| D-008 | 2026-08-13 | Superseded by D-019. The initial classifier accepted a payload beside any one of `flash.log`, `capture.log`, or `summary.txt`. | The post-implementation review found that a generic summary could make this signature too broad; D-019 records the corrected current and legacy signatures. |
| D-009 | 2026-08-13 | The only recognized structured outcome report today is `report.json` (`final_status`/`finished_at`) written by `scripts/tests/hw/test_wifi_regression_gate.sh`. Every other unit is `inconclusive` for outcome purposes. | It is the sole current producer of a structured owning-gate report; adding report schemas for other producers is out of this phase's scope (no producer-wide metadata). |
| D-010 | 2026-08-13 | Applied prune reports are written under `logs/.prune-reports/`, a new operational root excluded from run-unit/standalone discovery alongside `logs/.state/` and `logs/locks/`. | Keeps the "Prune reports" retention class (90 days, per the plan) physically separate from ad hoc runtime state and from evidence content. |
| D-011 | 2026-08-13 | Prune-report `removed[].path` entries and printed candidate/report paths are repo-relative, not absolute. | Matches the existing generated-report convention (`report.json`'s `log_path` fields) and the AGENTS.md absolute-local-path mandate's spirit, even though `logs/` is gitignored. Fixed after the first live apply; that report (`artifact-prune_20260813_205609_401743000.json`) predates the fix and still holds absolute paths -- harmless since it is gitignored and was never committed. |
| D-012 | 2026-08-13 | Any retention record (any scope) protects a run unit from whole-unit expiry; `evidence`/`reflash`/`debug` differ only in which payload roles they additionally protect from thinning. | Matches the plan's retention-record section literally ("evidence: retain the unit from run expiry"; "reflash: retain the unit plus..."); avoids a second protection model for run-level expiry. |
| D-013 | 2026-08-13 | `--runs` is additive on the existing `prune` command, not a separate mode: units too young for whole-unit expiry still get ordinary payload thinning in the same pass; units selected for whole-unit expiry are not also scanned for payload candidates (the directory removal subsumes them). | Matches the plan's "extend the same command with run expiry" wording; avoids double-reporting the same bytes as both a payload candidate and part of a removed unit. |
| D-014 | 2026-08-13 | `.retain.json` is excluded from a run unit's "newest evidence-file modification time" (`scan::scan_unit`). | The retention record is metadata about the unit's evidence, not evidence itself; counting it would reset a unit's inferred run age (and inventory-displayed age) every time its retention record is added or renewed, which is misleading and would make expiry eligibility depend on retention-editing activity. Found via a fixture test failure during Phase 2 implementation, not a live-data issue (no retention records existed live at the time). |
| D-015 | 2026-08-13 | R-003 (the plan's "review before implementing/enabling run expiry" boundary) was not run as a standalone review turn; the user chose to proceed directly to Phase 2 implementation with the Phase 1 apply's results (already summarized) in hand. R-004 was added as the concrete pre-apply review boundary for Phase 2's own, more consequential action (whole-directory removal). | Records the actual decision sequence for audit purposes without blocking on a formality the user had already effectively satisfied and explicitly chose to skip. |
| D-016 | 2026-08-13 | A live `--runs --apply` is not required for Phase 2 to pass and was deliberately deferred. | The acceptance criteria are fixture-verifiable. The post-implementation review later reopened the boundary and applied-report proof under V-201/V-202; `logs/` still carries the 995-unit/794-standalone candidate set recorded in R-004/E-0003 for whenever the apply is wanted. |
| D-017 | 2026-08-13 | The existing `artifacts prune --runs` flow also expires prior prune reports after 90 days. | Implements the declared report-retention class without another tool, workflow, scheduler, or state mechanism. |
| D-018 | 2026-08-13 | A structured report remains authoritative for outcome; run age is the later of report completion and newest evidence mtime. | Prevents a reused unit with newer output from expiring against a stale completion timestamp without introducing active-run state. |
| D-019 | 2026-08-13 | A recognized flash-capture layout requires `capture.log` plus `sha256.txt`, or the legacy `capture.log` + `flash.log` + `summary.txt` sibling set. | Keeps ordinary diagnostic directories with a coincidental payload filename or summary outside artifact thinning. |

## Findings and deviations

Append implementation findings here. A finding that changes scope or retention policy first amends
the plan.

| ID | Date | Phase | State | Finding or deviation | Resolution |
| --- | --- | --- | --- | --- | --- |
| F-001 | 2026-08-13 | 0 | Noted, no plan change | The planning baseline's "Text logs and metadata" figure (207 MiB) is far below the recalculated execution figure (702.3 MiB, evidence bytes minus recognized-payload bytes). The gap is the large pre-existing population of legacy/unclassified diagnostic run units (Wi-Fi/blackout-era experiments) that carry substantial non-payload bytes the original estimate did not anticipate. | Does not change thinning scope -- only recognized payloads are ever selected. Recorded so the execution baseline isn't read as a reconciliation failure. |
| F-002 | 2026-08-13 | 0/1 | Noted, no plan change | Two live units (`hostctl_flashcapture_backend_legacy_port_20260311_berlin_{legacyqueue,queuehandlefix}_115k`) have a `capture.log` that is itself a directory containing a full nested bundle (`flash.log`/`capture.log`/`summary.txt`/`app.bin`), from an old capture-path bug. The recursive classifier recognizes the nested `app.bin` because the nested directory carries its own valid sibling markers. | Working as designed (recursion exists for exactly this shape); both units are legacy (March 2026) and already candidates. Covered by `classifier_recurses_into_nested_bundle_layouts`. |

## Evidence entries

Append one entry at each completed phase. Record commands, relevant source identity, counts, result,
and artifact or prune-report paths without turning this ledger into a raw log dump.

### E-0001: Inventory and retention baseline

Command: `scripts/hostctl.sh artifacts inventory`, run 2026-08-13 against the live `logs/` tree.

- Total: 1.9 GiB across 6,840 files. Reconciled exactly against `find logs -type f | wc -l`
  (6,840) and `du -sh logs` (1.9G).
- Files older than 30 days: 4,192 / 858.8 MiB.
- Operational (`.state`, `locks`): 9.4 KiB. Prune reports (`.prune-reports`): 0 B (none applied yet).
- Run units: 1,657 (1.9 GiB evidence). Recognized flash payloads: 669 / 1.2 GiB; older than 7
  days: 487 / 806.9 MiB.
- Outcomes: passed=47, failed=41, inconclusive=1,569 (recognized-report outcome is currently
  only produced by the Wi-Fi regression gate; see D-009).
- Standalone items: 1,115 / 62.0 MiB.
- No `.retain.json` records exist anywhere in the live tree yet: 0 retained units/items, 0 due
  reviews.
- Also covered by the fixture test `inventory_totals_reconcile_with_manual_walk`
  (`tools/hostctl/src/workflows/artifacts/tests.rs`), which asserts byte/file-count parity
  against an independent manual walk on a synthetic tree.

Result: V-001 passed. See F-001 for the "text logs and metadata" baseline discrepancy.

### E-0002: Artifact-thinning implementation and first apply

Implementation:

- `tools/hostctl/src/workflows/artifacts/{mod,model,scan,retention,report,inventory,prune}.rs`:
  classifier, `.retain.json` handling, inventory totals, and prune dry-run/apply.
- `tools/hostctl/src/main.rs`: `Commands::Artifacts` container wired to
  `hostctl artifacts inventory` and `hostctl artifacts prune [--apply] [--ignore-age]`.
- `scripts/ci/check_script_surface.py`: generalized `count_hostctl_leaf_commands` to sum every
  `Commands` container's nested subcommand enum (previously hardcoded to `Test` only).
- `scripts/surface.json`: `documented_leaf_commands.count` 13 -> 15, with a change_log entry.

Validation:

- 27 focused tests (`tools/hostctl/src/workflows/artifacts/{tests,retention,report}.rs`)
  covering recent/expired/ignore-age, `evidence`/`reflash`/`debug` retention scopes, partially
  thinned and legacy (unmarked) units, nested bundle recursion, standalone retention/due-review,
  outcome inference, dry-run/apply parity, and second-apply idempotency. All passing.
- `scripts/host-test.sh test hostctl`: 186/186 passed (full suite, no regressions).
- `scripts/host-test.sh lint hostctl` (strict Clippy): clean.
- `cargo fmt --check` (tools/hostctl): clean.
- `python3 scripts/ci/check_orphan_modules.py`: clean (559 files, 0 unreachable).
- `python3 scripts/ci/check_script_surface.py`: clean (`hostctl leaf commands=15/15`).

Live dry run (read-only): `scripts/hostctl.sh artifacts prune`, 2026-08-13 -> 487 candidate
payloads, 806.9 MiB reclaimable, 0 retained-skipped (no retention records exist live yet).
Matched E-0001's inventory figure for "recognized flash payloads older than 7 days" exactly,
confirming dry-run/inventory parity on the live tree.

R-001 and R-002 were reviewed and approved by the user the same turn this evidence was
presented.

**First live `--apply`**, 2026-08-13 20:56:09 UTC: `scripts/hostctl.sh artifacts prune --apply`
removed 487 payloads, reclaimed 846,126,696 bytes (806.9 MiB), report written to
`logs/.prune-reports/artifact-prune_20260813_205609_401743000.json`. `logs/` dropped from 1.9 GiB
to 1.1 GiB (`du -sh logs`), matching the reclaimed total exactly. Recognized flash payloads older
than 7 days is now 0 (182 recent payloads remain, all under the 7-day threshold).

Idempotency: a second `--apply` run immediately after removed 0 / reclaimed 0 B, writing a second,
empty report -- matches V-101's fixture-level idempotency test.

Post-apply fix (D-011): the first live report's `removed[].path` entries were absolute local
paths; `relative_display()` was added in `prune.rs` afterward so all printed and reported paths
are repo-relative going forward. That one already-written report is unaffected (gitignored,
never committed) and was left as-is rather than regenerated.

Result: R-001, R-002 reviewed and approved. Phase 1 acceptance met: dry-run/apply parity,
retention-scope and `--ignore-age` behavior (fixture + live), idempotent second apply (fixture +
live), and the first artifact-only prune's actual reclaimed bytes are recorded above.

### E-0003: Run-expiry implementation

Implementation:

- `tools/hostctl/src/workflows/artifacts/outcome.rs` (new): outcome/age inference factored out of
  `inventory.rs` so inventory display and expiry eligibility can never disagree.
- `tools/hostctl/src/workflows/artifacts/expiry.rs` (new): `ExpiredUnit`/`ExpiredStandalone`
  shapes and the 30-day (passed) / 90-day (failed or inconclusive) / 30-day (standalone) age
  thresholds (D-012, D-013).
- `tools/hostctl/src/workflows/artifacts/prune.rs`: extended `build_prune_plan` to also select
  whole-unit and standalone expiry candidates when `--runs` is set (split into
  `collect_run_units`/`evaluate_unit`/`evaluate_unit_expiry`/`collect_standalone` to keep the
  rust-code-analysis `nargs` ratchet under its threshold -- see below); `apply_prune_plan` now
  also removes expired unit directories (`fs::remove_dir_all`) and standalone files, and the
  prune report gained `runs`/`removed_units*`/`removed_standalone*` fields alongside the
  unchanged payload fields.
- `tools/hostctl/src/workflows/artifacts/scan.rs`: `.retain.json` excluded from a unit's
  "newest evidence-file modification time" (D-014).
- `tools/hostctl/src/main.rs`: `hostctl artifacts prune --runs` flag wired through.

Validation:

- 11 new focused tests (`tools/hostctl/src/workflows/artifacts/tests.rs`, plus 2 in
  `expiry.rs`) covering: older/younger passed and inconclusive run selection,
  `--runs --ignore-age` on a brand-new unit, `evidence` retention surviving whole-unit expiry
  while still allowing payload thinning, `debug` retention blocking both, standalone expiry and
  retention, an integration test asserting the expiring unit's directory and the expiring
  standalone file are actually gone post-apply while the retained unit's directory survives (with
  its payloads still thinned) and the young unit is untouched, and idempotent second apply. Also:
  a regression test proving `prune` without `--runs` never touches whole units/standalone logs
  even when they qualify by age. 38/38 artifacts tests passing; 197/197 full hostctl suite.
- `scripts/host-test.sh lint hostctl` (strict Clippy): clean.
- `cargo fmt --check` (tools/hostctl): clean.
- `python3 scripts/ci/check_orphan_modules.py`: clean.
- `python3 scripts/ci/check_script_surface.py`: clean, unchanged (`--runs` is a flag on the
  existing `prune` leaf, not a new leaf command; 15/15).
- `scripts/ci/lint_code_analysis.sh` (blocking SLOC/complexity ratchet): initially caught a real
  regression -- `build_prune_plan`'s `nargs` metric (which sums a function's own parameters plus
  every closure's parameters in its body) rose to 9 against the ratchet's max of 8, driven by
  three inline `sort_by(|a, b| ...)` closures. Fixed by extracting three named sort helpers;
  `prune.rs` stays at 582 SLOC (under the 600 warn threshold). Final run: 0 blocking violations.

Live dry run (read-only): `scripts/hostctl.sh artifacts prune --runs`, 2026-08-13 -> **995 run
units** (913 inconclusive, 47 passed, 35 failed; 608.7 MiB) and **794 standalone logs** (41.7
MiB) selected, 650.4 MiB total, 0 retained-skipped (no retention records exist live). All
candidates are 146-172 days old -- no near-boundary cases. Saved to the session scratchpad
(`artifact-prune-runs-dry-run-20260813.log`), not pasted here in full (1,799 lines).

**The first `--runs --apply` has not been run.** The user reviewed and approved applying it;
one attempt was blocked by the harness's permission classifier (a 995-directory removal read as
consequential enough to require a direct manual run), and the user then chose to skip/defer it
rather than route around the block (D-016). `logs/` is unchanged from the E-0001/E-0002 apply
state -- still 1.1 GiB, still carrying this exact 995-unit/794-standalone candidate set. Running
it later needs no new implementation: `scripts/hostctl.sh artifacts prune --runs --apply`.

Result at initial closeout: R-004 reviewed and deferred; Phase 2 was marked Passed on fixture
verification (D-016) with the live apply available but not executed. The post-implementation review
reopened V-201/V-202 for exact boundary, failed-outcome, serialized-report, and partial-apply proof.

### E-0004: Integration and closeout

Documentation: `docs/guides/development-setup.md` gained a "Log and Artifact Cleanup" section
(policy summary, `.retain.json` schema, and the `artifacts inventory`/`artifacts prune` commands)
as the host-tooling guide; `docs/guides/build-and-flash.md` gained a cross-reference from its
flash-capture artifact-directory bullets (the build/flash guide). `check_markdown_links.sh` clean
on both; both sit a little over the 220-line advisory (223 and 267 lines) -- non-blocking per
policy.

Aggregate validation: 197/197 hostctl tests (38 artifact-specific), strict Clippy clean, `cargo
fmt --check` clean, `check_orphan_modules.py` clean, `check_script_surface.py` clean (leaf-command
ratchet 15/15, unchanged since `--runs` is a flag not a new leaf).

Retained-candidate check: no `.retain.json` records exist anywhere in the live tree, so this
criterion is vacuously satisfied (0 retained-skipped throughout).

Final `logs/` size: 1.9 GiB -> 1.1 GiB (806.9 MiB reclaimed via the Phase 1 apply). The Phase 2
`--runs --apply` (995 units / 794 standalone, 650.4 MiB more) remains available but was not run
this session (D-016); `logs/` still carries that exact candidate set.

Result: Phase 3 passed. Plan and ledger Status changed to Done.

### E-0005: Post-implementation review fix pass

Implementation and focused proof:

- the existing `artifacts prune --runs` flow expires direct prune-report JSON files after 90 days;
- fixed-clock fixtures cover exact and just-before 30/90-day boundaries for passed, failed,
  inconclusive, standalone, and prune-report candidates, plus recent standalone and prune-report
  selection under `--ignore-age`;
- applied-report JSON assertions cover removed paths, outcomes, ages, sizes, and reclaimed bytes;
- a partial apply preserves each successful removal in the audit and records the later deletion
  failure; report replacement writes a sibling temporary file before renaming it over the live JSON;
- stale report completion cannot make newer evidence expire, and classifier fixtures cover current,
  observed legacy, and generic-summary negative layouts.

Validation, 2026-08-13:

- `scripts/host-test.sh test hostctl`: 202/202 passed (43 artifact-specific);
- `scripts/host-test.sh lint hostctl`: strict Clippy clean;
- hostctl `cargo fmt --check` and `git diff --check`: clean;
- `check_orphan_modules.py`: 559 tracked Rust files, zero unreachable;
- `check_script_surface.py`: clean, hostctl leaf commands 15/15;
- `lint_code_analysis.sh`: zero blocking violations;
- whole-repository Markdown links: zero errors; Markdown LOC remains advisory-only;
- final focused adversarial, test/acceptance, and scope reviews found no residual material issue.

Result: V-201, V-202, and V-302 passed. Phase 2 and Phase 3 returned to Passed; plan and ledger
status returned to Done. No live whole-run apply was performed during this fix pass.
