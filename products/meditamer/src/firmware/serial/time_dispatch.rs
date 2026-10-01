//! Wall-clock serial handling: the session-protocol commands
//! (`TIME_REPLY`/`TIMESYNC`) plus the always-available `TIMEGET` read.
//! `docs/references/runtime/serial-control.md#time-synchronization` owns the contract; this module is
//! Inkplate's runtime adapter (INK-01) -- it owns triggers (boot, manual
//! demand), timeout polling, and RTC application/verification, all via the
//! shared `wall_clock` coordinator. The retired `TIMESET` used to expose
//! unconditional RTC mutation directly on the wire; `TIME_REPLY` now mutates
//! the RTC only when `wall_clock::session::Coordinator::on_reply` accepts it
//! as an open session's first valid reply.

use core::fmt::Write;

use embassy_time::Instant;

use super::task_state::SerialTaskState;
use crate::firmware::{
    config::{WALL_CLOCK_REQUESTS, WALL_CLOCK_RESPONSES},
    touch::debug_log::uart_write_all,
    types::{SerialWriter, WallClockQueryResult},
};

/// The only transport this task ever admits wall-clock replies from.
const SOURCE: wall_clock::session::SourceId = wall_clock::session::SourceId(0);

/// Transport deadlines. The boot deadline is generous enough to still be
/// open when hostctl's flash-capture `time_sync` action
/// reconnects after the boot-capture console closes and settles
/// (`tools/hostctl/src/workflows/flash_capture/capture.rs`); Inkplate's UART
/// task is permanent, so waiting costs nothing but session lifetime.
const BOOT_SYNC_DEADLINE_MS: u32 = 15_000;
const MANUAL_DEMAND_DEADLINE_MS: u32 = 8_000;

fn now_ms() -> u32 {
    Instant::now().as_millis() as u32
}

async fn send_request(uart: &mut SerialWriter, request: &wall_clock::message::SyncRequest) {
    let mut line = heapless::String::<96>::new();
    if wall_clock::codec::encode_request(request, &mut line).is_ok()
        && line.push_str("\r\n").is_ok()
    {
        let _ = uart_write_all(uart, line.as_bytes()).await;
    }
}

async fn send_result(uart: &mut SerialWriter, result: &wall_clock::message::SyncResult) {
    let mut line = heapless::String::<64>::new();
    if wall_clock::codec::encode_result(result, &mut line).is_ok() && line.push_str("\r\n").is_ok()
    {
        let _ = uart_write_all(uart, line.as_bytes()).await;
    }
}

/// Called once at task startup: opens the cold-boot session appropriate to
/// whatever the RTC currently holds (invalid -> required; valid -> offered)
/// and emits its `TIME_REQUEST`.
pub(super) async fn open_boot_session(uart: &mut SerialWriter, state: &mut SerialTaskState) {
    let nonce = state.next_wall_clock_nonce();
    let (rtc, coordinator) = state.rtc_and_wall_clock_mut();
    let started = wall_clock::policy::open_boot_session(
        coordinator,
        rtc,
        nonce,
        now_ms(),
        BOOT_SYNC_DEADLINE_MS,
        wall_clock::session::SourceSet::single(SOURCE),
    )
    .await;
    match &started.snapshot {
        Ok(snapshot) if snapshot.valid => {
            console::println!("RTC_BOOT valid=on");
        }
        Ok(snapshot) => {
            let reason = snapshot
                .reason
                .expect("an unavailable snapshot always carries a reason");
            console::println!("RTC_BOOT valid=off reason={}", reason.label());
        }
        Err(error) => console::println!("RTC_BOOT error={}", error.label()),
    }
    send_request(uart, &started.request).await;
}

/// Polled every loop iteration alongside the task's other cross-task channel
/// drains; closes and reports a session that timed out waiting for a reply.
pub(super) async fn poll_wall_clock_session(uart: &mut SerialWriter, state: &mut SerialTaskState) {
    if let Some(result) = state.wall_clock_mut().poll_timeout(now_ms()) {
        send_result(uart, &result).await;
    }
}

pub(super) async fn run_timesync_command(uart: &mut SerialWriter, state: &mut SerialTaskState) {
    let nonce = state.next_wall_clock_nonce();
    let request = state.wall_clock_mut().open_session(
        wall_clock::message::TriggerReason::ManualDemand,
        nonce,
        now_ms(),
        MANUAL_DEMAND_DEADLINE_MS,
        wall_clock::session::SourceSet::single(SOURCE),
    );
    send_request(uart, &request).await;
}

pub(super) async fn run_time_reply_command(
    uart: &mut SerialWriter,
    state: &mut SerialTaskState,
    reply: wall_clock::message::SyncReply,
) {
    let accepted = matches!(
        state.wall_clock_mut().on_reply(SOURCE, &reply, now_ms()),
        wall_clock::session::ReplyOutcome::Accepted
    );
    if !accepted {
        // Late, duplicate, mismatched, or wrong-state -- ignored per the
        // architecture doc; there is no session for this reply to report a
        // result against.
        return;
    }

    let mut delay = wall_clock::rtc_backend::EmbassyDelay;
    let (rtc, coordinator) = state.rtc_and_wall_clock_mut();
    let result =
        wall_clock::rtc_backend::apply_and_verify(coordinator, rtc, &reply, &mut delay).await;
    send_result(uart, &result).await;
}

/// Answers every pending Ambient Home wall-clock request (from the display
/// task) with one fresh RTC read each. Polled from the serial task's main
/// loop alongside its other cross-task channel drains
/// ([`super::task_state::SerialTaskState::drain_runtime_samples`]); never
/// blocks on there being no request pending.
pub(super) async fn process_wall_clock_requests(state: &mut SerialTaskState) {
    if WALL_CLOCK_REQUESTS.try_receive().is_ok() {
        let result = match state.rtc_mut().read_snapshot().await {
            Ok(snapshot) => WallClockQueryResult::Snapshot(snapshot),
            Err(_) => WallClockQueryResult::I2cError,
        };
        // A requester may have timed out. A full reply queue must not stop
        // command RX; the next requester drains old replies before submitting.
        if WALL_CLOCK_RESPONSES.try_send(result).is_err() {
            console::println!("WALL_CLOCK_QUERY reply=discarded reason=queue_full");
        }
    }
}

pub(super) async fn run_timeget_command(uart: &mut SerialWriter, state: &mut SerialTaskState) {
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
