use super::{TouchPoint, TouchSample};

pub(super) fn sample_primary(sample: &TouchSample) -> (u8, Option<TouchPoint>) {
    if sample.touch_count == 0 {
        (0, None)
    } else {
        (sample.touch_count, Some(sample.points[0]))
    }
}

pub(super) fn squared_distance(a: TouchPoint, b: TouchPoint) -> i32 {
    let dx = a.x as i32 - b.x as i32;
    let dy = a.y as i32 - b.y as i32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

pub(super) fn squared_i32(value: i32) -> i32 {
    value.saturating_mul(value)
}

pub(super) fn is_axis_dominant(dx: i32, dy: i32, ratio_x100: i32) -> bool {
    let ax = dx.abs();
    let ay = dy.abs();
    let major = ax.max(ay);
    let minor = ax.min(ay);
    major > 0 && major.saturating_mul(100) >= minor.saturating_mul(ratio_x100)
}

pub(super) fn int_sqrt_i32(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let mut lo = 0i32;
    let mut hi = value.min(46_340) + 1;
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if mid.saturating_mul(mid) <= value {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}
