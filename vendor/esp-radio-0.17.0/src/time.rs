//! Time conversions
//!
//! We're using 1ms per tick, to offer a decent-ish timeout range on u32.

#![allow(unused)]

const fn blob_tick_millis() -> u32 {
    if matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_BLOB_TICK_10MS"),
        Some(_)
    ) || matches!(option_env!("ESP_RADIO_BLOB_TICK_10MS"), Some(_)) {
        10
    } else {
        1
    }
}

pub(crate) const fn blob_ticks_to_micros(ticks: u32) -> u32 {
    ticks.saturating_mul(blob_tick_millis()).saturating_mul(1_000)
}

pub(crate) const fn micros_to_blob_ticks(micros: u32) -> u32 {
    let tick_micros = blob_tick_millis().saturating_mul(1_000);
    if tick_micros == 0 {
        0
    } else {
        micros / tick_micros
    }
}

pub(crate) const fn blob_ticks_to_millis(ticks: u32) -> u32 {
    ticks.saturating_mul(blob_tick_millis())
}

pub(crate) const fn millis_to_blob_ticks(millis: u32) -> u32 {
    let tick_ms = blob_tick_millis();
    if tick_ms <= 1 {
        millis
    } else {
        millis.saturating_add(tick_ms - 1) / tick_ms
    }
}
