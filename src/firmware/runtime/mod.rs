mod bootstrap;
pub(crate) mod display_task;
mod serial_task;
pub use bootstrap::run;

use embassy_time::Instant;

use super::{
    config::{
        BACKLIGHT_FADE_MS, BACKLIGHT_HOLD_MS, BACKLIGHT_MAX_BRIGHTNESS, FACE_BASELINE_HOLD_MS,
        FACE_BASELINE_RECALIBRATE_MS, FACE_DOWN_HOLD_MS, FACE_DOWN_REARM_MS,
        FACE_NORMAL_MIN_ABS_AXIS, FACE_NORMAL_MIN_GAP,
    },
    types::InkplateDriver,
};

#[derive(Clone, Copy)]
pub(crate) struct FaceDownToggleState {
    baseline_pose: Option<FacePose>,
    baseline_candidate: Option<FacePose>,
    baseline_candidate_since: Option<Instant>,
    face_down_since: Option<Instant>,
    rearm_since: Option<Instant>,
    latched: bool,
}

impl FaceDownToggleState {
    pub(crate) fn new() -> Self {
        Self {
            baseline_pose: None,
            baseline_candidate: None,
            baseline_candidate_since: None,
            face_down_since: None,
            rearm_since: None,
            latched: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FacePose {
    axis: u8,
    sign: i8,
}

pub(crate) fn update_face_down_toggle(
    state: &mut FaceDownToggleState,
    now: Instant,
    ax: i16,
    ay: i16,
    az: i16,
) -> bool {
    let ax_i32 = ax as i32;
    let ay_i32 = ay as i32;
    let az_i32 = az as i32;
    let Some(pose) = detect_face_pose(ax_i32, ay_i32, az_i32) else {
        state.face_down_since = None;
        state.rearm_since = None;
        return false;
    };

    if state.baseline_pose.is_none() {
        if update_baseline_candidate(state, now, pose, FACE_BASELINE_HOLD_MS) {
            state.baseline_pose = Some(pose);
        }
        return false;
    }

    let baseline_pose = state.baseline_pose.unwrap_or(pose);
    if !state.latched && pose.axis != baseline_pose.axis {
        if update_baseline_candidate(state, now, pose, FACE_BASELINE_RECALIBRATE_MS) {
            state.baseline_pose = Some(pose);
        }
        state.face_down_since = None;
        state.rearm_since = None;
        return false;
    }
    clear_baseline_candidate(state);

    let is_face_down = pose.axis == baseline_pose.axis && pose.sign == -baseline_pose.sign;
    if is_face_down {
        state.rearm_since = None;
        if state.latched {
            return false;
        }
        let since = state.face_down_since.get_or_insert(now);
        if now.saturating_duration_since(*since).as_millis() >= FACE_DOWN_HOLD_MS {
            state.latched = true;
            state.face_down_since = None;
            return true;
        }
        return false;
    }

    state.face_down_since = None;
    if state.latched {
        let since = state.rearm_since.get_or_insert(now);
        if now.saturating_duration_since(*since).as_millis() >= FACE_DOWN_REARM_MS {
            state.latched = false;
            state.rearm_since = None;
            state.baseline_pose = Some(pose);
        }
    } else {
        state.rearm_since = None;
    }
    false
}

fn update_baseline_candidate(
    state: &mut FaceDownToggleState,
    now: Instant,
    pose: FacePose,
    hold_ms: u64,
) -> bool {
    if state.baseline_candidate != Some(pose) {
        state.baseline_candidate = Some(pose);
        state.baseline_candidate_since = Some(now);
        return false;
    }
    let Some(since) = state.baseline_candidate_since else {
        state.baseline_candidate_since = Some(now);
        return false;
    };
    if now.saturating_duration_since(since).as_millis() >= hold_ms {
        clear_baseline_candidate(state);
        return true;
    }
    false
}

fn clear_baseline_candidate(state: &mut FaceDownToggleState) {
    state.baseline_candidate = None;
    state.baseline_candidate_since = None;
}

fn detect_face_pose(ax: i32, ay: i32, az: i32) -> Option<FacePose> {
    let x = ax.abs();
    let y = ay.abs();
    let z = az.abs();
    let (axis, major, secondary) = if x >= y && x >= z {
        (0u8, x, y.max(z))
    } else if y >= x && y >= z {
        (1u8, y, x.max(z))
    } else {
        (2u8, z, x.max(y))
    };

    if major < FACE_NORMAL_MIN_ABS_AXIS || (major - secondary) < FACE_NORMAL_MIN_GAP {
        return None;
    }

    let signed = match axis {
        0 => ax,
        1 => ay,
        _ => az,
    };

    Some(FacePose {
        axis,
        sign: if signed >= 0 { 1 } else { -1 },
    })
}

pub(crate) fn trigger_backlight_cycle(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
) {
    *backlight_cycle_start = Some(Instant::now());
    apply_backlight_level(display, backlight_level, BACKLIGHT_MAX_BRIGHTNESS);
}

pub(crate) fn run_backlight_timeline(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
) {
    let Some(cycle_start) = *backlight_cycle_start else {
        return;
    };

    let elapsed_ms = Instant::now()
        .saturating_duration_since(cycle_start)
        .as_millis();
    let target_level = if elapsed_ms < BACKLIGHT_HOLD_MS {
        BACKLIGHT_MAX_BRIGHTNESS
    } else if elapsed_ms < BACKLIGHT_HOLD_MS + BACKLIGHT_FADE_MS {
        let fade_elapsed = elapsed_ms - BACKLIGHT_HOLD_MS;
        let fade_remaining = BACKLIGHT_FADE_MS.saturating_sub(fade_elapsed);
        ((BACKLIGHT_MAX_BRIGHTNESS as u64 * fade_remaining) / BACKLIGHT_FADE_MS) as u8
    } else {
        *backlight_cycle_start = None;
        0
    };

    apply_backlight_level(display, backlight_level, target_level);
}

fn apply_backlight_level(display: &mut InkplateDriver, current_level: &mut u8, next_level: u8) {
    if *current_level == next_level {
        return;
    }

    let _ = display.set_brightness(next_level);
    if next_level == 0 {
        let _ = display.frontlight_off();
    }
    *current_level = next_level;
}
