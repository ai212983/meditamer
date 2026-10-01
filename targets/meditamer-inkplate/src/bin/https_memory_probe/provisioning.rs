//! Boot-only, volatile provisioning for an unprovisioned diagnostic image.
//! Reuses the existing NETCFG parser and never echoes credentials or writes flash.
use esp_hal::{
    peripherals,
    uart::{Config, UartRx},
};
use netstack::config::{command::parse_netcfg_set_command, WifiCredentials};

pub fn receive(
    uart: peripherals::UART0<'static>,
    rx: peripherals::GPIO3<'static>,
) -> (WifiCredentials, UartRx<'static, esp_hal::Blocking>) {
    let mut uart = UartRx::new(uart, Config::default().with_baudrate(115200))
        .expect("probe UART setup")
        .with_rx(rx);
    console::println!("HTTPS_PROBE state=awaiting_credentials volatile=true");
    let mut line = [0u8; 256];
    let mut len = 0;
    let mut overflow = false;
    let start = esp_hal::time::Instant::now();
    loop {
        assert!(
            start.elapsed().as_secs() < 180,
            "probe provisioning timed out"
        );
        let mut byte = [0u8; 1];
        match uart.read_buffered(&mut byte) {
            Ok(0) => esp_hal::delay::Delay::new().delay_millis(1),
            Ok(_) => {
                if byte[0] == b'\r' || byte[0] == b'\n' {
                    let config = if overflow {
                        None
                    } else {
                        parse_netcfg_set_command(&line[..len])
                    };
                    line.fill(0);
                    len = 0;
                    overflow = false;
                    if let Some(credentials) = config.and_then(|c| c.credentials) {
                        console::println!("HTTPS_PROBE state=credentials_received volatile=true");
                        return (credentials, uart);
                    }
                } else if len < line.len() && !overflow {
                    line[len] = byte[0];
                    len += 1;
                } else {
                    overflow = true;
                }
            }
            Err(_) => {
                line.fill(0);
                len = 0;
                overflow = false;
            }
        }
    }
}
