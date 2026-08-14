# Meditamer fixed compat-queue owner patch

Status: repository-owned Phase 1 candidate

## Immutable base

- Package: `esp-radio-rtos-driver` 0.3.0
- Crates.io checksum: `0bd75cd9073a90ffaa53db0bf17df7dc14164f2407a6ff36c725d2d1f78ff494`
- Upstream source revision: `347003de8a48320bb7724f53045be3afa9204411`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `6210f7f63f290aaf6bd412faa6de8ae18dff225fdf340b25daf7b61eb7fe0e1f`

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

The directory retains its diagnostic-era name only to avoid an unrelated path-only vendor-tree
rename. The retained-allocation behavior itself is gone: retired storage is reclaimable through the
source-quiescent contract.

## Maintenance rule

Run `scripts/ci/check_ble_controller_patch.sh`, strict BLE Clippy, and both locked firmware builds
after any change. Changing slot count, payload ceilings, canaries, state transitions, or reclamation
preconditions reopens BLE Phase 1.
