//! Network configuration: types, validation, channels, and durable Wi-Fi
//! credential persistence.
//!
//! [`types`] is the complete Wi-Fi configuration domain: credentials, runtime
//! policy and its sanitization, and the control/request/response types.
//! [`channels`] connect the serial command surface to the network owner.
//! [`flash_store`] owns the shared, versioned two-sector record format used by
//! both supported targets. [`persistence`] is the small installed store port;
//! each target maps the shared record operations onto its single internal
//! flash owner.

pub mod command;
pub mod flash_store;
mod persistence;
mod types;

pub mod channels;

pub use persistence::{
    format_configured_netcfg_status_line, format_net_status_line, format_netcfg_status_line,
    install_credential_store, result_code_label, store_credentials, StoreCredentialsError,
};
pub use types::{
    NetConfigSet, NetControlCommand, WifiConfigResultCode, WifiCredentials, WifiRuntimePolicy,
    WIFI_DHCP_TIMEOUT_MAX_MS, WIFI_DHCP_TIMEOUT_MIN_MS, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
};
