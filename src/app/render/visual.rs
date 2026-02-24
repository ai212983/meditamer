use core::sync::atomic::Ordering;

use embassy_time::{Instant, Timer};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
use meditamer::{
    shanshui,
    sumi_sun::{self, Fx as SunFx, SumiSunParams},
    suminagashi,
};

use super::{
    super::{
        config::{
            LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS, REFRESH_INTERVAL_SECONDS,
            RENDER_TIME_FONT, SUMINAGASHI_BG_ALPHA_50_THRESHOLD, SUMINAGASHI_CHUNK_ROWS,
            SUMINAGASHI_DITHER_MODE, SUMINAGASHI_ENABLE_SUN, SUMINAGASHI_RENDER_MODE,
            SUMINAGASHI_RGSS_MODE, SUMINAGASHI_SUN_ONLY, SUNRISE_SECONDS_OF_DAY,
            SUNSET_SECONDS_OF_DAY, SUN_FORCE_CENTER, SUN_RENDER_TIME_Y_OFFSET,
            SUN_TARGET_DIAMETER_PX, VISUAL_DEFAULT_SEED,
        },
        types::{InkplateDriver, TimeSyncState},
    },
    clock::{draw_centered_bitmap_text_with_white_rim, format_render_time_text, render_time_font},
    local_seconds_since_epoch,
};

pub(crate) fn next_visual_seed(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
) -> u32 {
    if *first_visual_seed_pending {
        *first_visual_seed_pending = false;
        return VISUAL_DEFAULT_SEED;
    }

    *pattern_nonce = pattern_nonce.wrapping_add(1);
    pattern_seed(uptime_seconds, time_sync, *pattern_nonce)
}

pub(crate) async fn render_suminagashi_update(
    display: &mut InkplateDriver,
    seed: u32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) {
    let started = Instant::now();
    let width = display.width() as i32;
    let height = display.height() as i32;
    let scene = suminagashi::build_seeded_scene(seed, Size::new(width as u32, height as u32));
    let render_time_text = format_render_time_text(uptime_seconds, time_sync);
    let sun_params = if SUMINAGASHI_ENABLE_SUN {
        Some(build_sun_params(
            seed,
            sun_center_for_time(width, height, uptime_seconds, time_sync),
        ))
    } else {
        None
    };

    let _ = display.clear(BinaryColor::Off);

    let mut y = 0i32;
    while y < height {
        let y_end = (y + SUMINAGASHI_CHUNK_ROWS).min(height);
        if !SUMINAGASHI_SUN_ONLY {
            suminagashi::render_scene_rows_bw_masked(
                &scene,
                width,
                y..y_end,
                suminagashi::SceneRenderStyle {
                    rgss: SUMINAGASHI_RGSS_MODE,
                    mode: SUMINAGASHI_RENDER_MODE,
                    dither: SUMINAGASHI_DITHER_MODE,
                },
                |x, py| background_alpha_50_mask(x, py, seed),
                |x, py| display.set_pixel_bw(x as usize, py as usize, true),
            );
        }
        if let Some(sun_params) = sun_params {
            sumi_sun::render_sumi_sun_rows_bw(
                width,
                height,
                y..y_end,
                sun_params,
                SUMINAGASHI_RENDER_MODE,
                SUMINAGASHI_DITHER_MODE,
                |x, py| display.set_pixel_bw(x as usize, py as usize, true),
            );
        }
        y = y_end;
        if y < height {
            Timer::after_millis(1).await;
        }
    }

    draw_centered_bitmap_text_with_white_rim(
        display,
        &RENDER_TIME_FONT,
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw_async(false).await;
    update_marble_metrics(started);
}

pub(crate) async fn render_shanshui_update(
    display: &mut InkplateDriver,
    seed: u32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) {
    let started = Instant::now();
    let width = display.width() as i32;
    let height = display.height() as i32;
    let render_time_text = format_render_time_text(uptime_seconds, time_sync);

    let _ = display.clear(BinaryColor::Off);

    shanshui::render_shanshui_bw_atkinson(width, height, seed, |x, py| {
        display.set_pixel_bw(x as usize, py as usize, true)
    });

    draw_centered_bitmap_text_with_white_rim(
        display,
        render_time_font(),
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw_async(false).await;
    update_marble_metrics(started);
}

fn update_marble_metrics(started: Instant) {
    let elapsed_ms = Instant::now()
        .saturating_duration_since(started)
        .as_millis()
        .min(u32::MAX as u64) as u32;
    LAST_MARBLE_REDRAW_MS.store(elapsed_ms, Ordering::Relaxed);

    let mut current_max = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
    while elapsed_ms > current_max {
        match MAX_MARBLE_REDRAW_MS.compare_exchange_weak(
            current_max,
            elapsed_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(seen) => current_max = seen,
        }
    }
}

fn pattern_seed(uptime_seconds: u32, time_sync: Option<TimeSyncState>, nonce: u32) -> u32 {
    let local_now = local_seconds_since_epoch(uptime_seconds, time_sync);
    let refresh_step = (local_now / REFRESH_INTERVAL_SECONDS as u64) as u32;
    refresh_step ^ refresh_step.rotate_left(13) ^ nonce.wrapping_mul(0x85EB_CA6B) ^ 0x9E37_79B9
}

fn background_alpha_50_mask(x: i32, y: i32, seed: u32) -> bool {
    let mixed =
        mix32(seed ^ (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B));
    (mixed as u8) < SUMINAGASHI_BG_ALPHA_50_THRESHOLD
}

fn sun_center_for_time(
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

fn build_sun_params(seed: u32, center: Point) -> SumiSunParams {
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

fn rand_i32(state: &mut u32, min: i32, max: i32) -> i32 {
    if min >= max {
        return min;
    }
    let span = (max - min + 1) as u32;
    min + (next_rand_u32(state) % span) as i32
}

fn next_rand_u32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn mix32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}
