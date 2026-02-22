use core::fmt::Write;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
};
use u8g2_fonts::{
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

use crate::pirata_clock_font;

use super::{
    super::{
        config::{
            BATTERY_FONT, BATTERY_REGION_HEIGHT, BATTERY_REGION_LEFT, BATTERY_REGION_TOP,
            BATTERY_REGION_WIDTH, BATTERY_TEXT_RIGHT_X, BATTERY_TEXT_Y, CLOCK_REGION_HEIGHT,
            CLOCK_REGION_LEFT, CLOCK_REGION_TOP, CLOCK_REGION_WIDTH, CLOCK_Y, DIVIDER_BOTTOM_Y,
            DIVIDER_TOP_Y, META_FONT, META_REGION_HEIGHT, META_REGION_LEFT, META_REGION_TOP,
            META_REGION_WIDTH, RENDER_TIME_FONT, SCREEN_WIDTH, SYNC_Y, TITLE_FONT, TITLE_Y,
            UPTIME_Y,
        },
        types::{InkplateDriver, TimeSyncState},
    },
    local_seconds_since_epoch,
};

pub(crate) fn render_clock_update(
    display: &mut InkplateDriver,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    full_refresh: bool,
) {
    if full_refresh {
        draw_clock_static(display);
        draw_clock_dynamic(display, uptime_seconds, time_sync);
        draw_battery_status(display, battery_percent);
        let _ = display.display_bw(false);
        return;
    }

    erase_clock_dynamic_regions(display);
    draw_clock_dynamic(display, uptime_seconds, time_sync);
    let _ = display.display_bw(false);
}

pub(crate) fn render_battery_update(display: &mut InkplateDriver, battery_percent: Option<u8>) {
    erase_battery_region(display);
    draw_battery_status(display, battery_percent);
    let _ = display.display_bw_partial(false);
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

fn draw_clock_static<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = display.clear(BinaryColor::Off);
    draw_divider(display, DIVIDER_TOP_Y);
    draw_divider(display, DIVIDER_BOTTOM_Y);
    draw_centered_bitmap_text(display, &TITLE_FONT, "MEDITAMER CLOCK", TITLE_Y);
}

fn erase_clock_dynamic_regions<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    clear_region(
        display,
        CLOCK_REGION_LEFT,
        CLOCK_REGION_TOP,
        CLOCK_REGION_WIDTH,
        CLOCK_REGION_HEIGHT,
    );
    clear_region(
        display,
        META_REGION_LEFT,
        META_REGION_TOP,
        META_REGION_WIDTH,
        META_REGION_HEIGHT,
    );
}

fn erase_battery_region<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    clear_region(
        display,
        BATTERY_REGION_LEFT,
        BATTERY_REGION_TOP,
        BATTERY_REGION_WIDTH,
        BATTERY_REGION_HEIGHT,
    );
}

fn draw_clock_dynamic<T>(display: &mut T, uptime_seconds: u32, time_sync: Option<TimeSyncState>)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let clock_text = format_clock_text(uptime_seconds, time_sync);
    let uptime_text = format_uptime_text(uptime_seconds);
    let sync_text = format_sync_text(time_sync);

    pirata_clock_font::draw_time_centered(
        display,
        clock_text.as_str(),
        Point::new(SCREEN_WIDTH / 2, CLOCK_Y),
    );
    draw_centered_bitmap_text(display, &META_FONT, sync_text.as_str(), SYNC_Y);
    draw_centered_bitmap_text(display, &META_FONT, uptime_text.as_str(), UPTIME_Y);
}

fn draw_battery_status<T>(display: &mut T, battery_percent: Option<u8>)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let battery_text = format_battery_text(battery_percent);
    draw_right_aligned_bitmap_text(
        display,
        &BATTERY_FONT,
        battery_text.as_str(),
        BATTERY_TEXT_RIGHT_X,
        BATTERY_TEXT_Y,
    );
}

fn draw_centered_bitmap_text<T>(display: &mut T, renderer: &FontRenderer, text: &str, center_y: i32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = renderer.render_aligned(
        text,
        Point::new(SCREEN_WIDTH / 2, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
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

fn draw_divider<T>(display: &mut T, y: i32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = Line::new(Point::new(40, y), Point::new(SCREEN_WIDTH - 40, y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
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

fn format_clock_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<12> {
    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as u32;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;

    let mut out = heapless::String::<12>::new();
    let _ = write!(&mut out, "{hours:02}:{minutes:02}");
    out
}

fn format_uptime_text(uptime_seconds: u32) -> heapless::String<32> {
    let days = uptime_seconds / 86_400;
    let hours = (uptime_seconds / 3_600) % 24;
    let minutes = (uptime_seconds / 60) % 60;
    let mut out = heapless::String::<32>::new();
    let _ = write!(&mut out, "UPTIME {days}d {hours:02}h {minutes:02}m");
    out
}

fn format_sync_text(time_sync: Option<TimeSyncState>) -> heapless::String<32> {
    let mut out = heapless::String::<32>::new();
    if let Some(sync) = time_sync {
        let sign = if sync.tz_offset_minutes >= 0 {
            '+'
        } else {
            '-'
        };
        let abs = sync.tz_offset_minutes.unsigned_abs();
        let hours = abs / 60;
        let minutes = abs % 60;
        let _ = write!(&mut out, "SYNCED UTC{sign}{hours:02}:{minutes:02}");
    } else {
        let _ = write!(&mut out, "UNSYNCED");
    }
    out
}

fn format_battery_text(battery_percent: Option<u8>) -> heapless::String<16> {
    let mut out = heapless::String::<16>::new();
    if let Some(percent) = battery_percent {
        let _ = write!(&mut out, "BAT {percent:>3}%");
    } else {
        let _ = write!(&mut out, "BAT --%");
    }
    out
}
