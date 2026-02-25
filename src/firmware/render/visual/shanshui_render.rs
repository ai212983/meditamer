use super::*;

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
