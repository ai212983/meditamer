// Time keeping
pub const TICKS_PER_SECOND: u64 = 1_000_000;

/// Current systimer count value
/// A tick is 1 / 1_000_000 seconds
/// This function must not be called in a critical section. Doing so may return
/// an incorrect value.
pub(crate) fn systimer_count() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

#[cfg(target_arch = "riscv32")]
pub(crate) fn time_diff(start: u64, end: u64) -> u64 {
    end.wrapping_sub(start) & 0x000f_ffff_ffff_ffff
}

#[cfg(target_arch = "xtensa")]
pub(crate) fn time_diff(start: u64, end: u64) -> u64 {
    end.wrapping_sub(start)
}

pub(crate) const fn micros_to_ticks(us: u64) -> u64 {
    us * (TICKS_PER_SECOND / 1_000_000)
}

pub(crate) const fn millis_to_ticks(ms: u64) -> u64 {
    ms * (TICKS_PER_SECOND / 1_000)
}

pub(crate) const fn ticks_to_micros(ticks: u64) -> u64 {
    ticks / (TICKS_PER_SECOND / 1_000_000)
}

pub(crate) const fn ticks_to_millis(ticks: u64) -> u64 {
    ticks / (TICKS_PER_SECOND / 1_000)
}

pub(crate) fn elapsed_time_since(start: u64) -> u64 {
    let now = systimer_count();
    time_diff(start, now)
}

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
