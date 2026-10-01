# Trouble host 0.8.0: release backing packets on host teardown

The published 0.8.0 source places connection and channel slots in
`HostResources` as `MaybeUninit` storage. `new()` initializes them with
`MaybeUninit::write`; `Stack` and `StackBuilder` drop `HostState`, whose managers
borrow those slots. Previously, dropping the host did not drop packets held by
the slots, and a new host could overwrite an incomplete reassembly or queued
GATT/L2CAP packet without releasing it.

`ConnectionManager::drop` and `ChannelManager::drop` now replace each borrowed
slot with its empty initial value. Ordinary assignment drops the old value,
including packet owners and registered wakers. The slots stay initialized and
empty until the next `new()`. This introduces no unsafe code, new allocation,
layout change, or controller behavior. The destructors are outlined so slot
initializers do not inflate caller stack frames. Rust's host borrows ensure no runner or
connection can outlive its manager.

The regression `dropping_stack_releases_backing_packets_before_resource_reuse`
retains counted packets in a connection reassembly and a channel receive queue,
then verifies exact release counts across two stack epochs using the same
`HostResources`. The original manager implementation fails on the first drop;
the patched implementation releases both packets once per epoch.

Run `cargo test --manifest-path vendor/trouble-host-0.8.0-restartable/Cargo.toml --lib`.
Controller callback shutdown and transport/queue quiescence remain the caller's
responsibility; this patch only fixes host-owned backing storage cleanup.
