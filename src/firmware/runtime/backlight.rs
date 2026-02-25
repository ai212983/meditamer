use embassy_time::Instant;

use super::super::{
    config::{BACKLIGHT_FADE_MS, BACKLIGHT_HOLD_MS, BACKLIGHT_MAX_BRIGHTNESS},
    types::InkplateDriver,
};

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
