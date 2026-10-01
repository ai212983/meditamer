//! Pure minute-cadence policy for the runtime analog-clock ambient screen.
//!
//! This module has no LVGL and no crate-path dependency: it only uses
//! `core`, so it can be pulled unmodified into a host test harness (see
//! `test-support/host/ui_shell_host_harness`) via `#[path]`. Every item uses
//! `pub` rather than a `pub(in crate::firmware::ui)` path restriction
//! for exactly that reason -- the restriction is resolved against whichever
//! crate the file is compiled into.

/// Catalogue namespace shared with the other Meditamer apps.
pub const ENTRY_NAMESPACE: u16 = 1;
/// Catalogue local id for the analog-clock ambient entry.
pub const ENTRY_LOCAL: u16 = 6;
/// Shell surface id for the analog-clock ambient surface (matches the
/// shell's `SurfaceId(u16)` width).
pub const SURFACE_ID: u16 = 11;
/// Native dial geometry: the 600x600 source dial renders 1:1, no downscale.
pub const WIDTH_PX: u32 = 600;
pub const HEIGHT_PX: u32 = 600;

/// Seconds in a local day.
pub const SECONDS_PER_DAY: u32 = 86_400;
/// Seconds in one wall-clock minute.
pub const SECONDS_PER_MINUTE: u32 = 60;

/// Minutes past the hour that take the full-refresh path. A normal minute
/// tick renders a strict partial; the 10-minute boundaries (and the first
/// frame, and any skip/jump) go full so the e-paper waveform stays clean.
/// Minute 0 covers the hour boundary (the 60th minute's successor).
pub fn is_clean_minute(minute_of_hour: u8) -> bool {
    minute_of_hour.is_multiple_of(10)
}

/// Whole minutes since the local epoch, the dedupe/skip unit.
pub const fn epoch_minute(local_epoch_seconds: u32) -> u64 {
    (local_epoch_seconds / SECONDS_PER_MINUTE) as u64
}

/// Minute within the hour, 0..60.
pub const fn minute_of_hour(local_epoch_seconds: u32) -> u8 {
    ((local_epoch_seconds / SECONDS_PER_MINUTE) % 60) as u8
}

/// Local `(hour, minute)` for hand-angle evaluation.
pub const fn clock_h_m(local_epoch_seconds: u32) -> (u8, u8) {
    let day_seconds = local_epoch_seconds % SECONDS_PER_DAY;
    (
        (day_seconds / 3_600) as u8,
        minute_of_hour(local_epoch_seconds),
    )
}

/// Physical refresh class for one freshly observed wall minute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinuteIntent {
    /// Normal minute tick: strict partial push.
    Fast,
    /// First frame, 10-minute boundary, skipped-boundary catch-up, or
    /// backwards/time-jump recovery: full refresh.
    Clean,
}

/// What one fresh wall-clock read means for the tracker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeObservation {
    /// Same epoch minute as the last successfully published frame: keep
    /// rendering (or keep waiting); never restart or republish.
    Duplicate,
    /// A minute worth (re)targeting, with its refresh class.
    Target {
        epoch_minute: u64,
        intent: MinuteIntent,
    },
}

/// Tracks only the last *successfully published* minute. Rendering progress
/// never advances this: a frame that never reaches the panel must not
/// suppress its own retry.
#[derive(Clone, Copy, Debug)]
pub struct MinuteTracker {
    last_published_epoch_minute: Option<u64>,
    next_poll_due_ms: u64,
}

impl Default for MinuteTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MinuteTracker {
    /// Nothing published yet: the first activation (or reentry) observes as
    /// a clean target, and a poll is due immediately.
    pub const fn new() -> Self {
        Self {
            last_published_epoch_minute: None,
            next_poll_due_ms: 0,
        }
    }

    /// Monotonic deadline for the next wall-clock read. Driven only by
    /// observations and publishes below, plus the wall-minute rollover the
    /// last observation could see.
    pub const fn next_poll_due_ms(&self) -> u64 {
        self.next_poll_due_ms
    }

    /// Last successfully published epoch minute, if any. The anchored
    /// scheduler reads this to decide whether the estimated current minute
    /// is already covered (prefetch the upcoming minute) or needs a
    /// catch-up target; rendering progress never sets it.
    pub const fn last_published(&self) -> Option<u64> {
        self.last_published_epoch_minute
    }

    /// Classify a fresh wall-clock read. Updates the poll deadline; never
    /// marks anything published.
    pub fn observe(&mut self, local_epoch_seconds: u32, now_ms: u64) -> TimeObservation {
        let current = epoch_minute(local_epoch_seconds);
        // Next poll at the coming wall-minute rollover, so a minute change
        // is noticed without polling every tick.
        let to_next_minute_ms =
            u64::from(SECONDS_PER_MINUTE - (local_epoch_seconds % SECONDS_PER_MINUTE)) * 1_000;
        self.next_poll_due_ms = now_ms.saturating_add(to_next_minute_ms);
        match self.last_published_epoch_minute {
            None => TimeObservation::Target {
                epoch_minute: current,
                intent: MinuteIntent::Clean,
            },
            Some(last) if current == last => TimeObservation::Duplicate,
            Some(last) => TimeObservation::Target {
                epoch_minute: current,
                intent: intent_for_epoch(Some(last), current),
            },
        }
    }

    /// Record a completed panel publish. Only this advances deduplication.
    /// The wall-aligned deadline from the last [`observe`] is preserved, so
    /// publishes never push the next read out by a minute and drift the
    /// cadence. If the render finished after its deadline, a fresh read is
    /// due immediately instead.
    ///
    /// [`observe`]: MinuteTracker::observe
    pub fn published(&mut self, epoch_minute: u64, now_ms: u64) {
        self.last_published_epoch_minute = Some(epoch_minute);
        if now_ms >= self.next_poll_due_ms {
            self.next_poll_due_ms = now_ms;
        }
    }
}

/// What one non-blocking asset-queue attempt means for the retry budget.
/// The SD loader holds at most one outstanding whole-file read; while it
/// is in flight every further attempt answers `Busy`. That is the expected
/// pending state -- not a failure -- so it must never consume the bounded
/// retry budget or park the screen on its placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetQueueAttempt {
    /// The read was accepted and is now in flight; later polls reap it.
    Accepted,
    /// A read is already outstanding; keep polling, count nothing.
    Busy,
    /// The queue refused a fresh attempt while idle (channel full or
    /// similar): a genuine failure worth one bounded retry.
    Refused,
}

/// Bounded consecutive-failure counter for the single-outstanding SD asset
/// load. The renderer maps loader errors onto [`AssetQueueAttempt`] and
/// calls this; host tests drive the same type through the exact tick
/// sequence (accepted, many `Busy` polls, successful reply), so the
/// no-count-on-`Busy` contract is proven on the real policy, not a mirror.
#[derive(Clone, Copy, Debug)]
pub struct AssetRetryPolicy {
    failures: u8,
}

impl Default for AssetRetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetRetryPolicy {
    /// Genuine failures tolerated per activation before parking.
    pub const MAX_ATTEMPTS: u8 = 3;

    /// No failures yet: a fresh activation starts unparked.
    pub const fn new() -> Self {
        Self { failures: 0 }
    }

    /// Consecutive genuine failures so far (successful loads reset it).
    pub const fn failures(&self) -> u8 {
        self.failures
    }

    /// Whether the budget is exhausted and the screen should park.
    pub const fn is_parked(&self) -> bool {
        self.failures >= Self::MAX_ATTEMPTS
    }

    /// Record one queue attempt. Returns whether the screen parks after it.
    /// `Accepted` and `Busy` never advance the counter: an accepted request
    /// is still in flight, and `Busy` only reports that flight.
    pub fn note_queue_attempt(&mut self, attempt: AssetQueueAttempt) -> bool {
        match attempt {
            AssetQueueAttempt::Accepted | AssetQueueAttempt::Busy => false,
            AssetQueueAttempt::Refused => self.note_failure(),
        }
    }

    /// Record a completed transfer that reports a read error. Returns
    /// whether the screen parks after it.
    pub fn note_completion_error(&mut self) -> bool {
        self.note_failure()
    }

    /// Record a successfully loaded pack: the loader path works, so the
    /// consecutive-failure streak clears.
    pub fn note_assets_ready(&mut self) {
        self.failures = 0;
    }

    fn note_failure(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        self.is_parked()
    }
}

/// What the presentation layer may do with one settled frame, decided
/// only after the settled staging bits were copied into the retained
/// canvas. The canvas copy is the publication: minute deduplication may
/// advance only when the panel covers the new canvas, never on an
/// intended-but-unrendered publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockPublishDecision {
    /// Hold the staged frame and request nothing: the canvas copy failed,
    /// or a full repaint is not allowed this cycle. Retried later.
    Hold,
    /// The canvas landed and the strict-partial push covered it: the
    /// caller confirms only after that push succeeds.
    ConfirmFast,
    /// The canvas landed and the merged full repaint covers it: request
    /// the Clean merge, then confirm.
    RequestCleanAndConfirm,
}

/// Monotonic deadline for the next clock tick. `poll` needs wall-clock work
/// only when `now_ms` has reached BOTH the minute-tracker deadline and the
/// unavailable-read retry deadline, so with no cooperative work outstanding
/// the due instant is their maximum (a `0` retry sentinel means "no retry
/// pending" and never pulls the deadline back to immediate once the tracker
/// moved on). A failed engine parks (no deadline); an installed target --
/// rendering, asset-await, or publish-ready -- needs its cooperative row
/// budget soon, so the deadline is a near-future instant that lets the
/// executor yield instead of busy-waking on an already-due timer.
pub const fn resolve_clock_deadline(
    failed: bool,
    target_some: bool,
    tracker_due_ms: u64,
    retry_due_ms: u64,
    now_ms: u64,
) -> u64 {
    if failed {
        return u64::MAX;
    }
    if target_some {
        return now_ms.saturating_add(8);
    }
    if tracker_due_ms >= retry_due_ms {
        tracker_due_ms
    } else {
        retry_due_ms
    }
}

pub const fn retry_ready(now_ms: u64, retry_due_ms: u64) -> bool {
    now_ms >= retry_due_ms
}

/// Retry pauses apply to ready frames too. Otherwise, held frames wait on
/// their absolute boundary and rendering/prefetch gets a cooperative tick.
pub const fn clock_tick_deadline(
    failed: bool,
    work_pending: bool,
    held_boundary_ms: Option<u64>,
    tracker_due_ms: u64,
    retry_due_ms: u64,
    now_ms: u64,
) -> u64 {
    if failed {
        return u64::MAX;
    }
    if !retry_ready(now_ms, retry_due_ms) {
        return retry_due_ms;
    }
    if let Some(boundary) = held_boundary_ms {
        return boundary;
    }
    resolve_clock_deadline(false, work_pending, tracker_due_ms, retry_due_ms, now_ms)
}

/// Resolve one settled frame to a publish decision. Mirrors the branch in
/// the presentation `push_published_frame`: a Clean-classified frame (or a
/// Fast frame with another Clean request already pending, since Clean
/// wins) takes the full-refresh merge once allowed; a Fast frame takes the
/// strict partial. `canvas_landed` reports whether the staging-to-canvas
/// copy succeeded -- a failed copy always holds, so a blank or stale
/// canvas is never confirmed as published.
pub fn resolve_clock_publish(
    intent: MinuteIntent,
    clean_pending: bool,
    allow_clean: bool,
    canvas_landed: bool,
) -> ClockPublishDecision {
    if !allow_clean || !canvas_landed {
        return ClockPublishDecision::Hold;
    }
    if intent == MinuteIntent::Clean || clean_pending {
        ClockPublishDecision::RequestCleanAndConfirm
    } else {
        ClockPublishDecision::ConfirmFast
    }
}

/// Refresh class for one epoch minute given the last published minute.
/// This is the single intent table: [`MinuteTracker::observe`] uses it for
/// fresh RTC reads and the anchored scheduler uses it for estimated minutes
/// (prefetch and catch-up), so both paths classify identically. First ever
/// frame, backwards steps, wall-time jumps, skipped 10-minute boundaries,
/// and landings on one recover clean; ordinary advances go strict partial.
/// Minute 0 covers the hour boundary, and the range scan already covers the
/// landing minute, so those arms merge.
pub fn intent_for_epoch(last_published: Option<u64>, current: u64) -> MinuteIntent {
    match last_published {
        None => MinuteIntent::Clean,
        Some(last) if current == last => MinuteIntent::Fast,
        Some(last) => {
            if current < last
                || crosses_clean_boundary(last, current)
                || is_clean_minute((current % 60) as u8)
            {
                MinuteIntent::Clean
            } else {
                MinuteIntent::Fast
            }
        }
    }
}

/// Before the first successful publish, render the estimated current minute.
/// Thereafter prepare the upcoming minute in staging, including when the
/// previous publish crossed a boundary. This avoids repeated late catch-up
/// renders. The caller uses [`intent_for_epoch`] to preserve Clean recovery
/// when the next target skips a ten-minute boundary.
pub fn prefetch_epoch(last_published: Option<u64>, estimated_current: u64) -> u64 {
    match last_published {
        None => estimated_current,
        Some(_) => estimated_current.saturating_add(1),
    }
}

/// Measured full-frame render budget for budget-aware prefetch selection.
///
/// The bare [`prefetch_epoch`] stages `current + 1` after any publish, which
/// still misses when a slow first frame publishes late in its minute: with
/// only seconds left to the next boundary a ~55 s render overruns again and
/// the screen publishes at :51 repeatedly instead of aligning. This sampler
/// measures one full frame (first stepped row through `FrameReady`) and the
/// prefetch below then stages the earliest future wall minute whose
/// absolute boundary still leaves room for a whole render plus margin, so
/// startup recovery may stage `current + 2` while steady state (a 55 s
/// render plus margin fits the remaining ~59 s) stages the normal `+ 1`.
///
/// Tiny metadata (one in-flight sample plus one `u64` budget) kept inline
/// on the screen value: no globals, no statics, no buffers, no channels.
#[derive(Clone, Copy, Debug)]
pub struct RenderTiming {
    start: Option<(u64, u64)>,
    budget_ms: u64,
}

impl Default for RenderTiming {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderTiming {
    /// Conservative budget before the first measured sample: assume a full
    /// minute so an unknown phase stages safely rather than optimistically.
    pub const DEFAULT_BUDGET_MS: u64 = 60_000;
    /// Scheduling margin added to each measured render: the sample covers
    /// row stepping only, not the publish/panel handoff around it.
    pub const SCHEDULING_MARGIN_MS: u64 = 2_000;

    /// No sample in flight, conservative default budget.
    pub const fn new() -> Self {
        Self {
            start: None,
            budget_ms: Self::DEFAULT_BUDGET_MS,
        }
    }

    /// Current render budget in milliseconds: the last measured frame plus
    /// the scheduling margin, or the conservative default before any sample.
    pub const fn budget_ms(&self) -> u64 {
        self.budget_ms
    }

    /// Begin (or continue) a sample for `epoch_minute` at monotonic
    /// `now_ms`. Idempotent for the matching epoch, so the first-row tick
    /// through every following row tick shares one sample instead of
    /// restarting it per row. A different epoch (retarget after a publish
    /// or a stale discard) starts a new sample.
    pub fn begin(&mut self, epoch_minute: u64, now_ms: u64) {
        match self.start {
            Some((epoch, _)) if epoch == epoch_minute => {}
            _ => self.start = Some((epoch_minute, now_ms)),
        }
    }

    /// Finish the sample for `epoch_minute` at monotonic `now_ms`: the
    /// budget becomes the measured elapsed time plus the scheduling margin,
    /// and the in-flight sample clears. Only the matching epoch records;
    /// a stale epoch's completion is ignored. Saturates instead of
    /// wrapping on a backwards clock step.
    pub fn complete(&mut self, epoch_minute: u64, now_ms: u64) {
        if let Some((epoch, started)) = self.start {
            if epoch == epoch_minute {
                self.budget_ms = now_ms
                    .saturating_sub(started)
                    .saturating_add(Self::SCHEDULING_MARGIN_MS);
                self.start = None;
            }
        }
    }

    /// Budget-aware prefetch target. Before the first publish the estimated
    /// current minute still renders immediately. Afterwards the
    /// [`prefetch_epoch`] candidate (`current + 1`) is kept when its
    /// absolute wall boundary leaves room for a whole budgeted render
    /// (`boundary >= now + budget`); otherwise the target advances by the
    /// ceiling number of additional minutes, computed in pure saturating
    /// arithmetic (no loops, so saturated inputs stay bounded). The caller
    /// still classifies the result with [`intent_for_epoch`], preserving
    /// Clean recovery across skipped ten-minute boundaries.
    pub fn planned_prefetch(
        &self,
        last_published: Option<u64>,
        anchor: &WallAnchor,
        now_ms: u64,
    ) -> u64 {
        let estimated = anchor.estimated_epoch_minute(now_ms);
        if last_published.is_none() {
            return estimated;
        }
        let candidate = prefetch_epoch(last_published, estimated);
        let boundary = anchor.boundary_ms(candidate);
        let deadline = now_ms.saturating_add(self.budget_ms());
        if boundary >= deadline {
            return candidate;
        }
        let behind = deadline.saturating_sub(boundary);
        let minute_ms = u64::from(SECONDS_PER_MINUTE) * 1_000;
        let extra = behind
            .saturating_add(minute_ms.saturating_sub(1))
            .saturating_div(minute_ms)
            .max(1);
        candidate.saturating_add(extra)
    }
}

/// Synthetic local-epoch seconds (second zero) for an epoch minute, for
/// hand-angle evaluation of prefetched targets. Saturates instead of
/// wrapping; realistic epochs never approach the bound.
pub const fn epoch_to_local_seconds(epoch_minute: u64) -> u32 {
    let seconds = epoch_minute.saturating_mul(SECONDS_PER_MINUTE as u64);
    if seconds > u32::MAX as u64 {
        u32::MAX
    } else {
        seconds as u32
    }
}

/// One monotonic-to-local anchor: the wall time of one RTC read paired with
/// the monotonic tick that observed it. Between RTC resyncs (entry, the
/// 10-minute Clean-boundary validation, anomaly recovery, unavailable
/// retry) wall time is estimated from monotonic elapsed time, so row
/// stepping, hold decisions, and deadlines cost zero RTC round trips.
/// Sixteen bytes on the screen value: no statics, no channels, no buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WallAnchor {
    observed_monotonic_ms: u64,
    observed_local_epoch_seconds: u32,
}

impl WallAnchor {
    /// Record one valid RTC read taken at monotonic `now_ms`.
    pub const fn new(now_ms: u64, local_epoch_seconds: u32) -> Self {
        Self {
            observed_monotonic_ms: now_ms,
            observed_local_epoch_seconds: local_epoch_seconds,
        }
    }

    /// Estimated local-epoch seconds at monotonic `now_ms`. Saturates on a
    /// backwards monotonic step or `u32` overflow instead of wrapping.
    pub const fn estimate_local_epoch(&self, now_ms: u64) -> u32 {
        let elapsed_ms = now_ms.saturating_sub(self.observed_monotonic_ms);
        let elapsed_s = elapsed_ms / 1_000;
        let base = self.observed_local_epoch_seconds as u64;
        let estimated = base.saturating_add(elapsed_s);
        if estimated > u32::MAX as u64 {
            u32::MAX
        } else {
            estimated as u32
        }
    }

    /// Estimated whole minutes since the local epoch at `now_ms`.
    pub const fn estimated_epoch_minute(&self, now_ms: u64) -> u64 {
        (self.estimate_local_epoch(now_ms) / SECONDS_PER_MINUTE) as u64
    }

    /// Absolute monotonic instant the wall minute `epoch_minute` starts,
    /// derived from this anchor. Stable for a fixed anchor: repeated calls
    /// return the same deadline (never a sliding now-plus). Saturates
    /// instead of underflowing for minutes that predate the anchor.
    pub const fn boundary_ms(&self, epoch_minute: u64) -> u64 {
        let minute_start_s = epoch_minute.saturating_mul(SECONDS_PER_MINUTE as u64);
        let anchor_s = self.observed_local_epoch_seconds as u64;
        if minute_start_s <= anchor_s {
            self.observed_monotonic_ms
                .saturating_sub((anchor_s - minute_start_s).saturating_mul(1_000))
        } else {
            self.observed_monotonic_ms
                .saturating_add((minute_start_s - anchor_s).saturating_mul(1_000))
        }
    }

    /// Absolute monotonic instant the minute after the estimated current
    /// minute starts: the stable deadline a held future frame waits on.
    pub const fn next_boundary_ms(&self, now_ms: u64) -> u64 {
        self.boundary_ms(self.estimated_epoch_minute(now_ms).saturating_add(1))
    }
}

/// What a completed frame means under anchor-estimated (RTC-free) time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EstimatedSettle {
    /// The frame is for the estimated current minute: publish it now with
    /// this refresh class.
    PublishCurrent(MinuteIntent),
    /// The frame is for a future minute: hold the completed staging frame
    /// without busy looping until this absolute monotonic boundary, then
    /// publish. The already-retained canvas stays visible meanwhile.
    HoldFuture { boundary_ms: u64 },
    /// The frame's minute already ended (render overran the wall clock):
    /// the caller discards it and retargets the estimated current minute.
    /// The retarget goes Clean as the safety fallback -- after an overrun
    /// the panel may show minutes-old time, so the recovery is a full
    /// refresh rather than a strict partial.
    StaleRetarget { epoch_minute: u64 },
}

/// Freshness-check one completed frame against anchor-estimated time, with
/// no RTC traffic. `target_epoch_minute`/`intent` identify the finished
/// frame, `estimated_current` is the anchor's current-minute estimate.
pub const fn settle_estimate(
    target_epoch_minute: u64,
    intent: MinuteIntent,
    estimated_current: u64,
    anchor: &WallAnchor,
) -> EstimatedSettle {
    if estimated_current < target_epoch_minute {
        EstimatedSettle::HoldFuture {
            boundary_ms: anchor.boundary_ms(target_epoch_minute),
        }
    } else if estimated_current == target_epoch_minute {
        EstimatedSettle::PublishCurrent(intent)
    } else {
        EstimatedSettle::StaleRetarget {
            epoch_minute: estimated_current,
        }
    }
}

/// Whether any wall minute in `(last, current]` lands on a 10-minute
/// boundary. Caps the scan: a gap beyond two hours is a jump, which is
/// clean by definition.
fn crosses_clean_boundary(last: u64, current: u64) -> bool {
    if current.saturating_sub(last) > 120 {
        return true;
    }
    let mut m = last.saturating_add(1);
    while m <= current {
        if is_clean_minute((m % 60) as u8) {
            return true;
        }
        m = m.saturating_add(1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_is_clean_target() {
        let mut tracker = MinuteTracker::new();
        let at_epoch = 9 * 3_600 + 60;
        match tracker.observe(at_epoch, 1_000) {
            TimeObservation::Target { intent, .. } => assert_eq!(intent, MinuteIntent::Clean),
            TimeObservation::Duplicate => panic!("first observation must target"),
        }
    }
}
