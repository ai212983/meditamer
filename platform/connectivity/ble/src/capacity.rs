//! Shared bounded capacities (shared BLE runtime and roles plan, Phase 2:
//! "Give buffers and collections explicit capacities").
//!
//! Centralized so a capacity change is auditable in one place instead of
//! scattered magic numbers, matching this repository's DRAM-budget
//! discipline (`docs/references/memory/README.md`) for every other bounded
//! buffer. Every constant here is a first-cut Phase 2 value sized to the
//! documented reason next to it, not yet a measured per-target budget --
//! Phase 2's own exit criteria defers "linked memory, fixed controller
//! allocations, task pools, and image growth" to per-target resource
//! evidence -- pairing/bonding's own dependency and flash cost (ADR-0016)
//! reopens that same measurement gate rather than being exempt from it.

/// Bounded set of scan results kept per window (plan Phase 3: "keep a
/// bounded set of results"). Eight covers a typical bench/desk BLE
/// environment (a handful of phones, watches, and beacons) with headroom
/// for the one chosen advertising device to always have a slot.
pub const SCAN_RESULTS_MAX: usize = 8;

/// Longest BLE advertised local name a [`crate::scan`] result stores.
/// `bt-hci`'s `AdStructure::CompleteLocalName`/`ShortenedLocalName` payload
/// is itself capped by the legacy `ADV_IND` payload (31 bytes total, minus
/// the AD structure's length/type octets), so 31 never truncates a
/// spec-legal name.
pub const SCAN_NAME_MAX: usize = 31;

/// Widest HID report descriptor [`crate::hid`] discovers and stores. The
/// standing CheerTok fixture is a composite keyboard/mouse/consumer/touch
/// device whose legal map exceeds 256 bytes, so the measured bound is 512.
/// A wider descriptor is rejected outright rather than truncated (plan
/// Phase 5: "malformed input" must be a distinguishable outcome, not
/// silently corrupted data).
pub const HID_DESCRIPTOR_MAX_BYTES: usize = 512;

/// Longest single HID input report [`crate::hid`] decodes. Covers a
/// button-bitmask-plus-hat-plus-six-axes report (the common gamepad shape)
/// with headroom; oversize reports are rejected, not truncated.
pub const HID_REPORT_MAX_BYTES: usize = 16;

/// Most HID Report characteristics retained for one peer. CheerTok exposes
/// six; eight leaves bounded room for a distinct keyboard, mouse, and
/// gamepad report set without making descriptor discovery unbounded.
pub const HID_REPORTS_MAX: usize = 8;

/// Most raw HID notifications retained by a one-shot physical capture.
/// Sixteen covers press/release pairs for every input report on the standing
/// CheerTok fixture while keeping the probe's task-local storage fixed.
pub const HID_NOTIFICATION_CAPTURE_MAX: usize = 16;

/// Most simultaneous analog axes one [`crate::input::GamepadSample`] carries
/// in descriptor order (for example four stick axes plus two triggers).
pub const GAMEPAD_AXES_MAX: usize = 6;

/// Most simultaneous digital buttons one
/// [`crate::input::GamepadSample`]'s bitmask covers -- a `u16`'s width.
pub const GAMEPAD_BUTTONS_MAX: usize = 16;

/// Bounded, ordered queue of pending button edges between two
/// [`crate::input::GamepadSample`] publications, tagged by generation.
/// Sixteen covers several frames'
/// worth of simultaneous button churn at typical BLE HID notify intervals
/// before the queue's own overflow-clears-and-reconciles behavior takes
/// over.
pub const EDGE_QUEUE_MAX: usize = 16;

/// Most simultaneous bounded connections [`crate::gatt`] tracks. Diagnostic
/// v1 style: one connection today, but the state machine itself is not
/// hardcoded to one, since Phase 8 needs a gamepad central connection and a
/// product peripheral connection open at the same time.
pub const GATT_CONNECTIONS_MAX: usize = 2;

/// Most services one [`crate::gatt::session::GattSession`] discovers. The
/// existing `ble-foundation` diagnostic peripheral exposes three
/// characteristics (Build Info, Echo, Lifecycle Status) under one custom
/// service; this leaves headroom for a central role discovering an
/// unfamiliar peripheral's full GATT table.
pub const GATT_SERVICES_MAX: usize = 8;

/// Most characteristics one [`crate::gatt::session::GattSession`] discovers
/// across all its services.
pub const GATT_CHARACTERISTICS_MAX: usize = 16;

/// Most simultaneous outstanding reads/writes one
/// [`crate::gatt::session::GattSession`] tracks.
pub const GATT_PENDING_OPERATIONS_MAX: usize = 4;

/// Bounded queue of notifications a [`crate::gatt::session::GattSession`]
/// holds before the caller drains them. Sixteen matches
/// [`EDGE_QUEUE_MAX`]'s reasoning: several notify intervals' worth of
/// headroom before the queue's own overflow-drops-newest-and-counts
/// behavior takes over.
pub const GATT_NOTIFICATION_QUEUE_MAX: usize = 16;

/// [`crate::diagnostic::BuildInfo`]'s fixed wire size (ADR-0011: "Fixed
/// 16-byte schema, protocol, capabilities, build-manifest digest prefix,
/// and reserved fields").
pub const BUILD_INFO_BYTES: usize = 16;

/// [`crate::diagnostic::BuildInfo`]'s build-manifest digest prefix length.
/// Identification, not a security digest (ADR-0011 explicitly omits
/// per-device and credential identity here) -- eight raw bytes distinguish
/// builds with enormous headroom over how many builds this product will
/// ever cut, while leaving [`BUILD_INFO_BYTES`] room for
/// `schema_version`/`protocol_version`/`capabilities` plus reserved bytes.
pub const BUILD_INFO_DIGEST_PREFIX_BYTES: usize = 8;

/// [`crate::diagnostic::LifecycleStatus`]'s fixed wire size (ADR-0011:
/// "Fixed 8-byte schema, state, remaining time, RX drops, and TX
/// timeouts").
pub const LIFECYCLE_STATUS_BYTES: usize = 8;

/// [`crate::diagnostic::Echo`]'s shortest legal payload (ADR-0011: "1-32
/// bytes").
pub const ECHO_PAYLOAD_MIN_BYTES: usize = 1;

/// [`crate::diagnostic::Echo`]'s longest legal payload (ADR-0011: "1-32
/// bytes").
pub const ECHO_PAYLOAD_MAX_BYTES: usize = 32;

/// [`crate::diagnostic::Echo`]'s write-rate ceiling (ADR-0011: "at most
/// four writes/second").
pub const ECHO_WRITES_PER_SECOND_MAX: usize = 4;

/// [`crate::diagnostic::Echo`]'s per-connection write ceiling (ADR-0011:
/// "at most ... 16/connection").
pub const ECHO_WRITES_PER_CONNECTION_MAX: u32 = 16;

/// Most bonded peers [`crate::bond::BondTable`] holds at once (ADR-0016:
/// "a fixed-capacity bond table"). Four covers CheerTok plus headroom to
/// swap in the target gamepad and one or two other test devices without
/// silently evicting a working bond -- inserting past this bound is a
/// rejected `Full` error, never a silent eviction (see [`crate::bond`]).
pub const BOND_TABLE_MAX: usize = 4;

/// [`crate::bond::BondRecord`]'s v2 fixed wire size. In addition to the v1
/// identity/LTK fields, it preserves Trouble 0.8's address kind and legacy
/// pairing EDIV, random number, and negotiated encryption-key length.
pub const BOND_RECORD_BYTES: usize = 64;

/// [`crate::bond::BondTable`]'s whole-table wire size: [`BOND_TABLE_MAX`]
/// fixed-size slots, each either a [`BOND_RECORD_BYTES`]-byte record or an
/// all-zero unused slot (ADR-0016: raw non-volatile read/write is a small
/// per-target port that persists this exact blob, not per-record I/O).
pub const BOND_TABLE_BYTES: usize = BOND_TABLE_MAX * BOND_RECORD_BYTES;
