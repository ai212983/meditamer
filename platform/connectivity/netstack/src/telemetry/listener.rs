//! Listener and network-pipeline recorders.
//!
//! Split out of
//! `products/meditamer/src/firmware/observability/recorders/upload_net.rs`
//! (product/target axis completion plan, Phase 4) on the real ownership
//! boundary the design names: listener bind/enable state and
//! network-pipeline counters (DHCP wait, gate reasons, accept timing) are
//! network-owned. HTTP request/body/SD-roundtrip timing -- the other half of
//! the old mixed recorder -- stayed in the product's own observability
//! module.

use core::sync::atomic::Ordering;

use super::counters::*;
use super::helpers::{saturating_add_u32, update_max_u32};
use super::types::NetPipelineGate;

pub fn set_upload_http_listener(listening: bool, ip: Option<[u8; 4]>) {
    let previous = UPLOAD_HTTP_LISTENING.swap(listening, Ordering::Relaxed);
    arbitration::claim::set_service_listening(listening);
    if listening && !previous {
        NET_PIPELINE_LISTENER_ON.fetch_add(1, Ordering::Relaxed);
    } else if !listening && previous {
        NET_PIPELINE_LISTENER_OFF.fetch_add(1, Ordering::Relaxed);
    }
    let raw_ip = ip.map(u32::from_be_bytes).unwrap_or(0);
    UPLOAD_HTTP_IPV4.store(raw_ip, Ordering::Relaxed);
}

pub fn record_net_pipeline_dhcp_wait(elapsed_ms: u32) {
    NET_PIPELINE_DHCP_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_DHCP_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_DHCP_WAIT_MS_MAX, elapsed_ms);
}

pub fn record_net_pipeline_dhcp_ready() {
    NET_PIPELINE_DHCP_READY_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn record_net_pipeline_gate(reason: NetPipelineGate) {
    match reason {
        NetPipelineGate::WifiDown => {
            NET_PIPELINE_GATE_WIFI_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::LinkDown => {
            NET_PIPELINE_GATE_LINK_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::NoIpv4 => {
            NET_PIPELINE_GATE_NO_IPV4.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_net_pipeline_accept_wait(elapsed_ms: u32) {
    NET_PIPELINE_ACCEPT_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_MAX, elapsed_ms);
}

pub fn record_net_pipeline_accept_arm_gap(elapsed_us: u32, after_mkdir: bool) {
    NET_PIPELINE_ACCEPT_ARM_GAP_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_US_TOTAL, elapsed_us);
    update_max_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_US_MAX, elapsed_us);
    if after_mkdir {
        NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_COUNT.fetch_add(1, Ordering::Relaxed);
        saturating_add_u32(
            &NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_TOTAL,
            elapsed_us,
        );
        update_max_u32(&NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_MAX, elapsed_us);
    }
}
