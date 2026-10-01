# phase-arena

`no_std` checked phase ownership. `PhaseArena` is an offset-only
single-activation core; `RuntimeBundle<A>` owns allocator-backed parts.

## What this crate owns

- `RuntimeBundle<A>`: fallible contiguous or segmented parts acquired from one
  caller-supplied allocator, with checked access and rollback on partial
  failure. Meditamer supplies its strict external-memory allocator.
- `PhaseArena`: a caller-owned arena with a byte capacity and exactly one
  live activation at a time — either a staged `Reservation` or one
  committed allocation named by a `BackingToken`.
- Offsets only, no backing bytes: `Reservation::alloc` returns
  caller-relative `Span { offset, len }` values; the caller maps offsets
  onto its own store. No backing bytes live here.
- Single activation: `reserve` fails with `ArenaError::Busy` while a
  staged or committed activation is live.
- Unique `ArenaId`: the `0` value is reserved (`InvalidConfig`); IDs must
  be unique among simultaneously live arenas (a caller configuration
  error this core cannot detect beyond its own token check).
- Staging semantics: dropping an uncommitted `Reservation` rolls its
  staging back; `Reservation::commit` publishes it (infallible, keeps the
  full reserved tail); `PhaseArena::release` consumes the token and
  restores full capacity. A failed release returns a `ReleaseFailure`
  holding the original token and changes nothing.
- Reservation-based accounting (`requested`, monotonic `high_water` and
  `alignment_waste`) and generation retirement (release at `u32::MAX`
  retires the arena; the generation never wraps). All arithmetic checked.

## When to use it

- A same-lifetime bundle (for example per-screen staging) needs checked,
  aligned, fragmentation-free offset allocation with all-or-nothing
  staging and unit reset.

## When not to use it

- Lifetimes differ per allocation or blocks must release in any order:
  use the transfer-cache block pool instead. This arena exposes no
  arbitrary `free`.
- The caller needs owned bytes from `PhaseArena`: its spans remain offsets
  only. Use `RuntimeBundle<A>` for independent allocator-backed parts.
- The caller needs DMA, policy handles, leases, or eviction: those live
  outside this crate.

## Example

```rust
use phase_arena::{ArenaId, PhaseArena};

let mut arena = PhaseArena::new(ArenaId(1), 16, 8).unwrap();
let token = {
    let mut reservation = arena.reserve(16).unwrap();
    let a = reservation.alloc(1, 1).unwrap();
    let b = reservation.alloc(1, 8).unwrap();
    assert_eq!((a.offset, a.len), (0, 1));
    assert_eq!((b.offset, b.len), (8, 1)); // 7 padding bytes counted as waste
    reservation.commit()
};
assert_eq!(arena.requested(), 16);
arena.release(token).unwrap();
assert_eq!(arena.requested(), 0);
```

## Safety, lifetime, and concurrency constraints

- No `unsafe`; `Reservation` borrows the arena mutably, so only one
  staged activation can exist (enforced by the borrow checker plus the
  `Busy` check).
- `BackingToken` is non-`Copy` with private fields: only `commit` mints
  a live token, so stale or foreign tokens fail `release` as `WrongToken`.
- No thread-safety claims: sharing across threads needs caller-side
  synchronization.

## Maturity and deferred integrations

- Implemented and unit-tested: checked aligned spans, rollback-on-drop,
  release/retry errors, accounting, generation retirement.
- `RuntimeBundle<A>` is host-tested and used by Meditamer's Mountain, sky/sun,
  and Clock paths through a strict-PSRAM target adapter.
- Policy-participant integration, child leases, DMA, and cache eviction are
  not provided by this crate.
