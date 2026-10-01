# transfer-cache-block-pool

`no_std` fixed-block host core for the transfer cache (one dependency:
`residency-policy`).

## What this crate owns

- `Pool<const CAP, const WORDS, const MAX_REQUEST>`: `CAP` block slots
  tracked by a bitmap. `reserve` checks only the request shape;
  `Participant::prepare` scans for free slots without mutation, then
  marks all or none and yields a policy-owned provisional.
- `Cell` means `!Sync`: the pool uses interior mutability (`Cell` bitmap,
  epochs, counters) through a shared reference and is intentionally not
  `Sync` — keep it on one thread or behind a caller lock.
- Manual release, no `Grant` Drop release: dropping a `Grant` releases
  nothing; only `Pool::release` frees a slot, in any order, with
  pool/index/epoch checks (`WrongHandle` otherwise, pool unchanged).
- Unique `PoolId`: the `0` value is reserved (`InvalidConfig`); IDs must
  be unique among simultaneously live pools, which this core cannot
  detect beyond its own ID check.
- `Grant` of `BlockHandle`s (pool, slot index, commit-time epoch);
  double release bumps the epoch so stale handles fail. A release at
  `u32::MAX` retires its slot permanently instead of wrapping the epoch.
  Current/free/high-water/retired counters are observable.

## When to use it

- SD transfer or cached rows/tiles need fixed-size, independently
  releasable blocks with atomic multi-block staging and ownership
  transfer that needs no physical contiguity.

## When not to use it

- Lifetimes are uniform per screen: use the phase arena instead.
- The caller needs cache identity, pins, priority, eviction, backing
  bytes, DMA, or cross-thread sharing without its own lock: the pool
  owns only reservation, transfer, access, and release.

## Example

```rust
use transfer_cache_block_pool::{Pool, PoolId};
use residency_policy::{Generation, RequestId, ResourceHandle, ResourceId, Participant};

let pool = Pool::<8, 1, 4>::new(PoolId::new(7).unwrap()).unwrap();
let handle = ResourceHandle::new(ResourceId(3), RequestId(11), Generation(7));
let mut reservation = pool.reserve(3).unwrap();
let grant = reservation.prepare(handle).unwrap().commit();
assert_eq!(grant.len(), 3);
for h in grant.iter() {
    pool.release(h).unwrap(); // manual: dropping `grant` frees nothing
}
```

## Safety, lifetime, and concurrency constraints

- No `unsafe`; `Cell` state requires either single-thread use or an
  external caller lock — `Pool` is `!Sync` by design.
- `StagedBlocks` borrows the pool; rollback clears exactly the staged
  bits, so a failed multi-block reservation restores the bitmap.
- Handles are copyable capabilities: whoever holds a live handle may
  release its slot exactly once; stale or foreign handles are rejected.

## Maturity and deferred integrations

- Implemented and unit-tested: atomic staging, rollback, arbitrary
  release order, epoch/retirement behavior, coordinator conformance.
- Deferred: `BlockBacking`, data access, target synchronization, cache
  policy, and device proof.
- Not provided here: target backing, DMA, thread safety, cache eviction,
  or production pack consumers.
