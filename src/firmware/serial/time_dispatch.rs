//! `TIMESET`/`TIMEGET` serial command handlers.
//!
//! Kept as its own module (rather than folded into `command_dispatch.rs`)
//! because the RTC driver's `Ok`/`Err` shapes and stable reason vocabulary
//! are specific to this one command pair.

use core::fmt::Write;

use super::task_state::SerialTaskState;
use crate::firmware::{
    config::{WALL_CLOCK_REQUESTS, WALL_CLOCK_RESPONSES},
    touch::debug_log::uart_write_all,
    types::{SerialUart, WallClockQueryResult},
};

pub(super) async fn run_timeset_command(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    utc_epoch_seconds: u32,
    offset_minutes: i16,
) {
    let mut line = heapless::String::<64>::new();
    match state
        .rtc_mut()
        .time_set(utc_epoch_seconds, offset_minutes)
        .await
    {
        Ok(outcome) => {
            let _ = write!(
                &mut line,
                "TIMESET OK utc={} offset_min={}\r\n",
                outcome.utc_epoch_seconds, outcome.offset_minutes,
            );
        }
        Err(error) => {
            let _ = write!(&mut line, "TIMESET ERR reason={}\r\n", error.label());
        }
    }
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

/// Answers every pending Ambient Home wall-clock request (from the display
/// task) with one fresh RTC read each. Polled from the serial task's main
/// loop alongside its other cross-task channel drains
/// ([`super::task_state::SerialTaskState::drain_runtime_samples`]); never
/// blocks on there being no request pending.
pub(super) async fn process_wall_clock_requests(state: &mut SerialTaskState) {
    while WALL_CLOCK_REQUESTS.try_receive().is_ok() {
        let result = match state.rtc_mut().read_snapshot().await {
            Ok(snapshot) => WallClockQueryResult::Snapshot(snapshot),
            Err(_) => WallClockQueryResult::I2cError,
        };
        WALL_CLOCK_RESPONSES.send(result).await;
    }
}

pub(super) async fn run_timeget_command(uart: &mut SerialUart, state: &mut SerialTaskState) {
    let mut line = heapless::String::<96>::new();
    match state.rtc_mut().read_snapshot().await {
        Ok(snapshot) if snapshot.valid => {
            let _ = write!(
                &mut line,
                "TIMEGET OK valid=on utc={} local={} offset_min={} os=clear\r\n",
                snapshot.utc_epoch_seconds, snapshot.local_epoch_seconds, snapshot.offset_minutes,
            );
        }
        Ok(snapshot) => {
            let reason = snapshot
                .reason
                .expect("an unavailable snapshot always carries a reason");
            let _ = write!(
                &mut line,
                "TIMEGET OK valid=off reason={}\r\n",
                reason.label()
            );
        }
        Err(error) => {
            let _ = write!(&mut line, "TIMEGET ERR reason={}\r\n", error.label());
        }
    }
    let _ = uart_write_all(uart, line.as_bytes()).await;
}
