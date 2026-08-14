# Meditamer exact allocation low-water provenance patch

Status: repository-owned Phase 1S diagnostic patch

## Immutable base

- Package: `esp-alloc` 0.10.0
- Crates.io checksum: `46ced060d4085858283df950b80a4da2348e1707d7d07b1e966308582dae79f5`
- Upstream source revision: `347003de8a48320bb7724f53045be3afa9204411`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `b24bdd8bc2f21a1de7786bbbe0ab33545ec3798c525be8cd76807c6c6f0bdc8e`

## Maintained delta

Upstream invokes its allocation hook only after releasing the heap mutex. A
hook that then queries free memory can attach one allocation's requested size
to a later allocation's low-water on the other ESP32 core.

This patch measures internal free bytes immediately before and after the
allocation while the existing allocator mutex still serializes every region.
It passes those immutable values to the application hook only after releasing
the non-reentrant mutex. The hook remains allocation-free and publishes the
winning free/charge/capability tuple through one native atomic word.

Deallocation completion is reported from inside the same allocator mutex,
immediately after a region accepts the exact pointer and layout. The product's
atomic-only correlation hook can therefore retire a low-water generation before
the address becomes reusable, without querying or re-entering the allocator.

## Maintenance rule

The root manifest must exactly pin and resolve this path. The source guard
binds the reviewed hook ABI, measurement-under-lock, hook-after-unlock order,
and patched tree digest. Any allocator version, hook ABI, mutex boundary, heap
algorithm, or low-water record layout change reopens the Phase 1S audit.
