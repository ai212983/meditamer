//! Display-side (core 0) WAKE/frontlight policy: press classification against
//! source timestamps, and fade math generalized to a nonzero baseline.
//!
//! This module is deliberately dependency-free so the host harness can
//! shadow it with `#[path]` and test the decision logic deterministically.
//! The acquisition task (core 1) only publishes classified timestamped
//! edges plus the generation/active snapshot; every long-press decision
//! below runs on the display task.
//!
//! Ownership: a press arms on the `Pressed` edge only. The short action
//! (flash to max, hold, fade to baseline) fires on release, so a long press
//! never flashes max. The long action (open calibration) fires once at the
//! hold deadline while still held; the later release is then consumed.

/// Hold duration that separates a short press from a long press. Matches the
/// Medinote button default (`ButtonTiming::long_press_ms`) and the meditamer
/// touch long-press constant (`TOUCH_LONG_PRESS_MS`).
pub const WAKE_LONG_PRESS_MS: u64 = 700;

/// Candidate shown when calibration opens while the baseline is off.
pub const FRONTLIGHT_CALIBRATION_INITIAL: u8 = 8;

/// Frontlight level while the baseline is disabled.
pub const FRONTLIGHT_BASELINE_OFF: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontlightCalibrationSession {
    pub original: u8,
    pub preview: u8,
}

impl FrontlightCalibrationSession {
    pub const fn new(original: u8) -> Self {
        Self {
            original,
            preview: if original == FRONTLIGHT_BASELINE_OFF {
                FRONTLIGHT_CALIBRATION_INITIAL
            } else {
                original
            },
        }
    }
}

/// Retry delay after a failed frontlight application. The driver already
/// retries internally, so a residual failure is persistent; re-attempt at a
/// bounded, non-spinning interval.
pub const FRONTLIGHT_APPLY_RETRY_MS: u64 = 250;

/// Whether a hardware application may run now. A failed write is retried at
/// a bounded deadline even if unrelated display work keeps the loop busy.
pub const fn application_due(retry_at_ms: Option<u64>, now_ms: u64) -> bool {
    match retry_at_ms {
        Some(retry_at_ms) => now_ms >= retry_at_ms,
        None => true,
    }
}

/// A pending retry represents uncertain hardware state even when the last
/// confirmed level equals the current desired level, so it still requires a
/// real write (and, for zero, a confirmed rail shutdown).
pub const fn needs_level_apply(current: u8, desired: u8, retry_pending: bool) -> bool {
    current != desired || retry_pending
}

/// While a write retry is pending it is the next useful hardware deadline.
/// An unchanged recorded level can otherwise make fade math return `now`
/// repeatedly and hot-loop before the retry is due.
pub const fn application_deadline(
    fade_deadline_ms: Option<u64>,
    retry_at_ms: Option<u64>,
) -> Option<u64> {
    match retry_at_ms {
        Some(retry_at_ms) => Some(retry_at_ms),
        None => fade_deadline_ms,
    }
}

/// Fade shape for one short-press flash. Field values come from the firmware
/// config constants; passing them explicitly keeps this module import-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FadeSpec {
    pub max: u8,
    pub hold_ms: u64,
    pub fade_ms: u64,
    pub baseline: u8,
}

/// A press armed from a classified `Pressed` edge. `pressed_ms` is the
/// source assertion timestamp, so deadline servicing stays correct when the
/// event queue delivers the edge late.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakePress {
    pub pressed_ms: u64,
    pub generation: u32,
    pub long_fired: bool,
}

impl WakePress {
    pub const fn new(pressed_ms: u64, generation: u32) -> Self {
        Self {
            pressed_ms,
            generation,
            long_fired: false,
        }
    }

    /// Source-timestamped hold duration in milliseconds.
    pub const fn held_ms(self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.pressed_ms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressArm {
    Armed(WakePress),
    IgnoredStale,
}

/// Arms a press from a `Pressed` edge. A generation that no longer matches
/// the acquisition snapshot (fault/suspend cancellation, or an edge that was
/// already superseded) is stale and must never arm. `active == false` with a
/// matching generation is a normal release whose queued edge still owns the
/// outcome, so it remains armable.
pub const fn arm_press(
    pressed_ms: u64,
    generation: u32,
    snapshot_generation: u32,
    _snapshot_active: bool,
) -> PressArm {
    // A matching inactive snapshot means the physical release was already
    // observed and its edge is queued behind this press. Cancellation and a
    // newer acceptance both bump the generation, so equality alone is the
    // staleness test here.
    if snapshot_generation != generation {
        return PressArm::IgnoredStale;
    }
    PressArm::Armed(WakePress::new(pressed_ms, generation))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// No press was armed, or the edge belongs to a cancelled/superseded
    /// hold. Never acts.
    Ignored,
    /// Released before the hold deadline: flash to max, hold, fade to baseline.
    ShortFlash,
    /// Released at or after the hold deadline without the deadline service
    /// having fired yet (delayed servicing): open calibration as a long press.
    LateLong,
    /// The deadline service already opened calibration for this hold: consume the
    /// release without further action, so a long press never flashes max.
    ConsumedLong,
}

/// Classifies a `Released` edge against the armed press. `released_ms` is
/// the source release timestamp; the `held_ms >= 700` boundary is long.
pub const fn classify_release(
    press: WakePress,
    released_ms: u64,
    release_generation: u32,
    snapshot_generation: u32,
    _snapshot_active: bool,
) -> ReleaseOutcome {
    if release_generation != press.generation || snapshot_generation != press.generation {
        return ReleaseOutcome::Ignored;
    }
    if press.long_fired {
        return ReleaseOutcome::ConsumedLong;
    }
    if press.held_ms(released_ms) >= WAKE_LONG_PRESS_MS {
        ReleaseOutcome::LateLong
    } else {
        ReleaseOutcome::ShortFlash
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldOutcome {
    /// Opens calibration exactly once. The caller marks the armed
    /// press `long_fired` and keeps it until release consumes it.
    OpenCalibration,
    /// The armed hold was cancelled or superseded (snapshot moved on):
    /// drop it without acting.
    DropStale,
    /// Still held short of the deadline, already fired, or nothing armed.
    Wait,
}

/// Deadline servicing for a still-held press. Fires at `held_ms >= 700`
/// only while the snapshot still shows this exact hold as active. A generation
/// mismatch is cancelled/stale and is dropped; a matching inactive snapshot
/// is a normal release and waits for its queued edge to decide the outcome.
pub const fn check_hold(
    press: Option<WakePress>,
    now_ms: u64,
    snapshot_generation: u32,
    snapshot_active: bool,
) -> HoldOutcome {
    let Some(press) = press else {
        return HoldOutcome::Wait;
    };
    if snapshot_generation != press.generation {
        return HoldOutcome::DropStale;
    }
    if press.long_fired || press.held_ms(now_ms) < WAKE_LONG_PRESS_MS {
        return HoldOutcome::Wait;
    }
    if snapshot_active {
        HoldOutcome::OpenCalibration
    } else {
        // The same-generation release edge owns the outcome. It may still be
        // queued behind the press, and its source timestamp decides short vs
        // late-long without pretending the button remains held.
        HoldOutcome::Wait
    }
}

/// Wakeup timestamp for the hold deadline, derived from the source press
/// timestamp so delayed servicing still decides against the true hold.
/// Present while a press is armed and unfired; afterwards the release path
/// owns the outcome and no deadline is needed.
pub const fn hold_deadline_ms(press: Option<WakePress>) -> Option<u64> {
    match press {
        Some(press) if !press.long_fired => {
            Some(press.pressed_ms.saturating_add(WAKE_LONG_PRESS_MS))
        }
        _ => None,
    }
}

/// Target level for `elapsed_ms` since the flash started, plus whether the
/// cycle is complete. Hold at max, then linear fade from max to the
/// baseline, then settle exactly at the baseline. With a zero baseline this
/// reduces to the original max-to-off formula.
pub const fn target_level_for_elapsed(spec: FadeSpec, elapsed_ms: u64) -> (u8, bool) {
    if elapsed_ms < spec.hold_ms {
        return (spec.max, false);
    }
    let fade_end = spec.hold_ms.saturating_add(spec.fade_ms);
    if elapsed_ms >= fade_end {
        return (spec.baseline, true);
    }
    let fade_remaining = fade_end.saturating_sub(elapsed_ms);
    let span = spec.max.saturating_sub(spec.baseline);
    let fade_ms = if spec.fade_ms == 0 { 1 } else { spec.fade_ms };
    let level = spec
        .baseline
        .saturating_add(((span as u64).saturating_mul(fade_remaining) / fade_ms) as u8);
    (level, false)
}

const fn const_max(a: u64, b: u64) -> u64 {
    if a > b {
        a
    } else {
        b
    }
}

const fn const_div_ceil(a: u64, divisor: u64) -> u64 {
    // Callers guarantee `divisor >= 1`.
    a.saturating_add(divisor.saturating_sub(1)) / divisor
}

/// Wakeup timestamp for the next fade step. `cycle_start_ms` is `None` when
/// idle at the baseline (or off): no deadline, so the loop never spins.
/// Returns `Some(now_ms)` for exactly one settling pass when the fade window
/// has passed but the recorded level is not yet at the baseline; after that
/// pass the cycle clears and the deadline is `None`, so there is no
/// terminal spin at level 0 or at the baseline.
pub const fn fade_deadline_ms(
    spec: FadeSpec,
    cycle_start_ms: Option<u64>,
    level: u8,
    now_ms: u64,
) -> Option<u64> {
    let Some(start_ms) = cycle_start_ms else {
        return None;
    };
    let fade_start = start_ms.saturating_add(spec.hold_ms);
    if now_ms < fade_start {
        return Some(fade_start.saturating_add(1));
    }
    let fade_end = fade_start.saturating_add(spec.fade_ms);
    if now_ms >= fade_end {
        if level == spec.baseline {
            return None;
        }
        return Some(now_ms);
    }
    if spec.max <= spec.baseline || level <= spec.baseline {
        return Some(const_max(fade_end, now_ms));
    }
    let remaining = const_div_ceil(
        ((level - spec.baseline) as u64).saturating_mul(spec.fade_ms),
        spec.max as u64 - spec.baseline as u64,
    );
    let remaining = if remaining == 0 { 1 } else { remaining };
    Some(const_max(
        fade_end.saturating_sub(remaining).saturating_add(1),
        now_ms,
    ))
}
