//! Bind prepared clock pixels to the shared LVGL draw unit once per boot.
//!
//! Unit metadata uses the existing LVGL PSRAM arena. Its checked handle also
//! lives in fallible PSRAM storage; only one reference stays in SharedCache.
//! Numerical preparation and minute scheduling remain in the clock engine.

use render::lvgl_adapter::{PreparedL8DrawUnit, UiAccessToken};

use super::render::{SharedCache, CLOCK_HEIGHT, CLOCK_WIDTH};
use crate::firmware::psram::ExternalValue;

pub(super) fn configure(token: &UiAccessToken, shared: &mut SharedCache) -> bool {
    let Some(canvas) = shared.canvas else {
        return false;
    };
    if shared.draw_unit.is_none() {
        // Reserve the checked handle before registering LVGL-owned metadata.
        let keeper: ExternalValue<Option<PreparedL8DrawUnit>> =
            match ExternalValue::try_new_with(|| None) {
                Ok(keeper) => keeper,
                Err(_) => {
                    console::println!("CLOCK_DRAW_UNIT status=handle_oom");
                    return false;
                }
            };
        let Some(unit) = PreparedL8DrawUnit::register(token) else {
            console::println!("CLOCK_DRAW_UNIT status=register_failed");
            return false;
        };
        let slot = keeper.leak();
        *slot = Some(unit);
        shared.draw_unit = slot.as_ref();
        console::println!("CLOCK_DRAW_UNIT status=registered");
    }
    let bound = shared
        .draw_unit
        .is_some_and(|unit| unit.set_source(canvas, CLOCK_WIDTH as usize, CLOCK_HEIGHT as usize));
    if !bound {
        console::println!("CLOCK_DRAW_UNIT status=bind_failed");
    }
    bound
}

pub(super) fn begin(shared: &SharedCache) {
    if let Some(unit) = shared.draw_unit {
        unit.reset_telemetry();
    }
}

pub(super) fn report(shared: &SharedCache, elapsed_us: u64) {
    let Some(stats) = shared.draw_unit.and_then(PreparedL8DrawUnit::telemetry) else {
        console::println!("CLOCK_DRAW_UNIT phase=publish status=unavailable");
        return;
    };
    console::println!(
        "CLOCK_DRAW_UNIT phase=publish accepted={} completed={} pixels={} idle={} fallbacks={} lvgl_publish_us={}",
        stats.accepted,
        stats.completed,
        stats.pixels,
        stats.idle_polls,
        stats.fallbacks,
        elapsed_us,
    );
}
