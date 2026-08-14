use esp_hal::uart::{Config, RxConfig};

pub(crate) const RX_FIFO_FULL_THRESHOLD: u16 = 64;

pub(crate) fn config(baudrate: u32) -> Config {
    Config::default()
        .with_baudrate(baudrate)
        .with_rx(RxConfig::default().with_fifo_full_threshold(RX_FIFO_FULL_THRESHOLD))
}
