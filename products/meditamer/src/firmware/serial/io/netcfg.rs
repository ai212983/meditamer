//! UART framing for the `NETCFG SET`/`NETCFG GET` commands: persist through the
//! installed target flash port before publishing runtime configuration, then
//! use `netstack::config`'s shared status formatting.

use core::fmt::Write;

use netstack::config::{
    self, channels::NET_CONFIG_SET_UPDATES, NetConfigSet, StoreCredentialsError,
};

use crate::firmware::touch::debug_log::uart_write_all;

use super::SerialWriter;

pub(crate) async fn run_netcfg_set_command(uart: &mut SerialWriter, mut config: NetConfigSet) {
    if let Some(credentials) = config.credentials {
        match config::store_credentials(&credentials) {
            Ok(()) => {}
            Err(StoreCredentialsError::Failed(code)) => {
                let mut line = heapless::String::<96>::new();
                let _ = write!(
                    &mut line,
                    "NET ERR reason=persist_failed code={}\r\n",
                    config::result_code_label(code)
                );
                let _ = uart_write_all(uart, line.as_bytes()).await;
                return;
            }
            Err(StoreCredentialsError::Unavailable) => {
                let _ = uart_write_all(uart, b"NET ERR reason=persist_unavailable\r\n").await;
                return;
            }
        }
    }

    if config.credentials.is_none() {
        config.credentials = netstack::wifi::current_runtime_config().credentials;
    }

    while NET_CONFIG_SET_UPDATES.try_receive().is_ok() {}

    if NET_CONFIG_SET_UPDATES.try_send(config).is_err() {
        let _ = uart_write_all(uart, b"NET ERR reason=busy\r\n").await;
        return;
    }
    netstack::wifi::remember_runtime_config(config);

    let _ = uart_write_all(uart, b"NET OK op=config_set\r\n").await;
}

pub(crate) async fn run_netcfg_get_command(uart: &mut SerialWriter) {
    let line = config::format_netcfg_status_line();
    let _ = uart_write_all(uart, line.as_bytes()).await;
}
