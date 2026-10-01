//! Selectable runtime analog-clock ambient screen (Durer 600x600 dial).
//!
//! Kept alongside the existing arc/environment ambient view: the catalogue
//! holds both, the persisted binding defaults still fall back to Home, and
//! the arc/tap-clock behavior is untouched. This surface already shows the
//! time, so it never opens the tap-clock overlay.
//!
//! Wall-clock time comes from the serial task -- the sole owner of RTC I2C
//! access -- over the existing request channel. Each valid snapshot
//! establishes a monotonic-to-local [`model::WallAnchor`]; between RTC
//! resyncs (entry, the 10-minute Clean-boundary validation, anomaly
//! recovery, unavailable retry) wall time is estimated from monotonic
//! elapsed time. Row stepping, future-frame holds, and deadlines therefore
//! cost zero RTC round trips: a full frame needs no per-row snapshot.
//! Rendering progress never requests a physical refresh; only a completed,
//! freshness-checked frame publishes into the LVGL canvas and emits a
//! Fast/Clean semantic.

mod composition;
mod model;
mod render;

pub(crate) use self::model::{
    resolve_clock_publish, ClockPublishDecision, EstimatedSettle, MinuteIntent,
};
pub(in crate::firmware::ui) use self::model::{ENTRY_LOCAL, ENTRY_NAMESPACE, SURFACE_ID};
pub(crate) use self::render::TargetMinute;
use self::render::{FrameSettlement, CLOCK_HEIGHT, CLOCK_WIDTH};
pub(in crate::firmware::ui) use self::render::{RenderEngine, SharedCache};

use ::render::lvgl_adapter::{
    AccessFault, ExternalL8CanvasBuffer, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
use rtc::driver::WallClockSnapshot;
use shell::types::SurfaceInstanceToken;

use crate::firmware::psram::ExternalValue;
use crate::firmware::storage::clock_assets::take_assets;

/// Retry a missing wall-clock read no sooner than this; never fabricate time.
const UNAVAILABLE_RETRY_MS: u64 = 30_000;
/// L8 canvas pixels published per frame.
pub const CANVAS_PIXELS: usize = 360_000;
/// Loading fill painted at entry so the wait for assets/rows never reads as
/// a blank white screen.
const PLACEHOLDER_LOADING: u8 = 0xD8;
/// Failure field + border painted once when assets/PSRAM/render fail.
const PLACEHOLDER_FAILED_FIELD: u8 = 0xB0;
const PLACEHOLDER_FAILED_BORDER: u8 = 0x40;
const PLACEHOLDER_BORDER_PX: usize = 8;

/// What the next analog-clock tick should do. Returned by the cheap,
/// monotonic-clock-only [`AnalogClockScreen::poll`] so the async wall-clock
/// fetch only happens when actually due.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClockPoll {
    /// Stale instance: do nothing.
    Inactive,
    /// Active but nothing due (rendering caught up, waiting on the minute
    /// rollover, loader, or a parked placeholder).
    Idle,
    /// No target minute: fetch a fresh wall-clock snapshot.
    NeedsTime,
    /// Target set and rows remain: advance the cooperative row budget.
    Step,
    /// A frame finished and is unpublished: fetch fresh time to check it
    /// is still current, then publish or discard.
    PublishReady(TargetMinute),
}

/// Outcome of settling a completed frame against fresh wall time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettleOutcome {
    /// Frame is current: published with this refresh class.
    Published(model::MinuteIntent),
    /// Frame went stale (or time is unavailable): discarded or held, no
    /// panel update emitted.
    Held,
}

pub(in crate::firmware::ui) struct AnalogClockScreen {
    root: Widget,
    canvas_widget: Widget,
    instance: SurfaceInstanceToken,
    engine: RenderEngine,
    /// Monotonic-to-local anchor from the last valid RTC snapshot (16 bytes
    /// inline, no statics or buffers). `None` until the entry snapshot or
    /// an unavailable-retry succeeds; never fabricated.
    anchor: Option<model::WallAnchor>,
    /// Measured frame-render budget for prefetch selection (one in-flight
    /// sample plus one `u64`, inline on this value: no statics, buffers,
    /// channels, or new bulk allocation).
    timing: model::RenderTiming,
    time_retry_due_ms: u64,
    failure_placeholder_painted: bool,
    /// One-shot activation probe flag. Per-instance bool on the existing
    /// screen value: no globals, no statics, no new allocation.
    activation_reported: bool,
}

fn abandon(root: Widget, token: &UiAccessToken) -> Option<AnalogClockScreen> {
    if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
        ::render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
    }
    None
}

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    instance: SurfaceInstanceToken,
    shared: &mut SharedCache,
) -> Option<AnalogClockScreen> {
    let engine = RenderEngine::new().ok()?;
    let root = Widget::screen(token).ok()?;
    let canvas_widget = match root.child(token, WidgetKind::Canvas) {
        Ok(widget) => widget,
        Err(_) => return abandon(root, token),
    };
    // Prepared pixels already contain all clock styling. Keep the source
    // command opaque and free of theme image effects, matching the draw-unit
    // contract exercised by the host prototype. Native overlays keep their styles.
    if canvas_widget.remove_style_all(token).is_err()
        || canvas_widget.set_pos(token, 0, 0).is_err()
        || canvas_widget
            .set_size(token, CLOCK_WIDTH as i32, CLOCK_HEIGHT as i32)
            .is_err()
    {
        return abandon(root, token);
    }
    // Boot-lifetime canvas: adopt strict-PSRAM L8 storage once and reuse it
    // across navigations -- never per entry, never on the stack. The small
    // keeper wrapper is allocated BEFORE the large owned buffer is leaked,
    // so a wrapper OOM loses nothing and the next entry retries cleanly
    // instead of pinning a permanent failure over a lost 360 KB.
    if shared.canvas.is_none() {
        let keeper: ExternalValue<Option<ExternalL8CanvasBuffer>> =
            match ExternalValue::try_new_with(|| None) {
                Ok(keeper) => keeper,
                Err(_) => {
                    console::println!("CLOCK_CANVAS status=keeper_oom");
                    return abandon(root, token);
                }
            };
        let storage = match shared.canvas_or_allocate() {
            Ok(storage) => storage,
            Err(_) => return abandon(root, token),
        };
        // Storage was placement- and alignment-verified before leaking, so
        // the adapter constructor cannot fail on those grounds (it only
        // rejects empty/misaligned storage).
        let adopted = match ExternalL8CanvasBuffer::new(storage) {
            Ok(buffer) => buffer,
            Err(_) => {
                console::println!("CLOCK_CANVAS status=adopt_failed");
                return abandon(root, token);
            }
        };
        let slot: &'static mut Option<ExternalL8CanvasBuffer> = keeper.leak();
        *slot = Some(adopted);
        shared.canvas = Some(slot.as_ref().expect("clock canvas keeper just installed"));
    }
    let Some(canvas) = shared.canvas else {
        return abandon(root, token);
    };
    if canvas_widget
        .set_external_l8_canvas_buffer(token, canvas, CLOCK_WIDTH as i32, CLOCK_HEIGHT as i32)
        .is_err()
    {
        return abandon(root, token);
    }
    if !composition::configure(token, shared) {
        return abandon(root, token);
    }
    // Loading state: the base pass + frame rows take many ticks, so paint
    // the canvas immediately instead of leaving adapter white up. The
    // binary panel renders L8 values below 128 as black, so the flat
    // 0xD8 fill alone would read as a blank white screen: add a dark
    // border plus a diagonal cross. Bounded immediate writes only, no
    // extra buffers; the failure placeholder keeps its plain bordered
    // field so the two states stay distinguishable.
    let _ = canvas.with_writer(token, |writer| {
        writer.fill(PLACEHOLDER_LOADING);
        let w = CLOCK_WIDTH as usize;
        let h = CLOCK_HEIGHT as usize;
        for y in 0..PLACEHOLDER_BORDER_PX {
            for x in 0..w {
                writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                writer.write((h - 1 - y) * w + x, PLACEHOLDER_FAILED_BORDER);
            }
        }
        for y in PLACEHOLDER_BORDER_PX..h - PLACEHOLDER_BORDER_PX {
            for x in 0..PLACEHOLDER_BORDER_PX {
                writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                writer.write(y * w + (w - 1 - x), PLACEHOLDER_FAILED_BORDER);
            }
        }
        let diagonal = w.min(h);
        for i in 0..diagonal {
            writer.write(i * w + i, PLACEHOLDER_FAILED_BORDER);
            writer.write(i * w + (w - 1 - i), PLACEHOLDER_FAILED_BORDER);
        }
    });
    let _ = canvas_widget.invalidate(token);
    console::println!("CLOCK_SCREEN status=entered");
    Some(AnalogClockScreen {
        root,
        canvas_widget,
        instance,
        engine,
        anchor: None,
        timing: model::RenderTiming::new(),
        time_retry_due_ms: 0,
        failure_placeholder_painted: false,
        activation_reported: false,
    })
}

impl AnalogClockScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        // Drain any late loader result even though we navigated away, so
        // its buffer is freed and the single outstanding read is released.
        while take_assets().is_some() {}
        let Self {
            root,
            canvas_widget,
            instance,
            engine,
            anchor,
            timing,
            time_retry_due_ms,
            failure_placeholder_painted: _,
            activation_reported,
        } = self;
        match root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                root,
                canvas_widget,
                instance,
                engine,
                anchor,
                timing,
                time_retry_due_ms,
                failure_placeholder_painted: false,
                activation_reported,
            }),
        }
    }

    pub(in crate::firmware::ui) fn instance(&self) -> SurfaceInstanceToken {
        self.instance
    }

    /// One-shot activation probe. Emits the full first-poll gate vector
    /// once per screen instance so a single flash identifies which gate
    /// holds a fresh clock at `Inactive` before any asset/time work runs.
    /// Callers must invoke this outside the instance-match gate so a
    /// stale-instance wedge reports itself instead of staying silent.
    pub(in crate::firmware::ui) fn report_activation_once(
        &mut self,
        active_instance: SurfaceInstanceToken,
        lvgl: Result<bool, AccessFault>,
        frame_match: bool,
        token_match: bool,
        now_ms: u64,
    ) {
        if self.activation_reported {
            return;
        }
        self.activation_reported = true;
        let poll = self.poll(now_ms, active_instance);
        console::println!(
            "CLOCK_ACTIVATION screen={:?} active={:?} instance_match={} lvgl={:?} frame_match={} token_match={} phase={} target_none={} tracker_due_ms={} retry_due_ms={} poll={:?}",
            self.instance,
            active_instance,
            self.instance == active_instance,
            lvgl,
            frame_match,
            token_match,
            self.engine.phase_label(),
            self.engine.target().is_none(),
            self.engine.tracker().next_poll_due_ms(),
            self.time_retry_due_ms,
            poll,
        );
    }

    /// Monotonic deadline for the next wall-clock read, retry, held-future
    /// boundary, or cooperative row budget. A held future frame waits on its
    /// absolute wall-minute boundary (stable for a fixed anchor, never a
    /// sliding now-plus); otherwise `poll` requires reaching both the
    /// tracker and retry dues, so an idle screen is due at their maximum,
    /// and installed row work resolves to a near-future instant so the
    /// executor yields instead of busy-waking on an already-due timer.
    pub(in crate::firmware::ui) fn next_deadline_ms(&self, now_ms: u64) -> u64 {
        model::clock_tick_deadline(
            self.engine.failed().is_some(),
            self.engine.target().is_some() || self.anchor.is_some(),
            self.held_boundary_ms(now_ms),
            self.engine.tracker().next_poll_due_ms(),
            self.time_retry_due_ms,
            now_ms,
        )
    }

    /// Absolute monotonic boundary a completed future frame is held until,
    /// if one is staged. `None` while rendering, while the frame is current,
    /// or without an anchor: those paths keep the existing cooperative and
    /// retry deadlines below.
    fn held_boundary_ms(&self, now_ms: u64) -> Option<u64> {
        let target = self.engine.frame_ready()?;
        match self.anchor {
            Some(anchor) if anchor.estimated_epoch_minute(now_ms) < target.epoch_minute => {
                Some(anchor.boundary_ms(target.epoch_minute))
            }
            Some(_) => None,
            // No time basis: wait for the unavailable-retry instant instead
            // of busy-waking on the cooperative tick.
            None => Some(self.time_retry_due_ms),
        }
    }

    /// Anchor-estimated current epoch minute, if any snapshot anchored the
    /// screen yet. The presentation layer reads this to hold future frames
    /// without spending an RTC round trip.
    pub(in crate::firmware::ui) fn estimated_current(&self, now_ms: u64) -> Option<u64> {
        self.anchor
            .map(|anchor| anchor.estimated_epoch_minute(now_ms))
    }

    /// Monotonic-clock-only check that also installs the next render target.
    /// Never touches RTC/I2C itself. When the engine is idle with a valid
    /// anchor, the target comes from the estimate -- the upcoming minute
    /// while the current one is already published (prepared in staging
    /// during the preceding minute), or the estimated current minute for
    /// first frames and catch-ups -- so steady-state rendering needs no
    /// snapshot. Rendering progress here never requests a physical refresh.
    pub(in crate::firmware::ui) fn poll(
        &mut self,
        now_ms: u64,
        active_instance: SurfaceInstanceToken,
    ) -> ClockPoll {
        if self.instance != active_instance {
            return ClockPoll::Inactive;
        }
        if self.engine.failed().is_some() {
            return ClockPoll::Idle;
        }
        if !model::retry_ready(now_ms, self.time_retry_due_ms) {
            return ClockPoll::Idle;
        }
        if let Some(anchor) = self.anchor {
            self.engine
                .observe_estimated_minute(anchor.estimated_epoch_minute(now_ms));
        }
        if let Some(target) = self.engine.frame_ready() {
            return ClockPoll::PublishReady(target);
        }
        if self.engine.target().is_none() {
            if let Some(anchor) = self.anchor {
                let last = self.engine.tracker().last_published();
                let epoch = self.timing.planned_prefetch(last, &anchor, now_ms);
                self.engine.set_target(TargetMinute::new(
                    epoch,
                    model::epoch_to_local_seconds(epoch),
                    model::intent_for_epoch(last, epoch),
                ));
                return ClockPoll::Step;
            }
            if now_ms >= self.time_retry_due_ms
                && now_ms >= self.engine.tracker().next_poll_due_ms()
            {
                return ClockPoll::NeedsTime;
            }
            return ClockPoll::Idle;
        }
        ClockPoll::Step
    }

    /// Apply a freshly fetched snapshot: anchor monotonic-to-local time,
    /// then retarget on a new minute, ignore duplicates, and count down
    /// unavailable reads without fabricating time.
    pub(in crate::firmware::ui) fn observe_time(
        &mut self,
        snapshot: Option<WallClockSnapshot>,
        now_ms: u64,
    ) {
        let Some(snapshot) = snapshot.filter(|snapshot| snapshot.valid) else {
            console::println!("CLOCK_TIME status=unavailable");
            self.time_retry_due_ms = now_ms.saturating_add(UNAVAILABLE_RETRY_MS);
            return;
        };
        self.anchor = Some(model::WallAnchor::new(now_ms, snapshot.local_epoch_seconds));
        self.time_retry_due_ms = 0;
        match self
            .engine
            .tracker_mut()
            .observe(snapshot.local_epoch_seconds, now_ms)
        {
            model::TimeObservation::Duplicate => {}
            model::TimeObservation::Target {
                epoch_minute,
                intent,
            } => {
                self.engine.set_target(TargetMinute::new(
                    epoch_minute,
                    snapshot.local_epoch_seconds,
                    intent,
                ));
            }
        }
    }

    /// Advance loader/base/frame work by the per-tick row budgets. Touches
    /// only PSRAM staging buffers, never the published canvas or panel.
    /// Samples the frame-render budget around the engine tick: the sample
    /// starts (idempotently per epoch) while rows step or the frame sits
    /// ready, and completes when the frame is ready. Base, asset, and
    /// placeholder work never counts.
    pub(in crate::firmware::ui) fn service(
        &mut self,
        shared: &mut SharedCache,
    ) -> Option<TargetMinute> {
        let before_ms = embassy_time::Instant::now().as_millis();
        let completed = self.engine.service(shared);
        // The pre-service instant covers the first row budget too; begin is
        // idempotent per epoch, so first-row through last-row ticks share
        // one sample and a retarget starts a new one.
        if let Some(target) = self.engine.target() {
            if self.engine.is_rendering_frame() || self.engine.frame_ready().is_some() {
                self.timing.begin(target.epoch_minute, before_ms);
            }
        }
        if let Some(ready) = self.engine.frame_ready() {
            let now_ms = embassy_time::Instant::now().as_millis();
            self.timing.complete(ready.epoch_minute, now_ms);
        }
        completed
    }

    /// Freshness gate for a completed frame against a fresh snapshot. The
    /// valid snapshot also re-anchors monotonic-to-local time, so the
    /// 10-minute Clean-boundary validation doubles as the periodic resync.
    /// A frame finished before its minute holds for the wall boundary; a
    /// render that finished after its minute ended is discarded and the
    /// estimated current minute is retargeted Clean (safety fallback after
    /// an overrun) rather than presenting the wrong minute.
    pub(in crate::firmware::ui) fn settle(
        &mut self,
        snapshot: Option<WallClockSnapshot>,
        completed: TargetMinute,
        now_ms: u64,
    ) -> SettleOutcome {
        let Some(snapshot) = snapshot.filter(|snapshot| snapshot.valid) else {
            console::println!("CLOCK_TIME status=unavailable");
            self.time_retry_due_ms = now_ms.saturating_add(UNAVAILABLE_RETRY_MS);
            return SettleOutcome::Held;
        };
        self.anchor = Some(model::WallAnchor::new(now_ms, snapshot.local_epoch_seconds));
        self.time_retry_due_ms = 0;
        let current = model::epoch_minute(snapshot.local_epoch_seconds);
        match self.engine.settle_minute(completed, current) {
            FrameSettlement::Current => {}
            FrameSettlement::Hold => return SettleOutcome::Held,
            FrameSettlement::Retargeted => {
                console::println!("CLOCK_FRAME status=stale_minute");
                return SettleOutcome::Held;
            }
        }
        // The target stays installed: minute deduplication advances only in
        // confirm_published, after the panel actually covers the frame. An
        // intended refresh the scheduler rejects stays pending for retry
        // instead of counting as a successful panel.
        SettleOutcome::Published(completed.intent)
    }

    /// RTC-free freshness gate for a completed frame, using the anchor
    /// estimate. Ordinary Fast minutes publish through here with zero RTC
    /// traffic; Clean minutes use [`settle`](Self::settle) instead so the
    /// boundary snapshot validates and resyncs the anchor.
    pub(in crate::firmware::ui) fn settle_estimated(
        &mut self,
        completed: TargetMinute,
        now_ms: u64,
    ) -> model::EstimatedSettle {
        let Some(anchor) = self.anchor else {
            // No time basis at all: never fabricate time. The first pass
            // triggers one snapshot fetch (the stale path below); while the
            // retry is pending the frame simply waits for its instant.
            if now_ms < self.time_retry_due_ms {
                return model::EstimatedSettle::HoldFuture {
                    boundary_ms: self.time_retry_due_ms,
                };
            }
            console::println!("CLOCK_TIME status=unanchored");
            self.time_retry_due_ms = now_ms.saturating_add(UNAVAILABLE_RETRY_MS);
            return model::EstimatedSettle::StaleRetarget {
                epoch_minute: completed.epoch_minute,
            };
        };
        let estimated = anchor.estimated_epoch_minute(now_ms);
        match model::settle_estimate(completed.epoch_minute, completed.intent, estimated, &anchor) {
            model::EstimatedSettle::PublishCurrent(intent) => {
                if self.engine.target().is_none() {
                    // Already published (or a duplicate completion): never
                    // republish; recheck at the next boundary.
                    model::EstimatedSettle::HoldFuture {
                        boundary_ms: anchor.next_boundary_ms(now_ms),
                    }
                } else {
                    model::EstimatedSettle::PublishCurrent(intent)
                }
            }
            model::EstimatedSettle::HoldFuture { boundary_ms } => {
                model::EstimatedSettle::HoldFuture { boundary_ms }
            }
            model::EstimatedSettle::StaleRetarget { epoch_minute } => {
                console::println!("CLOCK_FRAME status=stale_minute");
                self.engine.discard_stale(completed);
                self.retarget_clean(epoch_minute, now_ms);
                model::EstimatedSettle::StaleRetarget { epoch_minute }
            }
        }
    }

    /// Discard recovery: retarget an epoch minute Clean. Records the
    /// wall-aligned tracker deadline (without advancing deduplication) and
    /// installs a Clean target, so a render that overran the wall clock
    /// recovers with a full refresh instead of a strict partial over
    /// minutes-old content.
    fn retarget_clean(&mut self, epoch_minute: u64, now_ms: u64) {
        let local = model::epoch_to_local_seconds(epoch_minute);
        let _ = self.engine.tracker_mut().observe(local, now_ms);
        self.engine.set_target(TargetMinute::new(
            epoch_minute,
            local,
            model::MinuteIntent::Clean,
        ));
    }

    /// Record that the panel covered `target` (strict-partial push admitted
    /// or Clean full repaint merged). Only this advances minute
    /// deduplication; rendering progress and settle never do.
    pub(in crate::firmware::ui) fn confirm_published(&mut self, target: TargetMinute, now_ms: u64) {
        self.engine.mark_published(target, now_ms);
    }

    /// Whether the failure placeholder still needs painting: the engine
    /// failed and no failure pattern has been published yet.
    pub(in crate::firmware::ui) fn failure_placeholder_needed(&self) -> bool {
        self.engine.failed().is_some() && !self.failure_placeholder_painted
    }

    /// Paint the failure pattern once (gray field + dark border) so a
    /// missing-asset/PSRAM/render failure reads as a clock error, never a
    /// blank screen. No-op unless the engine failed.
    pub(in crate::firmware::ui) fn paint_failure_placeholder(
        &mut self,
        token: &UiAccessToken,
        canvas: &'static ExternalL8CanvasBuffer,
    ) {
        let Some(failure) = self.engine.failed() else {
            return;
        };
        if self.failure_placeholder_painted {
            return;
        }
        console::println!("CLOCK_PLACEHOLDER status=failed kind={:?}", failure);
        let painted = canvas
            .with_writer(token, |writer| {
                writer.fill(PLACEHOLDER_FAILED_FIELD);
                let w = CLOCK_WIDTH as usize;
                let h = CLOCK_HEIGHT as usize;
                for y in 0..PLACEHOLDER_BORDER_PX {
                    for x in 0..w {
                        writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                        writer.write((h - 1 - y) * w + x, PLACEHOLDER_FAILED_BORDER);
                    }
                }
                for y in PLACEHOLDER_BORDER_PX..h - PLACEHOLDER_BORDER_PX {
                    for x in 0..PLACEHOLDER_BORDER_PX {
                        writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                        writer.write(y * w + (w - 1 - x), PLACEHOLDER_FAILED_BORDER);
                    }
                }
            })
            .is_ok();
        if painted {
            self.failure_placeholder_painted = true;
            self.invalidate_canvas(token);
        }
    }

    pub(in crate::firmware::ui) fn staging_bits(&self) -> Option<&[u8]> {
        self.engine.staging_bits()
    }

    pub(in crate::firmware::ui) fn invalidate_canvas(&self, token: &UiAccessToken) -> bool {
        self.canvas_widget.invalidate(token).is_ok()
    }

    /// Publish helper shared by the backend glue: copy settled staging
    /// bits into the retained canvas and invalidate it. Returns true only
    /// when a full frame landed in the canvas; the caller measures damage
    /// through its normal publish frame.
    pub(in crate::firmware::ui) fn publish_staging(
        &self,
        token: &UiAccessToken,
        canvas: &'static ExternalL8CanvasBuffer,
    ) -> bool {
        let Some(bits) = self.staging_bits() else {
            return false;
        };
        let wrote = canvas
            .with_writer(token, |writer| writer.write_ink_bits(bits, CANVAS_PIXELS))
            .ok();
        if wrote != Some(CANVAS_PIXELS) {
            return false;
        }
        self.invalidate_canvas(token)
    }
}

/// Reset per-publication draw counters without touching numerical preparation.
pub(in crate::firmware::ui) fn begin_composition(shared: &SharedCache) {
    composition::begin(shared);
}

/// Report the LVGL publication pass, excluding the physical panel waveform.
pub(in crate::firmware::ui) fn report_composition(shared: &SharedCache, elapsed_us: u64) {
    composition::report(shared, elapsed_us);
}
