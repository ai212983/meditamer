use super::super::config::{
    FACE_BASELINE_HOLD_MS, FACE_BASELINE_RECALIBRATE_MS, FACE_DOWN_HOLD_MS, FACE_DOWN_REARM_MS,
    FACE_NORMAL_MIN_ABS_AXIS, FACE_NORMAL_MIN_GAP,
};

#[derive(Clone, Copy, Default)]
pub(super) struct FaceDownToggleState {
    baseline_pose: Option<FacePose>,
    baseline_candidate: Option<FacePose>,
    baseline_candidate_since_ms: Option<u64>,
    face_down_since_ms: Option<u64>,
    rearm_since_ms: Option<u64>,
    latched: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FacePose {
    axis: u8,
    sign: i8,
}

pub(super) fn update_face_down_toggle(
    state: &mut FaceDownToggleState,
    now_ms: u64,
    ax: i16,
    ay: i16,
    az: i16,
) -> bool {
    let Some(pose) = detect_face_pose(ax as i32, ay as i32, az as i32) else {
        state.face_down_since_ms = None;
        state.rearm_since_ms = None;
        return false;
    };

    if state.baseline_pose.is_none() {
        if update_baseline_candidate(state, now_ms, pose, FACE_BASELINE_HOLD_MS) {
            state.baseline_pose = Some(pose);
        }
        return false;
    }

    let baseline_pose = state.baseline_pose.unwrap_or(pose);
    if !state.latched && pose.axis != baseline_pose.axis {
        if update_baseline_candidate(state, now_ms, pose, FACE_BASELINE_RECALIBRATE_MS) {
            state.baseline_pose = Some(pose);
        }
        state.face_down_since_ms = None;
        state.rearm_since_ms = None;
        return false;
    }
    clear_baseline_candidate(state);

    let is_face_down = pose.axis == baseline_pose.axis && pose.sign == -baseline_pose.sign;
    if is_face_down {
        state.rearm_since_ms = None;
        if state.latched {
            return false;
        }
        let since = state.face_down_since_ms.get_or_insert(now_ms);
        if now_ms.saturating_sub(*since) >= FACE_DOWN_HOLD_MS {
            state.latched = true;
            state.face_down_since_ms = None;
            return true;
        }
        return false;
    }

    state.face_down_since_ms = None;
    if state.latched {
        let since = state.rearm_since_ms.get_or_insert(now_ms);
        if now_ms.saturating_sub(*since) >= FACE_DOWN_REARM_MS {
            state.latched = false;
            state.rearm_since_ms = None;
            state.baseline_pose = Some(pose);
        }
    } else {
        state.rearm_since_ms = None;
    }
    false
}

fn update_baseline_candidate(
    state: &mut FaceDownToggleState,
    now_ms: u64,
    pose: FacePose,
    hold_ms: u64,
) -> bool {
    if state.baseline_candidate != Some(pose) {
        state.baseline_candidate = Some(pose);
        state.baseline_candidate_since_ms = Some(now_ms);
        return false;
    }
    let Some(since) = state.baseline_candidate_since_ms else {
        state.baseline_candidate_since_ms = Some(now_ms);
        return false;
    };
    if now_ms.saturating_sub(since) >= hold_ms {
        clear_baseline_candidate(state);
        return true;
    }
    false
}

fn clear_baseline_candidate(state: &mut FaceDownToggleState) {
    state.baseline_candidate = None;
    state.baseline_candidate_since_ms = None;
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

    if major < FACE_NORMAL_MIN_ABS_AXIS || major - secondary < FACE_NORMAL_MIN_GAP {
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
