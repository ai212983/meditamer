//! Requests one fresh RTC read from the serial task for the Ambient Home
//! screen (`docs/plans/ambient-home-prototype.md`). The serial task remains
//! the sole owner of RTC I2C access; this is a bounded, single-in-flight
//! request/response round trip, mirroring `storage::sd_task::power`'s
//! `SD_POWER_REQUESTS`/`SD_POWER_RESPONSES` pattern (just with the requester
//! and responder tasks swapped).

use embassy_time::{with_timeout, Duration};

use crate::firmware::config::{WALL_CLOCK_REQUESTS, WALL_CLOCK_RESPONSES};
use crate::firmware::types::WallClockQueryResult;

const REQUEST_ENQUEUE_TIMEOUT_MS: u64 = 50;
const RESPONSE_TIMEOUT_MS: u64 = 150;

/// Requests one fresh RTC read. Returns `None` on a full request queue, an
/// enqueue/response timeout, or an I2C transaction error -- the Ambient Home
/// screen treats all of these identically as "wall-clock time is currently
/// unavailable" and retries on its own cadence.
pub(in crate::firmware::display) async fn request_wall_clock_snapshot(
) -> Option<rtc::driver::WallClockSnapshot> {
    while WALL_CLOCK_RESPONSES.try_receive().is_ok() {}
    if with_timeout(
        Duration::from_millis(REQUEST_ENQUEUE_TIMEOUT_MS),
        WALL_CLOCK_REQUESTS.send(()),
    )
    .await
    .is_err()
    {
        return None;
    }
    match with_timeout(
        Duration::from_millis(RESPONSE_TIMEOUT_MS),
        WALL_CLOCK_RESPONSES.receive(),
    )
    .await
    {
        Ok(WallClockQueryResult::Snapshot(snapshot)) => Some(snapshot),
        Ok(WallClockQueryResult::I2cError) | Err(_) => None,
    }
}
