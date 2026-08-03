pub(crate) const BATTERY_INTERVAL_SECONDS: u32 = 300;
// Automatic full refreshes are disabled. Full waveforms are still requested
// explicitly for startup, recovery, and requested repaints.
pub(crate) const AUTOMATIC_FULL_REFRESH_AFTER_PARTIAL_UPDATES: Option<u32> = None;
pub(crate) const UART_BAUD: u32 = 115_200;
// Shared UART command buffer for host instrumentation commands.
// NETCFG SET JSON payloads can exceed 320 bytes in hard-cut network mode.
pub(crate) const SERIAL_CMD_BUF_LEN: usize = 768;
pub(crate) const APP_STATE_STORE_MAGIC: u32 = 0x4150_5053;
pub(crate) const APP_STATE_STORE_VERSION: u8 = 3;
pub(crate) const APP_STATE_STORE_RECORD_LEN: usize = 32;
pub(crate) const BACKLIGHT_MAX_BRIGHTNESS: u8 = 63;
pub(crate) const BACKLIGHT_HOLD_MS: u64 = 3_000;
pub(crate) const BACKLIGHT_FADE_MS: u64 = 2_000;
// State transitions may trigger service teardown/allocation paths that can exceed
// short UART command deadlines on real hardware.
pub(crate) const APP_STATE_APPLY_ACK_TIMEOUT_MS: u64 = 150_000;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_CONFIG_RESPONSE_TIMEOUT_MS: u64 = 10_000;
