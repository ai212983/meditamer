//! Wall-clock synchronization for this target's runtime Home surface. Every
//! firmware runtime opens exactly one bounded boot session after a fresh RTC
//! read. A valid clock is immediately usable while the optional offer is
//! open; an invalid or unreadable clock remains unavailable until a reply is
//! accepted. Later Home refreshes only read the battery-backed RTC.
//!
//! [`start`]/[`ClockSession::poll`] never block waiting on a host reply --
//! each call only drains whatever bytes are already sitting in the RX FIFO
//! and returns immediately either way. The one boot-sync session that ever
//! runs is serviced by the UI loop's own cadence instead of a dedicated task
//! or channel: a host that never answers does not stall hourglass physics,
//! rendering, or button sampling for the whole `BOOT_SYNC_DEADLINE_MS`
//! window, since the loop keeps ticking between polls. While a session is
//! live it is the RX FIFO's sole reader --
//! see [`ClockSession`] and `observations::poll_observations`'s
//! `jtag_owned_elsewhere`.

use embassy_time::Instant;
use rtc::driver::{Pcf85063a, WallClockSnapshot};
use wall_clock::message::SyncReply;
use wall_clock::rtc_backend::EmbassyDelay;
use wall_clock::session::{Coordinator, ReplyOutcome, SourceId, SourceSet};

use waveshare_rlcd42::jtag_rx;

use crate::SharedI2c;

const SOURCE: SourceId = SourceId(0);

/// Inherited from this board's prior 3s transport timeout plus margin, not a
/// fresh hardware measurement. Since `poll` is nonblocking, this is the
/// wall-clock time the one boot session can remain open across short UI-loop
/// iterations rather than one long `.await`.
const BOOT_SYNC_DEADLINE_MS: u32 = 5_000;

/// One reply line's worth of buffering. Matches the fixed-size buffer the
/// previous blocking `read_line` used.
const REPLY_LINE_CAPACITY: usize = 128;

fn now_ms() -> u32 {
    Instant::now().as_millis() as u32
}

/// Reads the RTC and opens this runtime's one boot session. The initial
/// snapshot is returned separately so Home can render a valid clock (or
/// `--:--`) immediately without waiting for the optional host round trip.
pub async fn start(clock: &mut Pcf85063a<SharedI2c>) -> (ClockSession, Option<WallClockSnapshot>) {
    let mut coordinator = Coordinator::new();
    // Not a security boundary -- the only "attacker" is a stale reply on a
    // trusted USB link -- just needs to be nonzero and change between
    // sessions, which the free-running embassy tick count already gives.
    let nonce = (Instant::now().as_ticks() as u32) | 1;
    let started = wall_clock::policy::open_boot_session(
        &mut coordinator,
        clock,
        nonce,
        now_ms(),
        BOOT_SYNC_DEADLINE_MS,
        SourceSet::single(SOURCE),
    )
    .await;
    match &started.snapshot {
        Ok(snapshot) if snapshot.valid => console::println!("RTC_BOOT valid=on"),
        Ok(snapshot) => {
            let reason = snapshot
                .reason
                .expect("an unavailable snapshot always carries a reason");
            console::println!("RTC_BOOT valid=off reason={}", reason.label());
        }
        Err(error) => console::println!("RTC_BOOT error={} detail={:?}", error.label(), error),
    }
    send_request(&started.request);

    let initial_snapshot = started.snapshot.ok();
    (
        ClockSession {
            coordinator,
            line: heapless::Vec::new(),
            discarding_line: false,
        },
        initial_snapshot,
    )
}

/// A boot-sync session in progress. Holds no reference to the UI loop's own
/// state -- the caller keeps this (typically in an `Option`) across
/// iterations and calls [`Self::poll`] once per tick until it resolves.
pub struct ClockSession {
    coordinator: Coordinator,
    line: heapless::Vec<u8, REPLY_LINE_CAPACITY>,
    discarding_line: bool,
}

impl ClockSession {
    /// Drains whatever bytes are already waiting in the RX FIFO without
    /// awaiting new ones, and checks the session deadline. Returns `Err`
    /// (the still-open session) until an accepted reply has been applied or
    /// the deadline passes. Malformed, unrelated, stale, or oversized lines
    /// are ignored just as they are by Meditamer's permanent command parser.
    pub async fn poll(
        mut self,
        clock: &mut Pcf85063a<SharedI2c>,
    ) -> Result<Option<WallClockSnapshot>, Self> {
        while let Some(byte) = jtag_rx::try_read_byte() {
            if self.discarding_line {
                if byte == b'\n' || byte == b'\r' {
                    self.discarding_line = false;
                }
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                if self.line.is_empty() {
                    // A lone CR or LF before any content -- likely the line
                    // ending of whatever the host's terminal sent before the
                    // real reply. Keep waiting rather than treat it as empty.
                    continue;
                }
                let reply = core::str::from_utf8(&self.line)
                    .ok()
                    .and_then(wall_clock::codec::decode_reply);
                self.line.clear();
                let Some(reply) = reply else {
                    continue;
                };
                match self.coordinator.on_reply(SOURCE, &reply, now_ms()) {
                    ReplyOutcome::Accepted => return Ok(self.apply(clock, &reply).await),
                    ReplyOutcome::Ignored(_) if self.coordinator.is_closed() => {
                        return Ok(read_snapshot(clock).await);
                    }
                    ReplyOutcome::Ignored(_) => continue,
                }
            }
            if self.line.push(byte).is_err() {
                self.line.clear();
                self.discarding_line = true;
            }
        }
        if let Some(result) = self.coordinator.poll_timeout(now_ms()) {
            send_result(&result);
            return Ok(read_snapshot(clock).await);
        }
        Err(self)
    }

    async fn apply(
        &mut self,
        clock: &mut Pcf85063a<SharedI2c>,
        reply: &SyncReply,
    ) -> Option<WallClockSnapshot> {
        let mut delay = EmbassyDelay;
        let result = wall_clock::rtc_backend::apply_and_verify(
            &mut self.coordinator,
            clock,
            reply,
            &mut delay,
        )
        .await;
        send_result(&result);

        read_snapshot(clock).await
    }
}

async fn read_snapshot(clock: &mut Pcf85063a<SharedI2c>) -> Option<WallClockSnapshot> {
    match clock.read_snapshot().await {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            console::println!("RTC_READ error={}", error.label());
            None
        }
    }
}

fn send_request(request: &wall_clock::message::SyncRequest) {
    let mut line = heapless::String::<96>::new();
    if wall_clock::codec::encode_request(request, &mut line).is_ok() {
        console::println!("{}", line.as_str());
    }
}

fn send_result(result: &wall_clock::message::SyncResult) {
    let mut line = heapless::String::<64>::new();
    if wall_clock::codec::encode_result(result, &mut line).is_ok() {
        console::println!("{}", line.as_str());
    }
}
