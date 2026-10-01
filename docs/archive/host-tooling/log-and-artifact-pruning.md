# Log and Artifact Pruning Plan

- Status: Done
- Last-reviewed: 2026-08-13
- Archived: 2026-08-14
- Owner: Host Tooling
- Scope: Generated flash-capture bundles, hardware-test output, standalone logs, and retained
  firmware images below `logs/`
- Ledger: [Implementation ledger](log-and-artifact-pruning-ledger.md)

## Objective

Keep recent diagnostic evidence convenient while preventing generated firmware images and completed
run output from accumulating indefinitely. Make retention visible, reviewable, and easy to apply
through the existing hostctl entry point.

## Baseline

Snapshot from 2026-08-13:

| Measure | Value |
| --- | ---: |
| Total `logs/` size | 1.9 GiB |
| Files | 6,801 |
| Recognized flash payloads | 724 / 1.28 GiB |
| Recognized flash payloads older than 7 days | 542 / 894 MiB |
| Text logs and metadata | 207 MiB |
| Files older than 30 days | 4,192 / 868 MiB |

Recognized flash payloads initially mean `firmware.elf`, `app.bin`, `bootloader.bin`, and
`partition-table.bin` inside a flash-capture layout identified by `capture.log` plus `sha256.txt`, or
the legacy `capture.log` plus `flash.log` plus `summary.txt` layout. Diagnostic dumps, fixtures, and
other legacy binary collections require an explicit classifier before they enter artifact thinning.

Recalculate the baseline at execution start. The first artifact-only prune should recover up to
about 894 MiB before retained artifacts are excluded.

## Retention policy

| Class | Default retention | Preserved content |
| --- | ---: | --- |
| Recognized flash payloads | 7 days | Payloads selected by `reflash` or `debug` retention |
| Successful run unit | 30 days | Non-payload evidence; payloads follow the 7-day rule |
| Failed or inconclusive run unit | 90 days | Non-payload evidence; payloads follow the 7-day rule |
| Standalone log | 30 days | The complete file |
| Prune reports | 90 days | Summary of each applied prune |
| Runtime state and lock roots | Operational lifetime | `logs/.state/`, `logs/locks/` |

Artifact thinning and run expiry are separate operations. Thinning removes recognized flash
payloads while preserving the run's logs, reports, metadata, and hashes. Run expiry later removes
the remaining run unit when its outcome retention period ends.

A run unit is an immediate child directory of `logs/`; a standalone item is an immediate child file.
Artifact discovery may recurse within a run unit. Whole-run expiry operates on the unit rather than
part of its directory tree.

Run outcome uses a recognized structured report when present. Run age uses the later of that
report's completion time and the newest evidence-file modification time, so newer output in a reused
unit remains recent. Without a recognized report, age uses the newest evidence-file modification
time. Standalone files use their own modification time. Flash-capture-only and interrupted runs are
inconclusive unless a structured owning gate records a test outcome.

## Retention records

Retention is declared by `.retain.json` inside a run unit or by an adjacent
`<filename>.retain.json` for a standalone file:

```json
{
  "scope": "reflash",
  "reason": "Phase 6 physical acceptance candidate",
  "owner": "firmware",
  "review_after": "2026-09-15"
}
```

Supported scopes:

- `evidence`: retain the unit from run expiry while allowing normal payload thinning;
- `reflash`: retain the unit plus application, bootloader, and partition images;
- `debug`: retain `reflash` content plus the ELF and linker map when present.

For a standalone file, `evidence` retains that file. Its adjacent retention record is metadata for
the target and is not an independent prune candidate.

`review_after` is required. A due record continues to retain its content until it is renewed or
removed; inventory reports it for review. The command reads run output and retention records as its
operational inputs. Documentation references may be recorded in `reason`, but are not retention
authority.

## Review boundaries

Review is focused at three execution boundaries:

1. After the implementation diff and focused tests exist, review classifier correctness, retention
   behavior, and the exact files selected by fixtures.
2. Immediately before the first `--apply`, review the live dry-run candidate list, retention
   records, and expected reclaimed bytes.
3. After the first artifact-only prune report, review the actual removals and any retention
   corrections before implementing or enabling whole-run expiry.

Each review is limited to the evidence and decision at that boundary.

## Phase 0: Inventory and retain active evidence

Add a read-only inventory command to the existing hostctl launcher:

```bash
scripts/hostctl.sh artifacts inventory
```

Report totals by unit, age, inferred outcome, payload class, retained scope, due review, and
potential reclaimed bytes. Add retention records for artifacts still needed for physical
acceptance, reflash, comparison, or symbolication.

Acceptance:

- inventory totals reconcile with `find` and `du` within filesystem block-size differences;
- recognized payloads are distinguished from diagnostic dumps, fixtures, and unclassified legacy
  binaries;
- flash-capture-only, interrupted, and otherwise unknown units classify as inconclusive;
- the initial candidate list and expected reclaimed bytes are reviewed before apply.

## Phase 1: Implement artifact thinning

Add the nested hostctl leaf command:

```bash
scripts/hostctl.sh artifacts prune
scripts/hostctl.sh artifacts prune --apply
scripts/hostctl.sh artifacts prune --ignore-age
```

The default invocation prints candidate payloads, reason, retained scope, and reclaimable bytes.
`--apply` removes eligible recognized flash payloads older than seven days and writes one timestamped
prune report listing removed paths, sizes, and reclaimed bytes. Inventory output shows which payloads
remain without rewriting the original evidence bundle.

If an apply encounters a deletion error after removing earlier candidates, it writes the successful
removals and failure details to the same report before returning an error.

`--ignore-age` suppresses the seven-day minimum and selects all recognized, unretained flash
payloads. It changes age eligibility only: retention scopes and dry-run/apply behavior remain the
same.

Implement inventory and pruning as a direct hostctl utility. Update hostctl's documented
leaf-command counter and change log to include `artifacts inventory` and `artifacts prune`; the
existing `scripts/hostctl.sh` surface remains the entry point.

Acceptance:

- dry-run and apply select the same eligible payload set;
- `--ignore-age` includes recent recognized payloads while preserving retained payloads;
- `evidence`, `reflash`, and `debug` retain exactly their declared content;
- a second apply is idempotent and reports zero additional bytes;
- focused tests cover recent, expired, retained, partially thinned, standalone, and legacy units;
- the first applied artifact-only prune records actual reclaimed bytes.

## Phase 2: Implement run expiry

After reviewing the first artifact-only apply report and correcting any retention records, extend
the same command with run expiry:

```bash
scripts/hostctl.sh artifacts prune --runs
scripts/hostctl.sh artifacts prune --runs --apply
scripts/hostctl.sh artifacts prune --runs --ignore-age
```

Expire successful run units after 30 days, failed or inconclusive run units after 90 days,
standalone logs after 30 days, and prior prune reports after 90 days. The applied prune report
records removed units, inferred outcomes, ages, and reclaimed bytes. With `--runs`, `--ignore-age`
suppresses those minimum ages and selects all unretained run units, standalone logs, and prior prune
reports.

Acceptance:

- fixed fixture timestamps exactly at and immediately before each retention boundary produce the
  expected passed, failed, inconclusive, standalone, and prune-report candidates;
- `--runs --ignore-age` includes recent unretained units and standalone logs;
- retained units remain present and due reviews are reported;
- serialized applied reports contain removed unit paths, outcomes, ages, and reclaimed bytes;
- a partial apply records successful removals and the deletion failure before returning an error;
- a second apply is idempotent.

## Phase 3: Integrate and close out

Document the policy and commands in the host-tooling and build/flash guides. Cleanup remains a
manual hostctl operation.

Closeout acceptance:

- hostctl unit and fixture tests pass;
- host lint, formatting, leaf-command ratchet, and relevant documentation checks pass;
- one inventory, artifact-only apply, and run-expiry dry-run are recorded;
- retained physical-test candidates still contain the content declared by their scopes;
- the resulting `logs/` size and reclaimed bytes are recorded here before status changes to Done.

### Initial closeout record (2026-08-13)

- Tests: 197/197 hostctl tests passing (38 artifact-specific); `scripts/host-test.sh lint hostctl`
  (strict Clippy), `cargo fmt --check`, `check_orphan_modules.py`, and `check_script_surface.py`
  (leaf-command ratchet, 15/15) all clean. `check_markdown_links.sh` clean on the updated guides.
- Recorded: one inventory (E-0001), one artifact-only apply (E-0002, 487 payloads / 806.9 MiB),
  and one run-expiry dry-run (E-0003, 995 units + 794 standalone / 650.4 MiB) -- see the
  [ledger](log-and-artifact-pruning-ledger.md).
- Retained candidates: no `.retain.json` records exist in the live tree, so this criterion is
  vacuously satisfied -- nothing was retained, and nothing was incorrectly removed as a result
  (0 retained-skipped in both the Phase 1 apply and the Phase 2 dry run).
- Documented in `docs/guides/development-setup.md` (host-tooling guide) with a cross-reference from
  `docs/guides/build-and-flash.md` (build/flash guide).
- `logs/` size: 1.9 GiB -> 1.1 GiB after the Phase 1 apply (806.9 MiB reclaimed). The Phase 2
  `--runs --apply` (995 units / 794 standalone logs, 650.4 MiB more) was reviewed and approved but
  deliberately deferred rather than run in this session; `logs/` still carries that candidate set
  and `scripts/hostctl.sh artifacts prune --runs --apply` remains available whenever it's wanted.
  Deferring it did not block the initial closeout because Phase 2 does not require a live apply
  (see ledger D-016).

The post-implementation review fix pass is complete. V-201, V-202, V-302, and E-0005 in the
implementation ledger record the focused and aggregate validation; the live whole-run apply remains
deliberately deferred as recorded above.
