# Meditamer bounded BLE transport patch

Status: repository-owned Phase 1 candidate

## Temporary connect-phase incident probe (2026-09-28)

The opt-in `meditamer-connect-phase-probe` feature records an attempt number
and the last `WifiController::connect_async` phase in RTC slow memory. It marks
event subscription, the entry/return of `esp_wifi_connect_internal`, event
wait/receipt, and future completion or cancellation. No connect behavior,
queue capacity, retry policy, or radio setting is changed. The Inkplate
target's temporary `context-probe` feature reads this marker after a
timer-group watchdog reset; remove it once the Mountain/radio hang is
attributed. The marker is not expected to survive a host DTR/RTS power-on
reset, so missing evidence after such a reset is inconclusive.

## Immutable base

- Package: `esp-radio` 1.0.0-beta.1
- Crates.io checksum: `50247f4e91000fc87661e77254126c74acc4535f822ee247f7778807202dc938`
- Upstream source revision: `f769fcd27056b0837febe4209f3bae78ed764561`
- Blobs: Wi-Fi and BLE rebased to ESP-IDF `6.1-255-g4dbfecac7e3` (upstream #6271)
- License: MIT OR Apache-2.0
- Repository ownership authorized: 2026-08-11
- Patched crate-tree SHA-256 (excluding this manifest):
  `951842dea4666da29a2e0ec137d8585c24827d5bc75518665fee8b9b1499858a`

The packaged source was copied without modification before applying the changes documented below.
Changing the base version, checksum, feature union, capacity, timeout, or overflow policy reopens
BLE Phase 1. This base change (from `1.0.0-beta.0-bounded`) reopens BLE Phase 1 by that rule;
re-qualification runs on the IDF 6.1 blobs, which are the point of the rebase (radio-era
stray-write hunt, `docs/notes/mountain-streaming-hangs-verdict-2026-09-27.md`).

The packaged source was copied without modification before applying the changes documented below.

## Maintained delta (ported from the beta.0 fork, adapted to beta.1)

The beta.0 review surface is preserved in substance; beta.1's own rework of the
same transport area (upstream #6257 non-blocking HCI, #6030 `bt-hci-transport`
0.1 implementation, `bt-hci` 0.8 -> 0.9) is superseded by the bounded design
where they overlap:

- BLE HCI transport keeps the reviewed bounded shape: four fixed 259-byte
  `heapless` RX slots with overflow/oversize/high-water counters, fixed TX
  collector with 100 ms Embassy deadlines, transport fault latch, callback
  admission fence with in-flight accounting, and source-quiescent reclamation.
  Upstream beta.1's heap-`Box` receive path (`take_next`), `VecDeque` queue,
  `HciPacketReadyEventFuture`, `collect_and_send`, and deadline-free
  `send_hci_async` are removed; the bounded `send_hci` (async, `Result`),
  fixed-slot `read_next`/`read_hci`, and `HciReadyEventFuture` are retained.
- `BleConnector` keeps both transport generations on the bounded path: the
  retained `bt-hci` 0.9 `transport::Transport` impl (staged fixed buffer) and
  the `bt-hci-transport` 0.1 impl (fixed staging array, no heap). Upstream's
  Box-based 0.1 `read` is superseded.
- Wi-Fi keeps the restartable exclusive-epoch controller (`WifiController`
  with `WifiQueueEpochGuard`, `shutdown_source`/`finalize_shutdown`,
  construction-error cleanup, RX ownership telemetry, allocator correlation
  hook). Upstream beta.1's refcounted shared controller (`WifiRefGuard`,
  `WIFI_REFCOUNT`, `Refcount::try_increment`) is removed: exclusive handoff
  epochs never increment the Wi-Fi init refcount, so keeping it would corrupt
  init accounting. The radio-level `RADIO_REFCOUNT` is untouched.
- The beta.1 init config for the IDF 6.1 blob is adopted verbatim
  (`sta_disconnected_pm`, `espnow_max_encrypt_num` from `ControllerConfig`,
  `privacy_enhancements`, `rmac_auto_reset_int`); only the refcount wrapper
  around it is replaced by the epoch flow.
- `esp-now`/`sniffer` handles keep upstream beta.1's bodies (they construct
  from the removed `WifiRefGuard` and only compile with those features, which
  no tree target enables). Enabling either feature requires reconciling its
  keep-alive guard with the exclusive-epoch model first.
- `bt-hci-transport` 0.1.0, `embassy-time` 0.5.1 (exact), and the
  `meditamer-allocation-provenance` / `queue-host-harness` features are
  carried; all beta.0-era dependency widenings are deleted because beta.1
  already pins esp-hal ~1.2.0, esp-alloc 0.11, esp-rtos 0.4.0,
  esp-radio-rtos-driver 0.4.2, esp32 0.42, and esp32s3 0.37.
- Classic-memory reclaim, TX-cancellation bridge/tests, compat queue
  lifecycle registry, `WifiStaticQueue` ABI wrapper, and the ESP32-S3
  `extern crate alloc` lint relaxation carry over unchanged in intent (see
  prior sections, retained below).

The shared compat-queue path now registers every queue in eight fixed lifecycle slots and fences all
operations before dereference. The lower driver owns queue controls in eight fixed internal slots;
deletion atomically retires a queue and explicit reclamation occurs only after a source-scoped epoch
has disabled and quiesced its callbacks. BLE initialization opens such an epoch and BTDM teardown
must retire every epoch queue, return with zero HCI callbacks in flight, and reclaim every slot.
Unretired queues, operations in flight, lower-owner rejection, canary damage, or pool exhaustion are
hard failures. The earlier queue-lifetime diagnostic allocation hooks, private-layout header probe,
and deliberate leak are removed. The product's separate allocator low-water hook is acceptance
telemetry and does not inspect queue layout or alter queue lifetime. Fixed queue payload uses a static first-fit arena; operations use bounded per-queue raw-lock
copies and timer-sleeping task waits rather than the upstream reentrant compat-semaphore/wait-queue
path. ISR operations never enter the task scheduler, and task/ISR nested-use rejection is counted.

Wi-Fi's adapter now returns the documented `wifi_static_queue_t` two-pointer C layout. The compat
queue owns its payload, so the second storage pointer is null, but the wrapper allocation preserves
the vendor ABI rather than allocating only the first handle field.

Each restartable Wi-Fi controller now opens a source-scoped compat-queue epoch. Explicit fallible
shutdown first revokes callback admission and deinitializes the source, then the supervisor waits for
callback/TX quiescence before finalization releases the radio reference and reclaims retired slots.
Packet admission closes before the first fallible vendor-stop operation; finalization also rejects
an open admission fence. Event callbacks use source liveness until source deinitialization completes.
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
- `src/ble/os_adapter_esp32.rs`
- `src/ble/controller/mod.rs`
- `src/ble/tx_cancellation.rs`
- `src/compat/mod.rs`
- `src/compat/queue.rs`
- `src/compat/queue_lifecycle.rs`
- `src/lib.rs`
- `src/refcount.rs` (removes the Wi-Fi-init `try_increment`; exclusive epochs never use it)
- `src/wifi/os_adapter/mod.rs`
- `src/wifi/mod.rs`

## Maintenance rule

Run `scripts/ci/check_ble_controller_patch.sh` and both locked firmware builds before accepting a
change. An upstream update must be audited as a new immutable base. Remove this patch only after the
resolved maintained dependency independently satisfies `BLE-BOUND-01` and `BLE-BOUND-02` from the
BLE plan.

## ESP32 beta.1 BLE-only ROM mappings

The IDF 6.1 `libbtdm_app.a` references three Classic inquiry/page-scan ROM
callbacks (`ld_iscan_evt_start_cbk`, `ld_page_evt_start_cbk`,
`ld_pscan_evt_start_cbk`). The exact `esp-rom-sys` 0.1.5 dependency lacks their
linker mappings, although upstream added them in esp-rs/esp-hal#6271.
`config/linker/esp32/meditamer-linkall.x` temporarily supplies those three
upstream ROM addresses with `PROVIDE`; these are not callback stubs. The
Inkplate `ble-shared-runtime-probe` now links in release mode. This is link
evidence only; BLE-only operation still needs device qualification. Remove the
local mappings when a compatible released `esp-radio`/`esp-rom-sys` dependency
pair provides them.

## ESP32 BLE-only Classic memory release (2026-09-21, carried)

The pinned ESP32 controller configuration is BLE-only and permits zero Classic
ACL connections, but the base Rust adapter still zeroed the controller table's
Classic-only `0x3ffb2730..0x3ffb6388` range on every BLE initialization. That
prevented the application from using the **15,448 bytes** which ESP-IDF returns
through `esp_bt_controller_mem_release(ESP_BT_MODE_CLASSIC_BT)`.

The maintained adapter now exposes one ownership token before the first
controller epoch. An atomic state makes release and controller startup mutually
exclusive, rejects duplicate/late release, and makes every later BLE epoch skip
the released Classic entry while retaining initialization of BLE and shared
BTDM regions. The token and its raw-parts transfer warn when discarded without
installing a permanent owner. Compile-time assertions pin BLE-only mode, zero Classic ACL
connections, and the exact region size.

Meditamer consumes the token during allocator initialization and installs the
range as the second `esp-alloc` region with internal-memory capability. The
ordinary `dram2_seg` internal heap remains the first region and PSRAM is added
third, consuming the final slot; non-BLE images never enable the reclaim
feature. This is separate heap capacity, not contiguous CPU0 stack.
Changing heap region count reopens the documented 40-boot and radio-handoff
device qualification; target builds and source guards alone do not close it.

## Trouble 0.8 transport bridge (2026-08-26, carried)

Trouble 0.8 uses `bt-hci` 0.10 for host types and consumes the wire contract through the independent
`bt-hci-transport` 0.1 crate.

The connector bridge is retained on the bounded path: the writer uses one fixed 259-byte stack
staging array and awaits the maintained `send_hci(buf)` path, preserving the existing size check,
serialization, cancellation fault latch, and 100 ms deadlines. It adds no heap allocation and does
not change queue capacity or overflow policy. The base `bt-hci` 0.9 transport implementation remains
for the vendor crate's internal controller integration; the two host API versions meet only at the
version-neutral transport trait.

Upstream beta.1 added its own `bt-hci-transport` 0.1 implementation (#6030), but its receive path
heaps a `Box` packet and its transmit path has no deadline, so it does not independently satisfy
`BLE-BOUND-01` or `BLE-BOUND-02`; removing this maintained fork is not yet safe.

## BLE-only lint relaxation for btdm chips (2026-08-24, carried)

Shared BLE runtime and roles plan, Phase 1
(`docs/plans/shared-ble-host-and-role-capabilities.md`): extending the reviewed BLE controller
source to ESP32-S3 surfaced a build failure never hit before, because nothing had previously built
this crate with `ble` enabled and `wifi` disabled on a `btdm`-controller chip (ESP32, ESP32-S3; the
`npl` controller chips — C3/C6/H2/... — are unaffected, see below). `src/lib.rs`'s
`extern crate alloc;` is the only crate-root item this touches; every `alloc::` call site in the
crate lives in `wifi/`, `ieee802154/`, or the `npl` BLE controller (`src/ble/npl.rs`), none of which
compile in under that exact feature combination, so `#![deny(rust_2018_idioms)]` flags the `extern
crate` as unused and the build fails before reaching any BLE controller logic.

Added a narrow, item-level `#[cfg_attr(..., allow(unused_extern_crates, ...))]` directly on
`extern crate alloc;`, conditioned on `not(any(feature = "wifi", feature = "ieee802154",
bt_controller = "npl"))` — relaxes exactly one lint, only when it is providably a false positive.
Does not touch the crate's existing broader `allow(unused)` block (which covers the unrelated
"neither wifi nor ble" case) and changes no runtime behavior: the global allocator such a build
still needs is `esp-alloc`, wired in by `platform/connectivity/ble`'s own `esp-radio` feature request
(`esp-alloc`), not by this crate's own `alloc::` call sites.

## Optional allocator correlation hook (carried)

The `meditamer-allocation-provenance` feature gates the Inkplate allocator
correlation callback in station receive handling. Inkplate enables it explicitly;
other targets can use the driver without providing a product-specific symbol.
The feature does not change packet handling or allocation behavior.
