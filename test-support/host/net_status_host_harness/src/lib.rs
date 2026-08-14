#![no_std]
#![allow(dead_code)]

#[cfg(test)]
extern crate std;

mod firmware;

#[cfg(test)]
mod tests {
    use super::firmware::observability::{
        set_upload_http_listener, set_wifi_ipv4, set_wifi_link_connected, snapshot,
    };

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
