use super::*;

pub(super) fn pattern_seed(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    nonce: u32,
) -> u32 {
    let local_now = local_seconds_since_epoch(uptime_seconds, time_sync);
    let refresh_step = (local_now / REFRESH_INTERVAL_SECONDS as u64) as u32;
    refresh_step ^ refresh_step.rotate_left(13) ^ nonce.wrapping_mul(0x85EB_CA6B) ^ 0x9E37_79B9
}

pub(super) fn background_alpha_50_mask(x: i32, y: i32, seed: u32) -> bool {
    let mixed =
        mix32(seed ^ (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B));
    (mixed as u8) < SUMINAGASHI_BG_ALPHA_50_THRESHOLD
}

pub(super) fn sun_center_for_time(
    width: i32,
    height: i32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> Point {
    if SUN_FORCE_CENTER {
        return Point::new(width / 2, height / 2);
    }

    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as i64;
    let margin = (width / 12).clamp(24, 72);
    let left_x = margin;
    let right_x = (width - 1 - margin).max(left_x + 1);
    let horizon_y = (height * 83 / 100).clamp(0, height - 1);
    let arc_height = (height * 50 / 100).clamp(1, height - 1);
    let below_horizon_y = (horizon_y + height / 12).clamp(0, height - 1);

    let (x, y) = if seconds_of_day < SUNRISE_SECONDS_OF_DAY {
        (left_x, below_horizon_y)
    } else if seconds_of_day > SUNSET_SECONDS_OF_DAY {
        (right_x, below_horizon_y)
    } else {
        let day_span = (SUNSET_SECONDS_OF_DAY - SUNRISE_SECONDS_OF_DAY).max(1);
        let t = (seconds_of_day - SUNRISE_SECONDS_OF_DAY).clamp(0, day_span);
        let x = left_x + (((right_x - left_x) as i64 * t) / day_span) as i32;

        let u = t * 2 - day_span;
        let denom_sq = day_span * day_span;
        let profile = (denom_sq - u * u).max(0);
        let lift = ((arc_height as i64 * profile) / denom_sq) as i32;
        let y = (horizon_y - lift).clamp(0, height - 1);
        (x, y)
    };

    Point::new(x, y)
}

pub(super) fn build_sun_params(seed: u32, center: Point) -> SumiSunParams {
    let mut state = mix32(seed ^ 0xA1C3_4D27);
    SumiSunParams {
        center,
        radius_px: ((SUN_TARGET_DIAMETER_PX / 2) + rand_i32(&mut state, -3, 3)).max(10),
        edge_softness_px: SunFx::from_bits(rand_i32(&mut state, 45_875, 98_304)),
        bleed_px: SunFx::from_bits(rand_i32(&mut state, 19_661, 98_304)),
        dry_brush: SunFx::from_bits(rand_i32(&mut state, 9_000, 26_000)),
        completeness: SunFx::from_bits(65_536),
        completeness_softness: SunFx::from_bits(rand_i32(&mut state, 600, 1_800)),
        completeness_warp: SunFx::from_bits(rand_i32(&mut state, 0, 600)),
        completeness_rotation: SunFx::from_bits(rand_i32(&mut state, 0, 65_535)),
        stroke_strength: SunFx::from_bits(rand_i32(&mut state, 24_000, 56_000)),
        stroke_anisotropy: SunFx::from_bits(rand_i32(&mut state, 65_536, 196_608)),
        ink_luma: SunFx::from_bits(rand_i32(&mut state, 0, 30_000)),
    }
}

pub(super) fn rand_i32(state: &mut u32, min: i32, max: i32) -> i32 {
    if min >= max {
        return min;
    }
    let span = (max - min + 1) as u32;
    min + (next_rand_u32(state) % span) as i32
}

pub(super) fn next_rand_u32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

pub(super) fn mix32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}
