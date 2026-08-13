#[path = "../../../../../../../src/firmware/observability/recorders/helpers.rs"]
mod helpers;
#[path = "../../../../../../../src/firmware/observability/recorders/upload_net.rs"]
mod upload_net;
#[path = "../../../../../../../src/firmware/observability/recorders/wifi.rs"]
mod wifi;

#[cfg(test)]
pub(crate) use upload_net::set_upload_http_listener;
#[cfg(test)]
pub(crate) use wifi::{set_wifi_ipv4, set_wifi_link_connected};
