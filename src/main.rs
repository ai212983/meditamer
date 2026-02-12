use anyhow::{anyhow, Result};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
use esp_idf_sys as sys;
use meditamer::Inkplate;
use std::{thread, time::Duration};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

const SCREEN_WIDTH: i32 = 600;
const REFRESH_SECONDS: i64 = 5 * 60;
const FRONTLIGHT_PULSE_STEPS: u8 = 10;
const FRONTLIGHT_PULSE_STEP_MS: u64 = 50;
const FRONTLIGHT_PULSE_MAX: u8 = 40;
const TITLE_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB18_tf>();
const TIME_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_logisoso58_tf>();
const DATE_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();

fn main() {
    sys::link_patches();
    reduce_log_noise();
    if let Err(err) = run() {
        eprintln!("Fatal error: {err:?}");
    }
}

fn reduce_log_noise() {
    unsafe {
        // GPIO driver prints an info line per configured pin; keep warnings/errors only.
        sys::esp_log_level_set(
            b"gpio\0".as_ptr() as *const core::ffi::c_char,
            sys::esp_log_level_t_ESP_LOG_WARN,
        );
    }
}

fn run() -> Result<()> {
    println!("I: Starting");

    let mut inkplate = Inkplate::new()?;
    inkplate.init()?;
    println!("I: Inkplate init complete");

    match pulse_frontlight_once(&mut inkplate) {
        Ok(()) => println!("I: Frontlight pulse complete"),
        Err(err) => eprintln!("W: Frontlight pulse unavailable: {err:?}"),
    }

    println!("I: Clock started, refreshing every 5 minutes (bitmap font)");

    loop {
        if let Err(err) = inkplate.frontlight_off() {
            eprintln!("W: Frontlight off failed: {err:?}");
        }

        let (time_label, date_label, now) = current_time_labels();
        println!("I: Rendering {date_label} {time_label}");

        draw_clock_screen(&mut inkplate, &time_label, &date_label)?;
        inkplate.display_bw(false)?;

        let sleep_for = seconds_to_next_refresh(now);
        println!("I: Next refresh in {sleep_for}s");
        thread::sleep(Duration::from_secs(sleep_for));
    }
}

fn pulse_frontlight_once(inkplate: &mut Inkplate) -> Result<()> {
    let step_delay = Duration::from_millis(FRONTLIGHT_PULSE_STEP_MS);

    for step in 1..=FRONTLIGHT_PULSE_STEPS {
        let level = ((u16::from(step) * u16::from(FRONTLIGHT_PULSE_MAX))
            / u16::from(FRONTLIGHT_PULSE_STEPS)) as u8;
        inkplate.set_brightness(level)?;
        thread::sleep(step_delay);
    }

    for step in (0..FRONTLIGHT_PULSE_STEPS).rev() {
        let level = ((u16::from(step) * u16::from(FRONTLIGHT_PULSE_MAX))
            / u16::from(FRONTLIGHT_PULSE_STEPS)) as u8;
        inkplate.set_brightness(level)?;
        thread::sleep(step_delay);
    }

    inkplate.frontlight_off()
}

fn draw_clock_screen(inkplate: &mut Inkplate, time_text: &str, date_text: &str) -> Result<()> {
    inkplate.clear(BinaryColor::Off).ok();

    draw_centered_bitmap_text(inkplate, &TITLE_FONT, "Meditamer", 120)?;
    draw_centered_bitmap_text(inkplate, &TIME_FONT, time_text, 300)?;
    draw_centered_bitmap_text(inkplate, &DATE_FONT, date_text, 430)?;
    Ok(())
}

fn draw_centered_bitmap_text(
    inkplate: &mut Inkplate,
    renderer: &FontRenderer,
    text: &str,
    center_y: i32,
) -> Result<()> {
    renderer
        .render_aligned(
            text,
            Point::new(SCREEN_WIDTH / 2, center_y),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            FontColor::Transparent(BinaryColor::On),
            inkplate,
        )
        .map_err(|err| anyhow!("bitmap font render failed: {err:?}"))?;
    Ok(())
}

fn current_time_labels() -> (String, String, i64) {
    let mut now = 0_i64;
    unsafe {
        sys::time(&mut now as *mut _);
    }

    let mut local_tm = unsafe { core::mem::zeroed::<sys::tm>() };
    let local_tm_ptr = unsafe { sys::localtime_r(&now as *const _, &mut local_tm as *mut _) };
    if local_tm_ptr.is_null() {
        return ("--:--".to_owned(), "Clock unavailable".to_owned(), now);
    }

    let hour = local_tm.tm_hour;
    let minute = local_tm.tm_min;
    let day = local_tm.tm_mday;
    let month = local_tm.tm_mon;
    let year = local_tm.tm_year + 1900;
    let weekday = local_tm.tm_wday;

    let weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let day_name = weekdays.get(weekday as usize).copied().unwrap_or("Day");
    let month_name = months.get(month as usize).copied().unwrap_or("Mon");

    (
        format!("{hour:02}:{minute:02}"),
        format!("{day_name}, {month_name} {day:02} {year}"),
        now,
    )
}

fn seconds_to_next_refresh(now: i64) -> u64 {
    if now <= 0 {
        return REFRESH_SECONDS as u64;
    }

    let remainder = now.rem_euclid(REFRESH_SECONDS);
    let wait = if remainder == 0 {
        REFRESH_SECONDS
    } else {
        REFRESH_SECONDS - remainder
    };
    wait as u64
}
