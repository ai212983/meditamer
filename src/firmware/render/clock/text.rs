use core::fmt::Write;

use super::*;

pub(super) fn format_clock_text(
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

pub(super) fn format_uptime_text(uptime_seconds: u32) -> heapless::String<32> {
    let days = uptime_seconds / 86_400;
    let hours = (uptime_seconds / 3_600) % 24;
    let minutes = (uptime_seconds / 60) % 60;
    let mut out = heapless::String::<32>::new();
    let _ = write!(&mut out, "UPTIME {days}d {hours:02}h {minutes:02}m");
    out
}

pub(super) fn format_sync_text(time_sync: Option<TimeSyncState>) -> heapless::String<32> {
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

pub(super) fn format_battery_text(battery_percent: Option<u8>) -> heapless::String<16> {
    let mut out = heapless::String::<16>::new();
    if let Some(percent) = battery_percent {
        let _ = write!(&mut out, "BAT {percent:>3}%");
    } else {
        let _ = write!(&mut out, "BAT --%");
    }
    out
}
