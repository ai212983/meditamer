# Wi-Fi Regression Isolation Plan (continued)

[Back to the plan and phases 1–3](../wifi-regression-isolation-plan.md).

## Phase 4: Replay Non-Wi-Fi-Safe Batches

### Goal

Restore low-risk host and app changes without disturbing device-side Wi-Fi.

### Steps

- [ ] Step 4.1 replay Batch A
- [ ] Step 4.2 validate discovery gate
- [ ] Step 4.3 replay Batch B
- [ ] Step 4.4 validate discovery gate again

### Decision Rule

If discovery fails in Batch A or B, reduce that batch immediately. Do not
continue to Wi-Fi batches until non-Wi-Fi replay is still green.

## Phase 5: Replay Wi-Fi-Adjacent Batches

### Goal

Find the earliest Wi-Fi-related batch that reintroduces zero-discovery.

### Steps

- [ ] Step 5.1 replay Batch C in small chronological slices
- [ ] Step 5.2 validate after every slice
- [ ] Step 5.3 stop at the first slice that reproduces blackout
- [ ] Step 5.4 if Batch C stays green, begin Batch D in even smaller slices

### Replay Size Rule

Never replay more than one logically related Wi-Fi slice before running
hardware validation. Prefer:

- one commit
- or one tightly grouped file-domain patch

over large cherry-pick ranges.

## Phase 6: Identify The First Bad Slice

### Goal

Pin the first replay slice that changes the firmware from green to blackout.

### Steps

- [ ] Step 6.1 record the last green commit/slice
- [ ] Step 6.2 record the first bad commit/slice
- [ ] Step 6.3 capture both logs:
  - last green discovery artifact
  - first bad discovery artifact

### Deliverable

A narrow regression interval that is small enough for code-level reasoning.

## Phase 7: Reduce To Minimal Offending Change Set

### Goal

Reduce the first bad slice to the minimal code change set that causes the
blackout.

### Steps

- [ ] Step 7.1 split the bad slice by file domain
- [ ] Step 7.2 replay/revert subparts one at a time
- [ ] Step 7.3 identify the minimal offending change set

### Decision Rule

If the bad slice cannot be reduced cleanly because the changes are too tangled,
stop and choose stabilization by reverting the whole slice.

## Phase 8: Choose Stabilize-Via-Revert Or Forward-Fix

### Goal

Choose the lowest-risk path to get stable discovery back.

### Options

- [ ] Option 8.1 revert the minimal offending change set and keep later safe work
- [ ] Option 8.2 revert the entire first bad slice if reduction is too tangled
- [ ] Option 8.3 forward-fix only if the minimal root cause is concrete and the
      validation loop is fast enough

### Preferred Order

1. minimal revert
2. slice revert
3. forward-fix

Forward-fix is last because the current branch already proved that deep
theorizing without isolating the first bad slice is expensive and low-confidence.

## Stop Conditions

Stop this plan and rewrite it if any of these happen:

- the rollback anchor no longer validates as green
- non-Wi-Fi replay batches already reproduce the blackout and cannot be reduced
- the replay branch drifts into new Wi-Fi feature work
- hardware validation becomes unavailable

## Current Next Step

Start with Phase 1:

- create a fresh regression-isolation branch from `826b235`
- rerun the bounded discovery gate there
- confirm that the historical known-good state is still reproducible before
  replaying any later work
