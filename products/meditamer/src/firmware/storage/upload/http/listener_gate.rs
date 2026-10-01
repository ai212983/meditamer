use embassy_net::Stack;
use embassy_time::Instant;
use netstack::telemetry::NetPipelineGate;

pub(super) fn dhcp_ipv4_status(stack: &Stack<'_>) -> Result<[u8; 4], NetPipelineGate> {
    // Use Wi-Fi task connectivity + non-zero DHCP lease as the listener gate.
    // `stack.is_link_up()` can transiently lag reconnect state and block listener
    // arming even when connect+lease have already recovered.
    if !netstack::telemetry::wifi_link_connected() {
        return Err(NetPipelineGate::WifiDown);
    }
    if let Some(ip) = stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
    {
        return Ok(ip);
    }
    if !stack.is_link_up() {
        return Err(NetPipelineGate::LinkDown);
    }
    Err(NetPipelineGate::NoIpv4)
}

pub(super) fn net_pipeline_gate_reason_str(reason: NetPipelineGate) -> &'static str {
    match reason {
        NetPipelineGate::WifiDown => "wifi_down",
        NetPipelineGate::LinkDown => "link_down",
        NetPipelineGate::NoIpv4 => "no_ipv4",
    }
}

pub(super) fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
