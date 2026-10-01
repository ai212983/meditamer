//! ADR-0011's diagnostic v1 GATT wire schema (shared BLE runtime and roles
//! plan, Phase 4 increment 2: "ADR-0011's GATT wire schema... as
//! host-testable encode/decode logic, independent of `trouble-host`'s
//! server macro").
//!
//! [`docs/architecture/0011-bounded-ble-service-foundation.md`] names three
//! unauthenticated characteristics under one primary service, but pins
//! their byte layout only loosely ("fixed N-byte schema" plus a short list
//! of named fields). This module owns the concrete layout: [`BuildInfo`]
//! and [`LifecycleStatus`] are fixed-size, versioned structs with
//! `encode`/`decode`; [`Echo`]'s bound is a payload-length range plus a
//! write-rate/per-connection quota, not a fixed layout, so
//! [`EchoRateLimiter`] tracks admission instead. Everything here is pure,
//! host-testable, and clock-free like the rest of this crate: callers
//! supply `now_ticks` and their own definition of "one second" in whatever
//! tick unit they use, same as [`crate::gatt`]'s deadlines.
//!
//! The product GATT server wires these schemas to advertising and deadlines.
//! See `products/meditamer/src/firmware/ble/runtime.rs` for that wiring and
//! ADR-0011 for service policy. This module owns wire shapes and Echo admission.

use heapless::Deque;

use crate::capacity::{
    BUILD_INFO_BYTES, BUILD_INFO_DIGEST_PREFIX_BYTES, ECHO_PAYLOAD_MAX_BYTES,
    ECHO_PAYLOAD_MIN_BYTES, ECHO_WRITES_PER_CONNECTION_MAX, ECHO_WRITES_PER_SECOND_MAX,
    LIFECYCLE_STATUS_BYTES,
};

/// The Diagnostic Service and its three characteristics' 128-bit UUIDs
/// (arbitrarily generated, since ADR-0011 requires but does not name one,
/// and fixed once a real client exists).
/// Byte arrays are little-endian wire order (reversed from the UUID
/// string's left-to-right reading, per the Bluetooth Core spec's 128-bit
/// UUID encoding), matching what `AdStructure::CompleteServiceUuids128` and
/// `trouble_host::Uuid::new_long` both expect. Shared here, not duplicated
/// between the server (`products/meditamer`, which builds the
/// `#[gatt_server]`) and any GATT client testing against it (this crate's
/// own `diagnostic_client`, increment 6's fake client), so the two cannot
/// silently drift apart.
///
/// | Characteristic | UUID string |
/// | --- | --- |
/// | Diagnostic Service | `BE836D30-5EA2-4E16-B476-4D32E4F7CBCE` |
/// | Build Info | `BE836D30-5EA2-4E16-B476-4D32E4F7CBCF` |
/// | Echo | `BE836D30-5EA2-4E16-B476-4D32E4F7CBD0` |
/// | Lifecycle Status | `BE836D30-5EA2-4E16-B476-4D32E4F7CBD1` |
pub const SERVICE_UUID: [u8; 16] = [
    0xce, 0xcb, 0xf7, 0xe4, 0x32, 0x4d, 0x76, 0xb4, 0x16, 0x4e, 0xa2, 0x5e, 0x30, 0x6d, 0x83, 0xbe,
];
pub const BUILD_INFO_UUID: [u8; 16] = [
    0xcf, 0xcb, 0xf7, 0xe4, 0x32, 0x4d, 0x76, 0xb4, 0x16, 0x4e, 0xa2, 0x5e, 0x30, 0x6d, 0x83, 0xbe,
];
pub const ECHO_UUID: [u8; 16] = [
    0xd0, 0xcb, 0xf7, 0xe4, 0x32, 0x4d, 0x76, 0xb4, 0x16, 0x4e, 0xa2, 0x5e, 0x30, 0x6d, 0x83, 0xbe,
];
pub const LIFECYCLE_STATUS_UUID: [u8; 16] = [
    0xd1, 0xcb, 0xf7, 0xe4, 0x32, 0x4d, 0x76, 0xb4, 0x16, 0x4e, 0xa2, 0x5e, 0x30, 0x6d, 0x83, 0xbe,
];

/// A fixed-size wire value did not decode: wrong length, or a byte that
/// does not name a legal enumerated value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    WrongLength,
    UnknownState,
}

/// Build Info's `capabilities` byte. Bits beyond the ones named here are
/// reserved: always zero in a v1 build, decoded and preserved rather than
/// rejected, so a future build can define them without breaking an older
/// client's decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(pub u8);

impl Capabilities {
    pub const ECHO: Capabilities = Capabilities(0b0000_0001);
    pub const LIFECYCLE_STATUS: Capabilities = Capabilities(0b0000_0010);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Build Info characteristic value (ADR-0011: "Fixed 16-byte schema,
/// protocol, capabilities, build-manifest digest prefix, and reserved
/// fields"; "identifies a firmware build through the manifest digest
/// prefix. Its fixed exposure omits timestamps, dirty markers, local
/// paths, serials, per-device values, and credential identity").
///
/// Wire layout (16 bytes, all reserved bytes zero):
///
/// | Offset | Bytes | Field |
/// | --- | --- | --- |
/// | 0 | 1 | `schema_version` |
/// | 1 | 1 | `protocol_version` |
/// | 2 | 1 | `capabilities` |
/// | 3 | 1 | reserved |
/// | 4 | 8 | `digest_prefix` |
/// | 12 | 4 | reserved |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    pub schema_version: u8,
    pub protocol_version: u8,
    pub capabilities: Capabilities,
    pub digest_prefix: [u8; BUILD_INFO_DIGEST_PREFIX_BYTES],
}

impl BuildInfo {
    pub fn encode(&self) -> [u8; BUILD_INFO_BYTES] {
        let mut out = [0u8; BUILD_INFO_BYTES];
        out[0] = self.schema_version;
        out[1] = self.protocol_version;
        out[2] = self.capabilities.0;
        // out[3] and out[12..16] stay reserved/zero.
        out[4..4 + BUILD_INFO_DIGEST_PREFIX_BYTES].copy_from_slice(&self.digest_prefix);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() != BUILD_INFO_BYTES {
            return Err(DecodeError::WrongLength);
        }
        let mut digest_prefix = [0u8; BUILD_INFO_DIGEST_PREFIX_BYTES];
        digest_prefix.copy_from_slice(&bytes[4..4 + BUILD_INFO_DIGEST_PREFIX_BYTES]);
        Ok(Self {
            schema_version: bytes[0],
            protocol_version: bytes[1],
            capabilities: Capabilities(bytes[2]),
            digest_prefix,
        })
    }
}

/// Lifecycle Status's `state` byte -- the diagnostic window's
/// client-visible phase, distinct from the coordinator's own
/// `Serving`/`Quiescing`/... ownership states (ADR-0011's "Ownership and
/// lifecycle" table) and from the existing probe's `ProbeState` (which
/// tracks the probe's own diagnostic-window bookkeeping, not what a
/// connected client should be told).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    /// Advertising, not yet connected (ADR-0011's 60 s advertising
    /// deadline).
    Advertising,
    /// Connected, with GATT traffic within the idle timeout.
    Connected,
    /// Connected, past the idle timeout, pending forced disconnect
    /// (ADR-0011's 30 s idle timeout within the 60 s connection deadline).
    ConnectedIdle,
    /// The visibility window is closing: advertising has stopped and any
    /// connection is being torn down.
    Closing,
}

impl LifecycleState {
    const fn to_byte(self) -> u8 {
        match self {
            Self::Advertising => 0,
            Self::Connected => 1,
            Self::ConnectedIdle => 2,
            Self::Closing => 3,
        }
    }

    const fn from_byte(byte: u8) -> Result<Self, DecodeError> {
        match byte {
            0 => Ok(Self::Advertising),
            1 => Ok(Self::Connected),
            2 => Ok(Self::ConnectedIdle),
            3 => Ok(Self::Closing),
            _ => Err(DecodeError::UnknownState),
        }
    }
}

/// Lifecycle Status characteristic value (ADR-0011: "Fixed 8-byte schema,
/// state, remaining time, RX drops, and TX timeouts").
///
/// Wire layout (8 bytes, little-endian multi-byte fields, reserved byte
/// zero):
///
/// | Offset | Bytes | Field |
/// | --- | --- | --- |
/// | 0 | 1 | `state` |
/// | 1 | 2 | `remaining_seconds` |
/// | 3 | 2 | `rx_drops` |
/// | 5 | 2 | `tx_timeouts` |
/// | 7 | 1 | reserved |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleStatus {
    pub state: LifecycleState,
    pub remaining_seconds: u16,
    pub rx_drops: u16,
    pub tx_timeouts: u16,
}

impl LifecycleStatus {
    pub fn encode(&self) -> [u8; LIFECYCLE_STATUS_BYTES] {
        let mut out = [0u8; LIFECYCLE_STATUS_BYTES];
        out[0] = self.state.to_byte();
        out[1..3].copy_from_slice(&self.remaining_seconds.to_le_bytes());
        out[3..5].copy_from_slice(&self.rx_drops.to_le_bytes());
        out[5..7].copy_from_slice(&self.tx_timeouts.to_le_bytes());
        // out[7] stays reserved/zero.
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() != LIFECYCLE_STATUS_BYTES {
            return Err(DecodeError::WrongLength);
        }
        Ok(Self {
            state: LifecycleState::from_byte(bytes[0])?,
            remaining_seconds: u16::from_le_bytes([bytes[1], bytes[2]]),
            rx_drops: u16::from_le_bytes([bytes[3], bytes[4]]),
            tx_timeouts: u16::from_le_bytes([bytes[5], bytes[6]]),
        })
    }
}

/// Coalesces Lifecycle Status updates into one pending notification (ADR-
///0011: "one coalesced pending notification"), same latest-plus-pending
/// shape as [`crate::input::InputPublisher`]: a value that changes faster
/// than the link can notify must never queue, it must replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleStatusPublisher {
    latest: LifecycleStatus,
    pending: bool,
}

impl LifecycleStatusPublisher {
    pub const fn new(initial: LifecycleStatus) -> Self {
        Self {
            latest: initial,
            pending: false,
        }
    }

    /// The current value, whether or not it has been notified yet.
    pub const fn latest(&self) -> LifecycleStatus {
        self.latest
    }

    /// Records a new value and marks a notification pending, replacing any
    /// value that was already pending and unsent.
    pub fn publish(&mut self, status: LifecycleStatus) {
        self.latest = status;
        self.pending = true;
    }

    /// Takes the pending notification, if any, clearing the pending flag.
    /// A second call before the next [`Self::publish`] returns `None`.
    pub fn take_pending_notification(&mut self) -> Option<LifecycleStatus> {
        if self.pending {
            self.pending = false;
            Some(self.latest)
        } else {
            None
        }
    }
}

/// Why [`EchoRateLimiter::try_admit_write`] refused a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EchoWriteRejection {
    /// Outside ADR-0011's 1-32 byte payload range.
    PayloadOutOfRange,
    /// Would exceed four writes/second.
    RateLimited,
    /// Would exceed sixteen writes for this connection.
    ConnectionQuotaExhausted,
}

/// Admits or refuses Echo writes against ADR-0011's payload-length and
/// rate/quota bounds ("1-32 bytes; ... at most four writes/second and
/// 16/connection"). Payload bytes themselves are not this type's concern
/// -- it only says whether one more write is currently allowed.
///
/// Clock-free like the rest of this crate: the caller supplies `now_ticks`
/// and `one_second_ticks` (its own tick unit's definition of one second),
/// so this stays deterministic under `cargo test` without a real timer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EchoRateLimiter {
    recent_write_ticks: Deque<u64, ECHO_WRITES_PER_SECOND_MAX>,
    connection_writes: u32,
}

impl EchoRateLimiter {
    pub const fn new() -> Self {
        Self {
            recent_write_ticks: Deque::new(),
            connection_writes: 0,
        }
    }

    /// Clears rate and quota state for a new connection. The `Echo`
    /// characteristic's own value is not this type's state.
    pub fn reset(&mut self) {
        self.recent_write_ticks.clear();
        self.connection_writes = 0;
    }

    /// Total writes admitted since the last [`Self::reset`].
    pub fn connection_writes(&self) -> u32 {
        self.connection_writes
    }

    /// Admits one write of `payload_len` bytes at `now_ticks`, or refuses
    /// it and leaves all state unchanged (a refused write consumes neither
    /// the rate window nor the connection quota).
    pub fn try_admit_write(
        &mut self,
        payload_len: usize,
        now_ticks: u64,
        one_second_ticks: u64,
    ) -> Result<(), EchoWriteRejection> {
        if !(ECHO_PAYLOAD_MIN_BYTES..=ECHO_PAYLOAD_MAX_BYTES).contains(&payload_len) {
            return Err(EchoWriteRejection::PayloadOutOfRange);
        }
        if self.connection_writes >= ECHO_WRITES_PER_CONNECTION_MAX {
            return Err(EchoWriteRejection::ConnectionQuotaExhausted);
        }
        while let Some(oldest) = self.recent_write_ticks.front() {
            if now_ticks.saturating_sub(*oldest) >= one_second_ticks {
                self.recent_write_ticks.pop_front();
            } else {
                break;
            }
        }
        if self.recent_write_ticks.len() >= ECHO_WRITES_PER_SECOND_MAX {
            return Err(EchoWriteRejection::RateLimited);
        }
        // Capacity was just proven above; the deque cannot be full here.
        let _ = self.recent_write_ticks.push_back(now_ticks);
        self.connection_writes += 1;
        Ok(())
    }
}

impl Default for EchoRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_round_trips() {
        let info = BuildInfo {
            schema_version: 1,
            protocol_version: 1,
            capabilities: Capabilities::ECHO.union(Capabilities::LIFECYCLE_STATUS),
            digest_prefix: [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04],
        };
        let encoded = info.encode();
        assert_eq!(encoded.len(), BUILD_INFO_BYTES);
        assert_eq!(BuildInfo::decode(&encoded), Ok(info));
    }

    #[test]
    fn build_info_reserved_bytes_are_zero() {
        let info = BuildInfo {
            schema_version: 0xff,
            protocol_version: 0xff,
            capabilities: Capabilities(0xff),
            digest_prefix: [0xff; BUILD_INFO_DIGEST_PREFIX_BYTES],
        };
        let encoded = info.encode();
        assert_eq!(encoded[3], 0);
        assert_eq!(&encoded[12..16], &[0, 0, 0, 0]);
    }

    #[test]
    fn build_info_wrong_length_is_rejected() {
        assert_eq!(BuildInfo::decode(&[0u8; 15]), Err(DecodeError::WrongLength));
        assert_eq!(BuildInfo::decode(&[0u8; 17]), Err(DecodeError::WrongLength));
    }

    #[test]
    fn capabilities_contains_checks_all_named_bits() {
        let both = Capabilities::ECHO.union(Capabilities::LIFECYCLE_STATUS);
        assert!(both.contains(Capabilities::ECHO));
        assert!(both.contains(Capabilities::LIFECYCLE_STATUS));
        assert!(!Capabilities::ECHO.contains(Capabilities::LIFECYCLE_STATUS));
    }

    #[test]
    fn lifecycle_status_round_trips_every_state() {
        for state in [
            LifecycleState::Advertising,
            LifecycleState::Connected,
            LifecycleState::ConnectedIdle,
            LifecycleState::Closing,
        ] {
            let status = LifecycleStatus {
                state,
                remaining_seconds: 42,
                rx_drops: 7,
                tx_timeouts: 3,
            };
            assert_eq!(LifecycleStatus::decode(&status.encode()), Ok(status));
        }
    }

    #[test]
    fn lifecycle_status_wrong_length_is_rejected() {
        assert_eq!(
            LifecycleStatus::decode(&[0u8; 7]),
            Err(DecodeError::WrongLength)
        );
        assert_eq!(
            LifecycleStatus::decode(&[0u8; 9]),
            Err(DecodeError::WrongLength)
        );
    }

    #[test]
    fn lifecycle_status_unknown_state_is_rejected() {
        let mut bytes = LifecycleStatus {
            state: LifecycleState::Advertising,
            remaining_seconds: 0,
            rx_drops: 0,
            tx_timeouts: 0,
        }
        .encode();
        bytes[0] = 99;
        assert_eq!(
            LifecycleStatus::decode(&bytes),
            Err(DecodeError::UnknownState)
        );
    }

    #[test]
    fn lifecycle_status_reserved_byte_is_zero() {
        let status = LifecycleStatus {
            state: LifecycleState::Connected,
            remaining_seconds: 0xffff,
            rx_drops: 0xffff,
            tx_timeouts: 0xffff,
        };
        assert_eq!(status.encode()[7], 0);
    }

    #[test]
    fn lifecycle_status_publisher_coalesces_repeated_publishes() {
        let mut publisher = LifecycleStatusPublisher::new(LifecycleStatus {
            state: LifecycleState::Advertising,
            remaining_seconds: 60,
            rx_drops: 0,
            tx_timeouts: 0,
        });
        assert_eq!(publisher.take_pending_notification(), None);

        publisher.publish(LifecycleStatus {
            state: LifecycleState::Connected,
            remaining_seconds: 60,
            rx_drops: 0,
            tx_timeouts: 0,
        });
        publisher.publish(LifecycleStatus {
            state: LifecycleState::ConnectedIdle,
            remaining_seconds: 30,
            rx_drops: 1,
            tx_timeouts: 0,
        });

        let notified = publisher.take_pending_notification().expect("pending");
        assert_eq!(notified.state, LifecycleState::ConnectedIdle);
        assert_eq!(publisher.take_pending_notification(), None);
        assert_eq!(publisher.latest().state, LifecycleState::ConnectedIdle);
    }

    #[test]
    fn echo_rejects_payload_outside_the_legal_range() {
        let mut limiter = EchoRateLimiter::new();
        assert_eq!(
            limiter.try_admit_write(0, 0, 1_000),
            Err(EchoWriteRejection::PayloadOutOfRange)
        );
        assert_eq!(
            limiter.try_admit_write(33, 0, 1_000),
            Err(EchoWriteRejection::PayloadOutOfRange)
        );
        assert_eq!(limiter.connection_writes(), 0);
    }

    #[test]
    fn echo_admits_up_to_four_writes_per_second_then_rate_limits() {
        let mut limiter = EchoRateLimiter::new();
        for tick in 0..4 {
            assert_eq!(limiter.try_admit_write(4, tick, 1_000), Ok(()));
        }
        assert_eq!(
            limiter.try_admit_write(4, 4, 1_000),
            Err(EchoWriteRejection::RateLimited)
        );
    }

    #[test]
    fn echo_rate_window_slides_forward() {
        let mut limiter = EchoRateLimiter::new();
        for tick in 0..4 {
            assert_eq!(limiter.try_admit_write(4, tick, 1_000), Ok(()));
        }
        assert_eq!(
            limiter.try_admit_write(4, 999, 1_000),
            Err(EchoWriteRejection::RateLimited)
        );
        // The tick=0 write is now outside the one-second window.
        assert_eq!(limiter.try_admit_write(4, 1_000, 1_000), Ok(()));
    }

    #[test]
    fn echo_enforces_the_per_connection_quota_across_many_seconds() {
        let mut limiter = EchoRateLimiter::new();
        let mut tick: u64 = 0;
        for _ in 0..ECHO_WRITES_PER_CONNECTION_MAX {
            assert_eq!(limiter.try_admit_write(4, tick, 1_000), Ok(()));
            tick += 1_000;
        }
        assert_eq!(
            limiter.try_admit_write(4, tick, 1_000),
            Err(EchoWriteRejection::ConnectionQuotaExhausted)
        );
        assert_eq!(limiter.connection_writes(), ECHO_WRITES_PER_CONNECTION_MAX);
    }

    #[test]
    fn echo_reset_clears_rate_and_quota_state() {
        let mut limiter = EchoRateLimiter::new();
        for tick in 0..4 {
            assert_eq!(limiter.try_admit_write(4, tick, 1_000), Ok(()));
        }
        limiter.reset();
        assert_eq!(limiter.connection_writes(), 0);
        assert_eq!(limiter.try_admit_write(4, 0, 1_000), Ok(()));
    }

    #[test]
    fn echo_rejected_write_does_not_consume_quota_or_window() {
        let mut limiter = EchoRateLimiter::new();
        assert_eq!(
            limiter.try_admit_write(0, 0, 1_000),
            Err(EchoWriteRejection::PayloadOutOfRange)
        );
        assert_eq!(limiter.connection_writes(), 0);
        for tick in 0..4 {
            assert_eq!(limiter.try_admit_write(4, tick, 1_000), Ok(()));
        }
        assert_eq!(limiter.connection_writes(), 4);
    }
}
