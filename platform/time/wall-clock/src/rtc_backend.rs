//! Applies an accepted [`crate::message::SyncReply`] to the shared `rtc`
//! driver and performs the delayed, verified readback the architecture doc
//! calls for: write, wait slightly longer than one second, read again, and
//! require the clock to have actually advanced with an exact offset match.

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use rtc::driver::{Pcf85063a, RtcDiagnosticSink, RtcError, TimeSetOutcome, WallClockSnapshot};

use crate::message::{SyncReply, SyncResult, SyncStatus, VerifyErrorReason};
use crate::session::Coordinator;

/// How long [`apply_and_verify`] waits between the immediate RTC write and
/// the delayed readback that confirms UTC actually advanced -- "slightly
/// longer than one second" per the architecture doc, mirroring hostctl's own
/// (now-retired) `TIME_SYNC_ADVANCE_DELAY_MS`.
pub const VERIFY_DELAY_MS: u32 = 1_100;

/// [`DelayNs`] over `embassy_time::Timer` -- both board runtime adapters
/// need exactly this to drive [`apply_and_verify`]'s delayed readback, so it
/// lives here once rather than as a byte-for-byte copy in each board's own
/// source (both already depend on `embassy-time`).
pub struct EmbassyDelay;

impl DelayNs for EmbassyDelay {
    async fn delay_ns(&mut self, ns: u32) {
        let micros = (ns as u64).div_ceil(1_000).max(1);
        embassy_time::Timer::after(embassy_time::Duration::from_micros(micros)).await;
    }

    async fn delay_ms(&mut self, ms: u32) {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(ms as u64)).await;
    }
}

/// What a session applies its accepted reply against. Generic so both
/// boards' `rtc::driver::Pcf85063a<I2C>` instances satisfy it with no glue
/// code (see the blanket impl below); a fake for host tests satisfies it
/// directly.
#[allow(async_fn_in_trait)]
pub trait RtcBackend {
    type Error;

    async fn time_set(
        &mut self,
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    ) -> Result<TimeSetOutcome, RtcError<Self::Error>>;

    async fn read_snapshot(&mut self) -> Result<WallClockSnapshot, RtcError<Self::Error>>;
}

impl<I2C, D> RtcBackend for Pcf85063a<I2C, D>
where
    I2C: I2c,
    D: RtcDiagnosticSink,
{
    type Error = I2C::Error;

    async fn time_set(
        &mut self,
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    ) -> Result<TimeSetOutcome, RtcError<Self::Error>> {
        Pcf85063a::time_set(self, utc_epoch_seconds, offset_minutes).await
    }

    async fn read_snapshot(&mut self) -> Result<WallClockSnapshot, RtcError<Self::Error>> {
        Pcf85063a::read_snapshot(self).await
    }
}

/// Applies `reply` to `backend` and performs the delayed, verified readback.
/// Must be called only after `coordinator.on_reply` returned
/// `ReplyOutcome::Accepted` for `reply`; closes the coordinator's session
/// with the returned [`SyncResult`] either way -- success or any failure.
pub async fn apply_and_verify<B, D>(
    coordinator: &mut Coordinator,
    backend: &mut B,
    reply: &SyncReply,
    delay: &mut D,
) -> SyncResult
where
    B: RtcBackend,
    D: DelayNs,
{
    if let Err(error) = backend
        .time_set(reply.utc_epoch_seconds, reply.offset_minutes)
        .await
    {
        return coordinator.finish(
            SyncStatus::Err,
            None,
            Some(VerifyErrorReason::Rtc(error.label())),
        );
    }

    coordinator.begin_verify();
    delay.delay_ms(VERIFY_DELAY_MS).await;

    match backend.read_snapshot().await {
        Ok(snapshot) if !snapshot.valid => {
            let reason = snapshot
                .reason
                .map(|reason| VerifyErrorReason::Rtc(reason.label()))
                .unwrap_or(VerifyErrorReason::Rtc("unknown"));
            coordinator.finish(SyncStatus::Err, None, Some(reason))
        }
        Ok(snapshot) if snapshot.utc_epoch_seconds <= reply.utc_epoch_seconds => {
            coordinator.finish(SyncStatus::Err, None, Some(VerifyErrorReason::NotAdvanced))
        }
        Ok(snapshot) if snapshot.offset_minutes != reply.offset_minutes => coordinator.finish(
            SyncStatus::Err,
            None,
            Some(VerifyErrorReason::OffsetMismatch),
        ),
        Ok(snapshot) => coordinator.finish(
            SyncStatus::Ok,
            Some((snapshot.utc_epoch_seconds, snapshot.offset_minutes)),
            None,
        ),
        Err(error) => coordinator.finish(
            SyncStatus::Err,
            None,
            Some(VerifyErrorReason::Rtc(error.label())),
        ),
    }
}
