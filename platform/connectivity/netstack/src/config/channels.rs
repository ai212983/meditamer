//! Network-configuration channels between the serial command surface and the
//! network owner.
//!
//! Moved wholesale from `products/meditamer/src/firmware/config/channels.rs`
//! (product/target axis completion plan, Phase 4). Unconditional now: the
//! whole `netstack` crate is behind the product's `asset-upload-http`
//! optional dependency.

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use super::types::{NetConfigSet, NetControlCommand, WifiCredentials, WifiRuntimePolicy};

pub static WIFI_CREDENTIALS_UPDATES: Channel<CriticalSectionRawMutex, WifiCredentials, 2> =
    Channel::new();
pub static WIFI_RUNTIME_POLICY_UPDATES: Channel<CriticalSectionRawMutex, WifiRuntimePolicy, 2> =
    Channel::new();
pub static NET_CONFIG_SET_UPDATES: Channel<CriticalSectionRawMutex, NetConfigSet, 2> =
    Channel::new();
pub static NET_CONTROL_COMMANDS: Channel<CriticalSectionRawMutex, NetControlCommand, 2> =
    Channel::new();
