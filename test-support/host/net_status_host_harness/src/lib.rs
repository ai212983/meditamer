#![no_std]
#![allow(dead_code)]

#[cfg(test)]
extern crate std;

mod telemetry;

#[path = "../../../../platform/connectivity/netstack/src/wifi/state.rs"]
mod wifi_state;

#[cfg(test)]
mod tests {
    use super::telemetry::{
        set_upload_http_listener, set_wifi_ipv4, set_wifi_link_connected, snapshot,
    };
    use super::wifi_state::NetStatusSnapshot;

    fn idle_network() -> NetStatusSnapshot {
        NetStatusSnapshot {
            state: "Idle",
            link: false,
            radio_quiesced: false,
            ipv4: [0; 4],
            listener: false,
            listener_enabled: true,
            failure_class: "none",
            failure_code: 0,
            ladder_step: "retry_same",
            attempt: 0,
            uptime_ms: 0,
        }
    }

    #[test]
    fn provisioning_wait_does_not_require_a_link_or_restart() {
        let mut status = idle_network();
        for uptime_ms in [0, 180_000, 360_000] {
            status.uptime_ms = uptime_ms;
            assert!(status.owner_restoration_ready(true, false));
        }
        // Once provisioned, startup must establish the real service again.
        assert!(!status.owner_restoration_ready(true, true));
    }

    #[test]
    fn missing_credentials_do_not_hide_connection_failures() {
        let mut status = idle_network();
        for state in [
            "Starting",
            "Scanning",
            "Associating",
            "DhcpWait",
            "ListenerWait",
            "Recovering",
            "Failed",
        ] {
            status.state = state;
            assert!(!status.owner_restoration_ready(true, false), "{state}");
        }
    }

    #[test]
    fn disabled_upload_still_requires_radio_quiescence() {
        let mut status = idle_network();
        assert!(!status.owner_restoration_ready(false, false));
        status.radio_quiesced = true;
        assert!(status.owner_restoration_ready(false, true));
        status.link = true;
        assert!(!status.owner_restoration_ready(false, true));
    }

    #[test]
    fn configured_service_requires_link_lease_and_enabled_listener() {
        let mut status = idle_network();
        status.state = "Ready";
        assert!(!status.owner_restoration_ready(true, true));
        status.link = true;
        assert!(!status.owner_restoration_ready(true, true));
        status.ipv4 = [192, 168, 10, 42];
        assert!(!status.owner_restoration_ready(true, true));
        status.listener = true;
        assert!(status.owner_restoration_ready(true, true));
        status.listener = false;
        status.listener_enabled = false;
        assert!(status.owner_restoration_ready(true, true));
    }

    #[test]
    fn listener_lifecycle_does_not_own_the_wifi_lease() {
        let lease = [192, 168, 10, 42];

        set_wifi_link_connected(true);
        set_wifi_ipv4(Some(lease));
        set_upload_http_listener(true, Some(lease));

        let listening = snapshot();
        assert_eq!(listening.wifi_ipv4, Some(lease));
        assert!(listening.upload_http_listening);
        assert_eq!(listening.upload_http_ipv4, Some(lease));

        set_upload_http_listener(false, None);

        let listener_disabled = snapshot();
        assert_eq!(listener_disabled.wifi_ipv4, Some(lease));
        assert!(!listener_disabled.upload_http_listening);
        assert_eq!(listener_disabled.upload_http_ipv4, None);

        set_wifi_link_connected(false);

        let disconnected = snapshot();
        assert!(!disconnected.wifi_link_connected);
        assert_eq!(disconnected.wifi_ipv4, None);
    }
}
