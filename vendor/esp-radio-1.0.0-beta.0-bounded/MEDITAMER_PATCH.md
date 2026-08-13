# Meditamer bounded BLE transport patch

Status: repository-owned Phase 1 candidate

## Immutable base

- Package: `esp-radio` 1.0.0-beta.0
- Crates.io checksum: `0f25cc4e3ce27476b42c4a68943f10a92f9dec3c24bb001269958f0318fef02c`
- Upstream source revision: `b4c8d9bc634373bc140df1c3c83ba42706a55944`
- License: MIT OR Apache-2.0
- Repository ownership authorized: 2026-08-11
- Patched crate-tree SHA-256 (excluding this manifest):
  `13b5b5bed520b5a96f2fe250bb8de3d6ae7ae4223e83b995cd233788d356540b`

The packaged source was copied without modification before applying the changes documented below.
Changing the base version, checksum, feature union, capacity, timeout, or overflow policy reopens
BLE Phase 1.

## Maintained delta

The accepted patch changes the ESP32 BTDM HCI transport and its connector error propagation:

- replace receive-path `Box` allocation and unbounded `VecDeque` growth with four fixed 259-byte
  `heapless` packet slots;
- drop the newest receive packet when the queue is full and expose overflow, oversize, and queue
  high-water counters;
- reject transmit assembly beyond 259 bytes rather than indexing beyond the collector;
- serialize access to the fixed transmit collector with a static async mutex;
- await callback-woken VHCI availability and completion with independent Embassy timer-backed 100 ms
  deadlines, and latch a transport fault until controller reinitialization so a late callback cannot
  acknowledge a later packet;
- latch the same fault if an in-progress send future is cancelled, and make queued senders recheck it
  after collector acquisition so partial or in-flight data cannot be appended or resent;
- count every VHCI callback before checking admission, reject ingress after shutdown begins, disable
  the controller callback source exactly once, and expose a deadline-bounded quiescence wait before
  callback-reachable storage is released;
- expose bounded live/peak Wi-Fi RX packet and payload counters so a stable before/after ownership
  window can correlate internal-heap low water with vendor-owned dynamic packet buffers without
  changing queue capacity;
- retain at most one station receive callback packet after callback entry and before queueing,
  rejecting and returning a contending vendor buffer before `PacketBuffer` construction;
- propagate bounded transport failures through `BleConnectorError`; and
- reset queues and counters at controller initialization and teardown; and
- normalize one upstream edition-sensitive `let` chain without changing its read behavior so the
  maintained tree passes the repository formatter.

The shared compat-queue path now registers every queue in eight fixed lifecycle slots and fences all
operations before dereference. The lower driver owns queue controls in eight fixed internal slots;
deletion atomically retires a queue and explicit reclamation occurs only after a source-scoped epoch
has disabled and quiesced its callbacks. BLE initialization opens such an epoch and BTDM teardown
must retire every epoch queue, return with zero HCI callbacks in flight, and reclaim every slot.
Unretired queues, operations in flight, lower-owner rejection, canary damage, or pool exhaustion are
hard failures. The earlier queue-lifetime diagnostic allocation hooks, private-layout header probe,
and deliberate leak are removed. Fixed queue payload uses a static first-fit arena; operations use
bounded per-queue raw-lock
copies and timer-sleeping task waits rather than the upstream reentrant compat-semaphore/wait-queue
path. ISR operations never enter the task scheduler, and task/ISR nested-use rejection is counted.

Wi-Fi's adapter now returns the documented `wifi_static_queue_t` two-pointer C layout. The compat
queue owns its payload, so the second storage pointer is null, but the wrapper allocation preserves
the vendor ABI rather than allocating only the first handle field.

Each restartable Wi-Fi controller now opens a source-scoped compat-queue epoch. Explicit fallible
shutdown first revokes callback admission and deinitializes the source, then the supervisor waits for
callback/TX quiescence before finalization releases the radio reference and reclaims retired slots.
Construction failures run the same cleanup and return `CleanupFailed` if ownership cannot be proved;
implicit drop never unconditionally reclaims ambiguous storage. This makes repeated exclusive radio
handoff epochs reuse the fixed queue registry without converting cleanup failures into panics.

ESP-RTOS 0.3.0 deletes a radio task by freeing its task/stack allocation without unwinding Rust
locals. A task blocked in a compat queue can therefore lose its `QueueUseGuard` destructor. Queue
operations are now tracked in a bounded registry by queue epoch, task identity, and generation. The
BTDM task-delete adapter cancels only records owned by the task after synchronous non-current-task
deletion (or immediately before non-returning self-deletion). ISR and unknown-owner records remain
strict blockers. A stale guard cannot decrement a reused record, and reclamation also requires zero
live BTDM tasks plus exact started/completed/cancelled operation balance. Teardown failures latch
observable ownership ambiguity instead of panicking and restoring Wi-Fi over uncertain radio state.

The host-only TX cancellation tests also serialize access to their shared fault latch. This does not
change target code; it removes a parallel-test race that could make the unchanged disarm case fail.

Modified upstream files:

- `Cargo.toml` (adds the existing exactly pinned `embassy-time` 0.5.1 runtime dependency)
- `src/ble/mod.rs`
- `src/ble/btdm.rs`
- `src/ble/controller/mod.rs`
- `src/ble/tx_cancellation.rs`
- `src/compat/mod.rs`
- `src/compat/queue.rs`
- `src/compat/queue_lifecycle.rs`
- `src/lib.rs`
- `src/wifi/os_adapter/mod.rs`
- `src/wifi/mod.rs`

## Maintenance rule

Run `scripts/ci/check_ble_controller_patch.sh` and both locked firmware builds before accepting a
change. An upstream update must be audited as a new immutable base. Remove this patch only after the
resolved maintained dependency independently satisfies `BLE-BOUND-01` and `BLE-BOUND-02` from the
BLE plan.
