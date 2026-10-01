//! Shared product-neutral wall-clock policy.
//!
//! Every firmware runtime opens one bounded boot session after one fresh RTC
//! read. A valid snapshot is an optional refresh offer; an unavailable or
//! unreadable RTC requests required provisioning. Opening or timing out a
//! session never mutates the RTC -- [`crate::rtc_backend::apply_and_verify`]
//! is the only write path, and callers may enter it only after an accepted
//! reply.

use rtc::driver::{RtcError, WallClockSnapshot};

use crate::message::{SyncRequest, TriggerReason};
use crate::rtc_backend::RtcBackend;
use crate::session::{Coordinator, SourceSet};

/// Retry cadence for consumers while the RTC is unavailable or unreadable.
/// Available clocks retain their product-specific display cadence.
pub const UNAVAILABLE_RETRY_INTERVAL_MS: u64 = 5_000;

/// The result of opening the one boot synchronization session for a firmware
/// runtime. The initial read is returned so the UI can use a valid RTC
/// immediately (or render time as unavailable) while the bounded session is
/// still awaiting an optional host reply.
pub struct BootSessionStart<E> {
    pub request: SyncRequest,
    pub snapshot: Result<WallClockSnapshot, RtcError<E>>,
}

/// Reads the RTC once and opens the boot session dictated by the shared
/// policy. An I2C read error is treated like an unavailable snapshot for
/// trigger selection, but is preserved in the return value for diagnostics.
pub async fn open_boot_session<B>(
    coordinator: &mut Coordinator,
    backend: &mut B,
    nonce: u32,
    now_ms: u32,
    timeout_ms: u32,
    allowed: SourceSet,
) -> BootSessionStart<B::Error>
where
    B: RtcBackend,
{
    let snapshot = backend.read_snapshot().await;
    let trigger = if matches!(&snapshot, Ok(snapshot) if snapshot.valid) {
        TriggerReason::ColdBootOffer
    } else {
        TriggerReason::InvalidColdBoot
    };
    let request = coordinator.open_session(trigger, nonce, now_ms, timeout_ms, allowed);
    BootSessionStart { request, snapshot }
}
