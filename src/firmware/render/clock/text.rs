use core::fmt::Write;

pub(super) fn format_battery_text(battery_percent: Option<u8>) -> heapless::String<16> {
    let mut out = heapless::String::<16>::new();
    if let Some(percent) = battery_percent {
        let _ = write!(&mut out, "BAT {percent:>3}%");
    } else {
        let _ = write!(&mut out, "BAT --%");
    }
    out
}
