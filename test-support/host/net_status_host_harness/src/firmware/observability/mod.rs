#[path = "../../../../../../src/firmware/observability/counters.rs"]
mod counters;
mod recorders;
#[path = "../../../../../../src/firmware/observability/snapshot.rs"]
mod snapshot_impl;
#[path = "../../../../../../src/firmware/observability/types.rs"]
mod types;

#[cfg(test)]
pub(crate) use recorders::{set_upload_http_listener, set_wifi_ipv4, set_wifi_link_connected};
#[cfg(test)]
pub(crate) use snapshot_impl::snapshot;
