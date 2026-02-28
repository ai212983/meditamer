use core::fmt::Write;

mod text;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use u8g2_fonts::{
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

use super::{
    super::{
        config::{
            BATTERY_FONT, BATTERY_TEXT_RIGHT_X, BATTERY_TEXT_Y, RENDER_TIME_FONT, SCREEN_WIDTH,
        },
        psram,
        types::{InkplateDriver, TimeSyncState},
    },
    local_seconds_since_epoch,
};
use text::format_battery_text;

pub(crate) async fn render_clock_overlay(
    display: &mut InkplateDriver,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
) {
    clear_region(display, 0, 0, SCREEN_WIDTH as u32, 68);
    let clock_text = format_overlay_clock_text(uptime_seconds, time_sync);
    let battery_text = format_battery_text(battery_percent);
    let _ = BATTERY_FONT.render_aligned(
        clock_text.as_str(),
        Point::new(24, BATTERY_TEXT_Y),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
    draw_right_aligned_bitmap_text(
        display,
        &BATTERY_FONT,
        battery_text.as_str(),
        BATTERY_TEXT_RIGHT_X,
        BATTERY_TEXT_Y,
    );
    let _ = display.display_bw_partial_async(false).await;
    psram::log_allocator_high_water("render_clock_overlay_partial");
}

pub(crate) fn sample_battery_percent(display: &mut InkplateDriver) -> Option<u8> {
    let soc = display.fuel_gauge_soc().ok()?;
    if soc > 100 {
        return None;
    }
    Some(soc as u8)
}

pub(crate) fn format_render_time_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<24> {
    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as u32;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;
    let seconds = seconds_of_day % 60;
    let mut out = heapless::String::<24>::new();
    let _ = write!(&mut out, "RENDER {hours:02}:{minutes:02}:{seconds:02}");
    out
}

pub(crate) fn draw_centered_bitmap_text_with_white_rim<T>(
    display: &mut T,
    renderer: &FontRenderer,
    text: &str,
    center_y: i32,
    rim_px: i32,
) where
    T: DrawTarget<Color = BinaryColor>,
{
    let cx = SCREEN_WIDTH / 2;
    let mut dy = -rim_px;
    while dy <= rim_px {
        let mut dx = -rim_px;
        while dx <= rim_px {
            if dx != 0 || dy != 0 {
                let _ = renderer.render_aligned(
                    text,
                    Point::new(cx + dx, center_y + dy),
                    VerticalPosition::Center,
                    HorizontalAlignment::Center,
                    FontColor::Transparent(BinaryColor::Off),
                    display,
                );
            }
            dx += 1;
        }
        dy += 1;
    }

    let _ = renderer.render_aligned(
        text,
        Point::new(cx, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

pub(crate) fn render_time_font() -> &'static FontRenderer {
    &RENDER_TIME_FONT
}

fn draw_right_aligned_bitmap_text<T>(
    display: &mut T,
    renderer: &FontRenderer,
    text: &str,
    right_x: i32,
    center_y: i32,
) where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = renderer.render_aligned(
        text,
        Point::new(right_x, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Right,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn clear_region<T>(display: &mut T, x: i32, y: i32, width: u32, height: u32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let style = PrimitiveStyle::with_fill(BinaryColor::Off);
    let _ = Rectangle::new(Point::new(x, y), Size::new(width, height))
        .into_styled(style)
        .draw(display);
}

fn format_overlay_clock_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<16> {
    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as u32;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;
    let mut out = heapless::String::<16>::new();
    let _ = write!(&mut out, "{hours:02}:{minutes:02}");
    out
}
