//! Bounded Medinote network-console adapter.
//!
//! Parsing and credential persistence stay in the shared netstack crate. This
//! target module only translates complete console lines and applies a
//! configuration after its credential record is durable. It never formats or
//! logs the password.

use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use netstack::config::{self, channels::NET_CONFIG_SET_UPDATES, NetConfigSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NetCommand {
    ConfigSet(NetConfigSet),
    ConfigGet,
    Start,
    Stop,
    Status,
    Recover,
    Listener(bool),
}

pub(crate) static COMMANDS: Channel<CriticalSectionRawMutex, NetCommand, 2> = Channel::new();

/// Parse one complete, already-framed console line.
pub(crate) fn parse(line: &[u8]) -> Option<NetCommand> {
    if let Some(config) = config::command::parse_netcfg_set_command(line) {
        return Some(NetCommand::ConfigSet(config));
    }
    if config::command::parse_netcfg_get_command(line) {
        return Some(NetCommand::ConfigGet);
    }
    if config::command::parse_net_start_command(line) {
        return Some(NetCommand::Start);
    }
    if config::command::parse_net_stop_command(line) {
        return Some(NetCommand::Stop);
    }
    if config::command::parse_net_status_command(line) {
        return Some(NetCommand::Status);
    }
    if config::command::parse_net_recover_command(line) {
        return Some(NetCommand::Recover);
    }
    config::command::parse_net_listener_command(line).map(NetCommand::Listener)
}

/// Admit one complete console line without allowing the console task to block
/// behind network or SD work. The worker emits the eventual result.
pub(crate) fn handle_line(line: &[u8]) -> bool {
    let Some(command) = parse(line) else {
        return false;
    };
    if COMMANDS.try_send(command).is_err() {
        console::println!("NET ERR reason=busy");
    }
    true
}

/// Network command worker. Boot has already restored credentials before this
/// task is spawned; this worker serializes persistence and runtime changes.
#[embassy_executor::task]
pub(crate) async fn run() {
    if crate::network_retention::take_restore_request() {
        if netstack::wifi::current_runtime_config()
            .credentials
            .is_none()
        {
            console::println!("NET restore=skipped reason=unprovisioned");
        } else if !crate::net_host::set_enabled(true).await {
            console::println!("NET ERR reason=restore_busy");
        }
    }
    loop {
        match select(COMMANDS.receive(), crate::net_host::wait_for_sleep_resume()).await {
            Either::First(command) => handle_command(command).await,
            Either::Second(()) => crate::net_host::handle_sleep_resume().await,
        }
    }
}

async fn handle_command(command: NetCommand) {
    match command {
        NetCommand::ConfigSet(config) => {
            if let Err(error) = store_config(config) {
                console::println!("NET ERR reason=persist_{:?}", error);
                return;
            }
            if !apply_runtime_config(config) {
                console::println!("NET ERR reason=runtime_busy");
                return;
            }
            console::println!("NET OK op=config_set");
        }
        NetCommand::ConfigGet => {
            console::println!(
                "{}",
                netstack::config::format_configured_netcfg_status_line()
            );
        }
        NetCommand::Start => {
            if netstack::wifi::current_runtime_config()
                .credentials
                .is_none()
            {
                console::println!("NET ERR reason=unprovisioned");
                return;
            }
            if crate::net_host::set_enabled(true).await {
                console::println!("NET OK op=start");
            } else {
                console::println!("NET ERR reason=busy");
            }
        }
        NetCommand::Stop => {
            if crate::net_host::set_enabled(false).await {
                console::println!("NET OK op=stop");
            } else {
                console::println!("NET ERR reason=busy");
            }
        }
        NetCommand::Status => {
            console::println!("{}", netstack::config::format_net_status_line());
        }
        NetCommand::Recover => {
            // Recovery is consumed by the network owner through its existing
            // control channel; this adapter remains UART/SD focused.
            if netstack::config::channels::NET_CONTROL_COMMANDS
                .try_send(netstack::config::NetControlCommand::Recover)
                .is_ok()
            {
                console::println!("NET OK op=recover");
            } else {
                console::println!("NET ERR reason=busy");
            }
        }
        NetCommand::Listener(enabled) => {
            if enabled && !crate::net_http::token_configured() {
                console::println!("NET ERR reason=token_required");
                return;
            }
            netstack::host::set_listener_enabled(enabled);
            console::println!("NET OK op=listener_{}", if enabled { "on" } else { "off" });
        }
    }
}

fn apply_runtime_config(mut config: NetConfigSet) -> bool {
    if config.credentials.is_none() {
        config.credentials = netstack::wifi::current_runtime_config().credentials;
    }
    while NET_CONFIG_SET_UPDATES.try_receive().is_ok() {}
    if NET_CONFIG_SET_UPDATES.try_send(config).is_err() {
        return false;
    }
    netstack::wifi::remember_runtime_config(config);
    true
}

/// Apply a complete configuration through the shared runtime and persist its
/// credentials before the caller reports success to the console.
pub(crate) fn store_config(
    config: NetConfigSet,
) -> Result<(), netstack::config::StoreCredentialsError> {
    let Some(credentials) = config.credentials else {
        return Ok(());
    };
    config::store_credentials(&credentials)
}
