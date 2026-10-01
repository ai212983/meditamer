# Meditamer exact allocation low-water provenance patch

Status: repository-owned Phase 1S diagnostic patch

## Immutable base

- Package: `esp-alloc` 0.11.0
- Crates.io checksum: `f73e8d355019c740f6a5a78429633040617d28a378c271464caadc52c78216e3`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `b79c178cce81e9985f6a6ddc0a9cd50fdede681f90628b5d6d7bdf0fb3cd6890`

## History

This folder replaces `esp-alloc-0.10.0-provenance`. The esp-hal 1.2.0 upgrade required esp-alloc
0.11.0 (esp-hal's own `esp-sync` requirement moved to 0.3.0, which 0.10.0's `esp-sync = "0.2.1"`
pin cannot satisfy alongside everything else in the graph). The patch below is unchanged in
substance from the 0.10.0 version -- diffed against the unmodified 0.11.0 tarball, it is exactly
the delta described here and nothing else; upstream's own 0.10.0->0.11.0 changes (e.g. `cfg_if!`
-> `cfg_select!`, an unrelated doc-logo URL update, a dealloc-loop rewrite this patch's hook call
was re-threaded onto) are not part of this patch and are carried through unmodified.

## Maintained delta

Upstream invokes its allocation hook only after releasing the heap mutex. A hook that then queries
free memory can attach one allocation's requested size to a later allocation's low-water on the
other ESP32 core.

This patch measures internal free bytes immediately before and after the allocation while the
existing allocator mutex still serializes every region. It passes those immutable values to the
application hook only after releasing the non-reentrant mutex. The hook remains allocation-free and
publishes the winning free/charge/capability tuple through one native atomic word.

Deallocation completion is reported from inside the same allocator mutex, immediately after a
region accepts the exact pointer and layout. The product's atomic-only correlation hook can
therefore retire a low-water generation before the address becomes reusable, without querying or
re-entering the allocator.

Builds without `alloc-hooks` explicitly consume the two sampled free-byte values after the
feature-gated hook call. This preserves the single allocation path and always-sample behavior while
keeping non-hook consumers warning-clean.

## Maintenance rule

The root manifest must exactly pin and resolve this path. The source guard binds the reviewed hook
ABI, measurement-under-lock, hook-after-unlock order, and patched tree digest. Any allocator
version, hook ABI, mutex boundary, heap algorithm, or low-water record layout change reopens the
Phase 1S audit. `scripts/ci/check_network_owner_source.sh` enforces the pin, path, and tree digest.
