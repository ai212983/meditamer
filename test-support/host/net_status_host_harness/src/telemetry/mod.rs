//! Shims `platform/connectivity/netstack`'s telemetry module directly (product/target
//! axis completion plan, Phase 4: this coverage moved out of the product's
//! `observability` module when the network-owned counters/recorders/
//! snapshot did). Flat, mirroring netstack's own `telemetry/` layout exactly
//! so the shimmed files' own `super::` imports resolve unchanged.

#[path = "../../../../../platform/connectivity/netstack/src/telemetry/counters.rs"]
mod counters;
#[path = "../../../../../platform/connectivity/netstack/src/telemetry/helpers.rs"]
mod helpers;
#[path = "../../../../../platform/connectivity/netstack/src/telemetry/listener.rs"]
mod listener;
#[path = "../../../../../platform/connectivity/netstack/src/telemetry/snapshot.rs"]
mod snapshot;
#[path = "../../../../../platform/connectivity/netstack/src/telemetry/types.rs"]
mod types;
#[path = "../../../../../platform/connectivity/netstack/src/telemetry/wifi.rs"]
mod wifi;

#[cfg(test)]
pub(crate) use listener::set_upload_http_listener;
#[cfg(test)]
pub(crate) use snapshot::snapshot;
#[cfg(test)]
pub(crate) use wifi::{set_wifi_ipv4, set_wifi_link_connected};
