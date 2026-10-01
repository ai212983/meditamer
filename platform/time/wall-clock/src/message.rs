//! Transport-neutral wall-clock synchronization messages: [`SyncRequest`],
//! [`SyncReply`], and [`SyncResult`]. Carried over serial via
//! [`crate::codec`]; the message shapes themselves know nothing about any
//! transport.

/// Wire/message schema version. Bumped only on a breaking change to a
/// message's fields or its [`crate::codec`] encoding; a receiver on an older
/// version is free to reject or ignore a mismatch, though nothing in this
/// crate enforces that policy itself -- there is only one version so far.
pub const PROTOCOL_VERSION: u8 = 1;

/// Why a session was opened, matching `TIME_REQUEST reason=<..>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerReason {
    /// The RTC has no usable time at all. A timeout leaves time unavailable.
    InvalidColdBoot,
    /// The RTC already holds valid time; synchronization is offered but not
    /// required. A timeout preserves the existing valid clock.
    ColdBootOffer,
    /// A product UI, button, or service action asked for a fresh sync.
    ManualDemand,
}

impl TriggerReason {
    /// Stable snake_case reason string for the serial wire protocol.
    pub const fn label(&self) -> &'static str {
        match self {
            TriggerReason::InvalidColdBoot => "invalid_cold_boot",
            TriggerReason::ColdBootOffer => "cold_boot_offer",
            TriggerReason::ManualDemand => "manual_demand",
        }
    }

    /// Parses [`Self::label`]'s output back into a `TriggerReason`. `None`
    /// for anything else.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "invalid_cold_boot" => Some(TriggerReason::InvalidColdBoot),
            "cold_boot_offer" => Some(TriggerReason::ColdBootOffer),
            "manual_demand" => Some(TriggerReason::ManualDemand),
            _ => None,
        }
    }
}

/// Why [`crate::rtc_backend::apply_and_verify`] (or a session timeout)
/// reports failure, matching `TIME_SYNC ERR reason=<..>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyErrorReason {
    /// The RTC driver itself failed applying or reading back the reply --
    /// carries one of `rtc::driver::RtcError`'s or `UnavailableReason`'s
    /// stable labels verbatim (e.g. `"i2c"`, `"verify"`, `"clock_stopped"`).
    Rtc(&'static str),
    /// `time_set` and the delayed readback both succeeded, but UTC did not
    /// advance past the reply's value.
    NotAdvanced,
    /// The delayed readback's offset does not match the reply's offset.
    OffsetMismatch,
    /// The session closed (or a stray reply arrived) without ever accepting
    /// a reply within its deadline.
    Timeout,
}

impl VerifyErrorReason {
    /// Stable snake_case reason string for the serial wire protocol.
    pub const fn label(&self) -> &'static str {
        match self {
            VerifyErrorReason::Rtc(label) => label,
            VerifyErrorReason::NotAdvanced => "not_advanced",
            VerifyErrorReason::OffsetMismatch => "offset_mismatch",
            VerifyErrorReason::Timeout => "timeout",
        }
    }

    /// Parses a `TIME_SYNC ERR reason=<..>` token back into a
    /// `VerifyErrorReason`. Every known `rtc` driver label round-trips
    /// through [`Self::Rtc`]; anything unrecognized still decodes (as
    /// `Rtc("unknown")`) rather than failing the whole message, since the
    /// reason is diagnostic, not load-bearing.
    pub fn from_label(label: &str) -> Self {
        match label {
            "not_advanced" => VerifyErrorReason::NotAdvanced,
            "offset_mismatch" => VerifyErrorReason::OffsetMismatch,
            "timeout" => VerifyErrorReason::Timeout,
            "range" => VerifyErrorReason::Rtc("range"),
            "offset" => VerifyErrorReason::Rtc("offset"),
            "i2c" => VerifyErrorReason::Rtc("i2c"),
            "verify" => VerifyErrorReason::Rtc("verify"),
            "clock_stopped" => VerifyErrorReason::Rtc("clock_stopped"),
            "oscillator_stopped" => VerifyErrorReason::Rtc("oscillator_stopped"),
            "offset_unset" => VerifyErrorReason::Rtc("offset_unset"),
            "invalid_calendar" => VerifyErrorReason::Rtc("invalid_calendar"),
            _ => VerifyErrorReason::Rtc("unknown"),
        }
    }
}

/// A device-initiated request for the current UTC and fixed offset. Opens
/// one bounded session; `deadline_ms` is the request's own timeout budget
/// (informational for the receiver -- the device enforces it independently
/// via [`crate::session::Coordinator::poll_timeout`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncRequest {
    pub version: u8,
    pub session: u32,
    pub nonce: u32,
    pub trigger: TriggerReason,
    pub deadline_ms: u32,
}

/// A host's answer to one [`SyncRequest`]: the sampled UTC and fixed local
/// offset, echoing the session and nonce it is answering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncReply {
    pub version: u8,
    pub session: u32,
    pub nonce: u32,
    pub utc_epoch_seconds: u32,
    pub offset_minutes: i16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncStatus {
    Ok,
    Err,
}

/// The device's final report for one session: either the verified
/// `(utc_epoch_seconds, offset_minutes)` the RTC now holds, or a stable
/// failure reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncResult {
    pub version: u8,
    pub session: u32,
    pub status: SyncStatus,
    /// Set only when `status` is `Ok`.
    pub verified: Option<(u32, i16)>,
    /// Set only when `status` is `Err`.
    pub reason: Option<VerifyErrorReason>,
}
