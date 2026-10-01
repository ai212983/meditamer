# residency-policy

`no_std`, dependency-free policy foundation for transactional residency.

## What this crate owns

- Identity newtypes (`ResourceId`, `RequestId`, `Generation`) and the
  generation-checked `ResourceHandle` (`matches_exact`,
  `is_current_for`): stale-generation completions fail the check and
  are never applied.
- The `Staged` trait (infallible `commit`/`rollback`; anything fallible
  stays in `prepare`) and the `Participant` trait (mechanisms stage one
  request into a policy-owned provisional).
- The policy-owned provisional: `Provisional::commit` takes the staged
  value and commits it; `Drop` rolls back only when a staged value is
  still present, so dropping an uncommitted provisional is rollback.
- `coordinate`: the coordinator for exactly two heterogeneous
  participants — prepares left then right, drops (rolls back) left when
  right prepare fails, and commits both only after both prepare
  successfully. Side identity survives in `PairError::Left/Right`.

## When to use it

- A joint reservation spans exactly two mechanisms (for example arena
  plus block pool) and must commit all-or-nothing with deterministic
  rollback order.

## When not to use it

- Three or more participants must commit jointly: this coordinator is
  exactly two; anything wider needs new policy work.
- The caller needs admission verdicts, bundle budgets, leases, pins, or
  eviction: declarations, budget model, and eviction remain open.
- The caller needs allocation, decoding, or target code: this crate
  never touches memory, formats, or hardware.

## Example

```rust
use residency_policy::{Generation, RequestId, ResourceHandle, ResourceId, coordinate};

let handle = ResourceHandle::new(ResourceId(3), RequestId(11), Generation(7));
// Each mechanism implements Participant; prepare stages, coordinate commits.
let (arena_out, block_out) = coordinate(&mut arena_part, &mut block_part, handle)
    .map_err(|e| match e {
        residency_policy::PairError::Left(l) => "arena prepare failed",
        residency_policy::PairError::Right(r) => "block prepare failed",
    })?;
assert!(handle.is_current_for(ResourceId(3), Generation(7)));
```

## Safety, lifetime, and concurrency constraints

- No `unsafe`; provisionals own their staged value, so an uncommitted
  stage cannot leak past its owner — `Drop` always runs.
- `commit` consumes `self`, so a provisional commits or rolls back
  exactly once; `commit` itself is infallible by contract.
- No locks or threads here; each mechanism owns only its local staged
  state and the joint transaction belongs to `coordinate`.

## Maturity and deferred integrations

- Implemented and unit-tested: identities, handle checks, provisional
  rollback, two-participant coordination and failure order.
- Deferred: bundle declarations, admission verdicts, leases, pins,
  eviction, and product budget consumption.
- Not provided here: allocation, target backing, DMA, cache eviction
  behavior, validated codecs, or production pack consumers.
