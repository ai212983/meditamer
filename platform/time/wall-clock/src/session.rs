//! [`Coordinator`]: the synchronization session state machine. Synchronization
//! is closed by default; a trigger opens one bounded session with a version,
//! a nonzero nonce, a deadline, and an allowed-source set. The first valid
//! reply from an allowed source claims the session; late, duplicate,
//! mismatched, and wrong-state replies are ignored (and counted in
//! [`SessionStats`]) rather than accepted.
//!
//! This module only tracks session admission -- applying an accepted reply
//! to the RTC and verifying it is [`crate::rtc_backend::apply_and_verify`],
//! which drives the `Applying`/`Verifying` transitions internally.

use crate::message::{
    SyncReply, SyncRequest, SyncResult, SyncStatus, TriggerReason, VerifyErrorReason,
    PROTOCOL_VERSION,
};

/// `now_ms >= deadline_ms`, but safe against `now_ms` wrapping around
/// `u32::MAX` (a free-running millisecond counter does this every ~49.7
/// days). Correct as long as `now_ms` never gets more than `i32::MAX` ms
/// (~24.8 days) ahead of `deadline_ms`, which every deadline in this crate
/// satisfies -- session timeouts are seconds, not weeks.
fn has_passed(now_ms: u32, deadline_ms: u32) -> bool {
    now_ms.wrapping_sub(deadline_ms) as i32 >= 0
}

/// Identifies which transport a reply arrived on, for source arbitration.
/// Up to 4 sources fit in [`SourceSet`]'s bitset; a single-transport board
/// just uses one fixed `SourceId` throughout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceId(pub u8);

/// A small bitset of allowed [`SourceId`]s for one session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSet(u8);

impl SourceSet {
    pub const fn single(source: SourceId) -> Self {
        SourceSet(1 << source.0)
    }

    pub const fn contains(&self, source: SourceId) -> bool {
        self.0 & (1 << source.0) != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Closed,
    AwaitingReply,
    Applying,
    Verifying,
}

/// Why a reply was not accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IgnoreReason {
    /// No session is currently `AwaitingReply` (closed, or already claimed
    /// and mid-apply/verify).
    WrongState,
    /// The reply's `session` does not match the open session.
    SessionMismatch,
    /// The reply's `nonce` does not match the open session's nonce.
    NonceMismatch,
    /// The reply arrived on a source the session did not allow.
    SourceNotAllowed,
    /// A reply for this session was already accepted; this is a duplicate or
    /// a collision from a second source.
    AlreadyClaimed,
    /// The reply arrived at or after the session's deadline.
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyOutcome {
    /// The first valid reply for this session; the caller should proceed to
    /// [`crate::rtc_backend::apply_and_verify`].
    Accepted,
    Ignored(IgnoreReason),
}

/// Counts of every [`IgnoreReason`] this coordinator has ever recorded.
/// Diagnostic, not policy: nothing here changes behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionStats {
    pub wrong_state: u32,
    pub session_mismatch: u32,
    pub nonce_mismatch: u32,
    pub source_not_allowed: u32,
    pub already_claimed: u32,
    pub expired: u32,
}

impl SessionStats {
    fn record(&mut self, reason: IgnoreReason) {
        let counter = match reason {
            IgnoreReason::WrongState => &mut self.wrong_state,
            IgnoreReason::SessionMismatch => &mut self.session_mismatch,
            IgnoreReason::NonceMismatch => &mut self.nonce_mismatch,
            IgnoreReason::SourceNotAllowed => &mut self.source_not_allowed,
            IgnoreReason::AlreadyClaimed => &mut self.already_claimed,
            IgnoreReason::Expired => &mut self.expired,
        };
        *counter = counter.saturating_add(1);
    }
}

/// The transport-neutral synchronization session state machine. Owns no RTC
/// and no transport -- just session admission. One instance per runtime
/// adapter (one per board's current awake lifecycle).
pub struct Coordinator {
    state: State,
    next_session: u32,
    session: u32,
    nonce: u32,
    deadline_ms: u32,
    allowed: SourceSet,
    accepted_source: Option<SourceId>,
    stats: SessionStats,
}

impl Coordinator {
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
            next_session: 1,
            session: 0,
            nonce: 0,
            deadline_ms: 0,
            allowed: SourceSet(0),
            accepted_source: None,
            stats: SessionStats {
                wrong_state: 0,
                session_mismatch: 0,
                nonce_mismatch: 0,
                source_not_allowed: 0,
                already_claimed: 0,
                expired: 0,
            },
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self.state, State::Closed)
    }

    /// Opens a new bounded session and returns the [`SyncRequest`] to send.
    /// `nonce` must be nonzero -- entropy sourcing is board-specific, so the
    /// caller supplies it. A session opened while another is still active
    /// simply replaces it (the caller is expected to check [`Self::is_closed`]
    /// first if that matters for its trigger policy).
    pub fn open_session(
        &mut self,
        trigger: TriggerReason,
        nonce: u32,
        now_ms: u32,
        timeout_ms: u32,
        allowed: SourceSet,
    ) -> SyncRequest {
        debug_assert!(nonce != 0, "wall-clock session nonce must be nonzero");
        let session = self.next_session;
        self.next_session = self.next_session.wrapping_add(1);
        self.session = session;
        self.nonce = nonce;
        // `wrapping_add`, paired with `has_passed`'s wraparound-safe signed
        // comparison below: `now_ms` is a free-running millisecond counter
        // (e.g. `embassy_time::Instant::now().as_millis() as u32`) that
        // wraps every ~49.7 days, and Inkplate's serial task is permanent --
        // a `saturating_add` deadline would stick at `u32::MAX` forever once
        // `now_ms` wrapped past it.
        self.deadline_ms = now_ms.wrapping_add(timeout_ms);
        self.allowed = allowed;
        self.accepted_source = None;
        self.state = State::AwaitingReply;
        SyncRequest {
            version: PROTOCOL_VERSION,
            session,
            nonce,
            trigger,
            deadline_ms: timeout_ms,
        }
    }

    /// Admits (or rejects) one reply against the currently open session.
    ///
    /// `Closed` (no session open at all) is `WrongState`; `Applying` and
    /// `Verifying` still belong to the same session, so a reply arriving
    /// during either of those falls through to the `accepted_source` check
    /// below and comes back `AlreadyClaimed` instead -- the more specific,
    /// still-correct answer for "this session already has its one accepted
    /// reply," whether that second reply is a genuine collision from another
    /// source or a resend queued up before this task got back around to
    /// awaiting the first one's `apply_and_verify`.
    pub fn on_reply(&mut self, source: SourceId, reply: &SyncReply, now_ms: u32) -> ReplyOutcome {
        if matches!(self.state, State::Closed) {
            self.stats.record(IgnoreReason::WrongState);
            return ReplyOutcome::Ignored(IgnoreReason::WrongState);
        }
        if matches!(self.state, State::AwaitingReply) && has_passed(now_ms, self.deadline_ms) {
            self.state = State::Closed;
            self.stats.record(IgnoreReason::Expired);
            return ReplyOutcome::Ignored(IgnoreReason::Expired);
        }
        if reply.session != self.session {
            self.stats.record(IgnoreReason::SessionMismatch);
            return ReplyOutcome::Ignored(IgnoreReason::SessionMismatch);
        }
        if reply.nonce != self.nonce {
            self.stats.record(IgnoreReason::NonceMismatch);
            return ReplyOutcome::Ignored(IgnoreReason::NonceMismatch);
        }
        if !self.allowed.contains(source) {
            self.stats.record(IgnoreReason::SourceNotAllowed);
            return ReplyOutcome::Ignored(IgnoreReason::SourceNotAllowed);
        }
        if self.accepted_source.is_some() {
            self.stats.record(IgnoreReason::AlreadyClaimed);
            return ReplyOutcome::Ignored(IgnoreReason::AlreadyClaimed);
        }

        self.accepted_source = Some(source);
        self.state = State::Applying;
        ReplyOutcome::Accepted
    }

    /// Time until an unanswered session expires. Runtime adapters can sleep
    /// until this deadline instead of polling. Other states have no RX deadline.
    pub fn timeout_remaining_ms(&self, now_ms: u32) -> Option<u32> {
        matches!(self.state, State::AwaitingReply).then(|| {
            if has_passed(now_ms, self.deadline_ms) {
                0
            } else {
                self.deadline_ms.wrapping_sub(now_ms)
            }
        })
    }

    /// Called at the session deadline by the runtime adapter; closes an expired
    /// `AwaitingReply` session as a timeout and returns the resulting
    /// [`SyncResult`]. A no-op in every other state.
    pub fn poll_timeout(&mut self, now_ms: u32) -> Option<SyncResult> {
        if matches!(self.state, State::AwaitingReply) && has_passed(now_ms, self.deadline_ms) {
            Some(self.finish(SyncStatus::Err, None, Some(VerifyErrorReason::Timeout)))
        } else {
            None
        }
    }

    pub fn stats(&self) -> SessionStats {
        self.stats
    }

    /// `Applying` -> `Verifying`, driven only by
    /// [`crate::rtc_backend::apply_and_verify`] once the RTC write itself
    /// has succeeded and the delayed readback is about to start.
    pub(crate) fn begin_verify(&mut self) {
        debug_assert!(matches!(self.state, State::Applying));
        self.state = State::Verifying;
    }

    /// Closes the session with a final result, from any state.
    pub(crate) fn finish(
        &mut self,
        status: SyncStatus,
        verified: Option<(u32, i16)>,
        reason: Option<VerifyErrorReason>,
    ) -> SyncResult {
        let session = self.session;
        self.state = State::Closed;
        SyncResult {
            version: PROTOCOL_VERSION,
            session,
            status,
            verified,
            reason,
        }
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}
