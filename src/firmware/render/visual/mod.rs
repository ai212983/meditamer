use core::sync::atomic::Ordering;

mod helpers;

use super::super::graphics::{
    shanshui,
    sumi_sun::{self, Fx as SunFx, SumiSunParams},
    suminagashi,
};
use embassy_time::{Instant, Timer};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};

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
        psram,
        types::{InkplateDriver, TimeSyncState},
    },
    clock::{draw_centered_bitmap_text_with_white_rim, format_render_time_text, render_time_font},
    local_seconds_since_epoch,
};
use helpers::*;

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

    #[cfg(feature = "psram-alloc")]
    let used_staging =
        render_suminagashi_with_staging(display, &scene, width, height, seed, sun_params).await;
    #[cfg(not(feature = "psram-alloc"))]
    let used_staging = false;

    if !used_staging {
        render_suminagashi_direct(display, &scene, width, height, seed, sun_params).await;
    }

    draw_centered_bitmap_text_with_white_rim(
        display,
        &RENDER_TIME_FONT,
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw_async(false).await;
    update_marble_metrics(started, "render_suminagashi");
}

async fn render_suminagashi_direct(
    display: &mut InkplateDriver,
    scene: &suminagashi::MarblingScene,
    width: i32,
    height: i32,
    seed: u32,
    sun_params: Option<SumiSunParams>,
) {
    let mut y = 0i32;
    while y < height {
        let y_end = (y + SUMINAGASHI_CHUNK_ROWS).min(height);
        if !SUMINAGASHI_SUN_ONLY {
            suminagashi::render_scene_rows_bw_masked(
                scene,
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
}

#[cfg(feature = "psram-alloc")]
async fn render_suminagashi_with_staging(
    display: &mut InkplateDriver,
    scene: &suminagashi::MarblingScene,
    width: i32,
    height: i32,
    seed: u32,
    sun_params: Option<SumiSunParams>,
) -> bool {
    if width <= 0 || height <= 0 {
        return true;
    }

    let width_usize = width as usize;
    let chunk_rows = SUMINAGASHI_CHUNK_ROWS.max(1) as usize;
    let stage_len = width_usize.saturating_mul(chunk_rows);
    if stage_len == 0 {
        return false;
    }

    let mut stage = match psram::alloc_large_byte_buffer(stage_len) {
        Ok(buffer) => buffer,
        Err(_) => return false,
    };
    let stage_buf = stage.as_mut_slice();

    let mut y = 0i32;
    while y < height {
        let y_end = (y + SUMINAGASHI_CHUNK_ROWS).min(height);
        let chunk_rows_active = (y_end - y) as usize;
        let active_len = chunk_rows_active.saturating_mul(width_usize);
        stage_buf[..active_len].fill(0);

        if !SUMINAGASHI_SUN_ONLY {
            suminagashi::render_scene_rows_bw_masked(
                scene,
                width,
                y..y_end,
                suminagashi::SceneRenderStyle {
                    rgss: SUMINAGASHI_RGSS_MODE,
                    mode: SUMINAGASHI_RENDER_MODE,
                    dither: SUMINAGASHI_DITHER_MODE,
                },
                |x, py| background_alpha_50_mask(x, py, seed),
                |x, py| {
                    let row = (py - y) as usize;
                    let idx = row * width_usize + x as usize;
                    stage_buf[idx] = 1;
                },
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
                |x, py| {
                    let row = (py - y) as usize;
                    let idx = row * width_usize + x as usize;
                    stage_buf[idx] = 1;
                },
            );
        }

        let mut row = 0usize;
        while row < chunk_rows_active {
            let py = y as usize + row;
            let row_start = row * width_usize;
            let row_slice = &stage_buf[row_start..row_start + width_usize];
            let mut x = 0usize;
            while x < row_slice.len() {
                if row_slice[x] != 0 {
                    display.set_pixel_bw(x, py, true);
                }
                x += 1;
            }
            row += 1;
        }

        y = y_end;
        if y < height {
            Timer::after_millis(1).await;
        }
    }

    psram::log_allocator_high_water("render_suminagashi_staging");
    true
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

    #[cfg(feature = "psram-alloc")]
    let used_staging = render_shanshui_with_staging(display, width, height, seed).await;
    #[cfg(not(feature = "psram-alloc"))]
    let used_staging = false;
    if !used_staging {
        render_shanshui_direct(display, width, height, seed);
    }

    draw_centered_bitmap_text_with_white_rim(
        display,
        render_time_font(),
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw_async(false).await;
    update_marble_metrics(started, "render_shanshui");
}

fn render_shanshui_direct(display: &mut InkplateDriver, width: i32, height: i32, seed: u32) {
    shanshui::render_shanshui_bw_atkinson(width, height, seed, |x, py| {
        display.set_pixel_bw(x as usize, py as usize, true)
    });
}

#[cfg(feature = "psram-alloc")]
async fn render_shanshui_with_staging(
    display: &mut InkplateDriver,
    width: i32,
    height: i32,
    seed: u32,
) -> bool {
    if width <= 0 || height <= 0 {
        return true;
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let stage_len = width_usize.saturating_mul(height_usize);
    if stage_len == 0 {
        return false;
    }

    let mut stage = match psram::alloc_large_byte_buffer(stage_len) {
        Ok(buffer) => buffer,
        Err(_) => return false,
    };
    let stage_buf = stage.as_mut_slice();
    stage_buf.fill(0);

    shanshui::render_shanshui_bw_atkinson(width, height, seed, |x, py| {
        let idx = py as usize * width_usize + x as usize;
        stage_buf[idx] = 1;
    });

    let mut py = 0usize;
    while py < height_usize {
        let row_start = py * width_usize;
        let row = &stage_buf[row_start..row_start + width_usize];
        let mut x = 0usize;
        while x < row.len() {
            if row[x] != 0 {
                display.set_pixel_bw(x, py, true);
            }
            x += 1;
        }
        py += 1;
    }

    psram::log_allocator_high_water("render_shanshui_staging");
    true
}

fn update_marble_metrics(started: Instant, tag: &str) {
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

    psram::log_allocator_high_water(tag);
}
