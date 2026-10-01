# asset-source

`no_std`, dependency-free checked read/borrow core for bulk asset bytes.

## What this crate owns

- The `AssetSource` trait: a read-only random-access contract over one
  logical byte string (`len`, `read_exact_at`, `borrow_at`).
- The `AccessError` type (`Overflow`, `OutOfBounds` with offset/len/capacity).
- Two adapters: `SliceSource` (one contiguous borrowed slice) and
  `FragmentedSource` (borrowed spans concatenated in order; empty spans
  skipped).
- Fully checked range arithmetic: wrapping addition reports `Overflow`
  before any capacity comparison; an empty range at exactly the capacity
  is valid for both operations.

A `None` borrow is never an error: it means a valid fragmented range
that is not contiguous in memory (a `FragmentedSource` range crossing
a span boundary). The caller falls back to `read_exact_at` with bounded
scratch for those ranges. `SliceSource::borrow_at` never returns `None`.

## When to use it

- A codec needs one offset-based read contract over contiguous slices,
  fragmented spans, or (later) resident arena/block storage.
- The caller wants zero-copy borrow on the fast path with an explicit
  copy fallback for cross-span ranges.

## When not to use it

- The source needs writing, prefetch, caching, or eviction: this trait
  is read-only and owns no cache.
- The caller needs validated formats (magic, geometry, CRC), policy
  handles, leases, or generations: those live in other crates/tracks.
- The caller needs target, board, SD, or screen-lifetime code: this
  crate borrows caller-owned memory and allocates nothing.

## Example

```rust
use asset_source::{AssetSource, FragmentedSource, SliceSource};

let bytes = [10u8, 20, 30, 40, 50];
let src = SliceSource::new(&bytes);
let mut dst = [0u8; 3];
src.read_exact_at(1, &mut dst).unwrap();
assert_eq!(dst, [20, 30, 40]);

let spans: [&[u8]; 2] = [&bytes[..2], &bytes[2..]];
let frag = FragmentedSource::new(&spans).unwrap();
match frag.borrow_at(1, 3).unwrap() {
    Some(span) => assert_eq!(span, &[20, 30, 40]),
    None => {
        // Valid range crossing a span boundary: fall back to copy.
        let mut tmp = [0u8; 3];
        frag.read_exact_at(1, &mut tmp).unwrap();
        assert_eq!(tmp, [20, 30, 40]);
    }
}
```

## Safety, lifetime, and concurrency constraints

- No `unsafe` in this crate; adapters borrow caller-owned spans, so every
  borrow is bounded by the source's lifetime.
- `FragmentedSource::new` totals span lengths with checked arithmetic;
  a total-length overflow reports `Overflow`.
- No locks, no threads, no `Sync` claims: sharing is the caller's
  business; borrows follow normal Rust borrow rules.

## Maturity and deferred integrations

- Implemented and unit-tested: contiguous/fragmented adapters, boundary
  and overflow behavior, empty-range-at-capacity rules.
- Deferred: policy handle/lease binding, the validated wrapper, SD and
  managed-memory adapters, and codec integration.
- Not provided here: validated codecs, cache eviction, target backing,
  DMA, or production pack consumers.
