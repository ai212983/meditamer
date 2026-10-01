# Meditamer fixed compat-queue owner patch

Status: repository-owned Phase 1 candidate

## Immutable base

- Package: `esp-radio-rtos-driver` 0.4.2
- Crates.io checksum: `27d3ad9d7ed96260566acd1a69a908ff805cfaa73a936ae445c1252e1ec983be`
- Upstream source revision: `f769fcd27056b0837febe4209f3bae78ed764561`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `98ff7d137198ccf6d3147fc965ba78c136fb989d5a9d34507ccc3c6bcb877088`

## History

This folder replaces `esp-radio-rtos-driver-0.4.1-retained`. Upstream
0.4.1 -> 0.4.2 changes no Rust source (verified by diff: only `CHANGELOG.md`,
`Cargo.toml` version/feature, and VCS metadata differ), so the `src/queue.rs`
rewrite below carries over verbatim. `Cargo.toml`'s only delta from upstream
remains the added `xtensa-lx` dependency below.

The 0.4.1 folder's history is retained for context: it replaced
`esp-radio-rtos-driver-0.3.0-retained` for the esp-hal 1.2.0 upgrade, with the
patch unchanged in substance (0.3.0 -> 0.4.1 only added `Send`/`Sync` for
`QueueHandle` outside the rewritten block).

## Maintained delta

The upstream `CompatQueue` allocates its control object with `Box` and drops it immediately when an
opaque radio owner requests deletion. That lifetime is unsafe when a callback can race deletion and
is not suitable for repeated BLE controller epochs.

This patch replaces the heap-owned control object with eight fixed internal slots. Creation claims an
empty slot atomically, deletion only transitions the slot to retired, and the separate unsafe
`compat_queue_reclaim` operation drops queue storage only after the caller proves that its
callback source is disabled and quiescent. Reuse is therefore explicit rather than timing based.

Each control slot and payload region has before/after canaries. Payload comes from a fixed 4 KiB
static first-fit arena; callback-time and queue-lifecycle payload allocation no longer touch the
heap. Queue items are copied under a
per-queue reentrant raw lock with a 512-byte item ceiling; a `RefCell` rejects nested same-core access
without creating aliased mutable references. Task-context waits use the RTOS wait queue with a
bounded 1 ms timer deadline, matching the radio stack's native tick. Task-context sends wake the
waiter immediately; ISR operations defer wakeup to the timer deadline and never enter the task
scheduler. Nominally blocking calls made with a raised Xtensa interrupt level are detected before
taking the queue lock, reduced to one bounded attempt, and counted. This removes the upstream queue's
reentrant `CompatSemaphore` path and its ISR wait-queue notification, which exact boots proved can
panic or fault when `pp_post` uses the ISR adapter. Each active queue has two bounded task-context
wait-queue allocations that are reclaimed with the source-quiescent queue. Queue payloads have a 2 KiB
per-queue ceiling and a 2 KiB aggregate ceiling; slot or payload exhaustion is a hard fault. Runtime
statistics expose active, retired, reclaimed, corruption, task/ISR contention rejection,
nonblocking-context redirects, payload, and high-water values.

The task-context wait is outside the per-queue raw lock. Registering a wait can acquire the global
RTOS scheduler lock and request a task switch. The September 2026 stall capture found CPU0
spinning at the `receive_with_deadline` lock acquire; moving the wait removes a lock-order hazard
that could sustain that state, though the capture alone does not identify the lock holder.
A send in the small gap between the queue check and wait is observed by the existing 1 ms poll.

Re-verified against esp-rtos 0.4.0 (the version esp-hal 1.2.0 pulls in): `delete_task` in
`esp-rtos`'s `src/scheduler.rs` still frees the task's stack allocation (via `Box::from_raw_in` /
`drop_in_place` on the `Task` control struct) without unwinding whatever the task's own stack was
still running -- the invariant this patch's `queue_lifecycle` bounded registry exists to work around
(a `QueueUseGuard` living as a local on a deleted task's stack never runs its own `Drop`).

## Maintenance rule

Run `scripts/ci/check_ble_controller_patch.sh`, strict BLE Clippy, and both locked firmware builds
after any change. Changing slot count, payload ceilings, canaries, state transitions, or reclamation
preconditions reopens BLE Phase 1.
