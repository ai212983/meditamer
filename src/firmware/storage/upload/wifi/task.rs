use embassy_time::Instant;
use esp_println::println;

use super::state::NetState;

pub(super) fn emit_net_event(from: NetState, to: NetState, trigger: &str, started_at: Instant) {
    let at_ms = started_at.elapsed().as_millis() as u32;
    println!(
        "NET_EVENT {{\"from\":\"{}\",\"to\":\"{}\",\"trigger\":\"{}\",\"at_ms\":{}}}",
        from.as_str(),
        to.as_str(),
        trigger,
        at_ms
    );
}
