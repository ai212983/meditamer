# Meditamer bounded BLE transport patch

Status: repository-owned Phase 1 probe patch

## Immutable base

- Package: `esp-radio` 1.0.0-beta.0
- Crates.io checksum: `0f25cc4e3ce27476b42c4a68943f10a92f9dec3c24bb001269958f0318fef02c`
- Upstream source revision: `b4c8d9bc634373bc140df1c3c83ba42706a55944`
- License: MIT OR Apache-2.0
- Repository ownership authorized: 2026-08-11
- Patched crate-tree SHA-256 (excluding this manifest):
  `4019a3738d1b312acd55030b26ac41691ebecb592bd1b3c0f91285b63d403a93`

The packaged source was copied without modification before applying the changes documented below.
Changing the base version, checksum, feature union, capacity, timeout, or overflow policy reopens
BLE Phase 1.

## Maintained delta

The patch changes only the ESP32 BTDM HCI transport and its connector error propagation:

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
- propagate bounded transport failures through `BleConnectorError`; and
- reset queues and counters at controller initialization and teardown; and
- normalize one upstream edition-sensitive `let` chain without changing its read behavior so the
  maintained tree passes the repository formatter.

Modified upstream files:

- `Cargo.toml` (adds the existing exactly pinned `embassy-time` 0.5.1 runtime dependency)
- `src/ble/mod.rs`
- `src/ble/btdm.rs`
- `src/ble/controller/mod.rs`
- `src/ble/tx_cancellation.rs`

## Maintenance rule

Run `scripts/ci/check_ble_controller_patch.sh` and both locked firmware builds before accepting a
change. An upstream update must be audited as a new immutable base. Remove this patch only after the
resolved maintained dependency independently satisfies `BLE-BOUND-01` and `BLE-BOUND-02` from the
BLE plan.
