pub(crate) const BATTERY_INTERVAL_SECONDS: u32 = 300;
/// Inkplate 4 TEMPERA BME688 board/enclosure temperature correction, measured
/// against an external SHT45 at approximately 7 cm after thermal settling.
/// This stays outside the driver so its factory-compensated reading remains
/// reusable without product calibration policy.
pub(crate) const INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES: i16 = -434;
/// BME688 relative-humidity gain measured in the same settled SHT45 series.
/// A gain fits the self-heating effect across humidity better than a one-point
/// additive offset. 1000 means no correction.
pub(crate) const INKPLATE_HUMIDITY_SCALE_PERMILLE: u32 = 1_445;
pub const UART_BAUD: u32 = 115_200;
// Shared UART command buffer for host instrumentation commands.
// NETCFG SET JSON payloads can exceed 320 bytes in hard-cut network mode.
pub(crate) const SERIAL_CMD_BUF_LEN: usize = 768;
pub(crate) const APP_STATE_STORE_MAGIC: u32 = 0x4150_5053;
pub(crate) const APP_STATE_STORE_VERSION: u8 = 5;
pub(crate) const APP_STATE_STORE_RECORD_LEN: usize = 128;
pub(crate) const APP_STATE_STORE_OFFSET: u32 = 0x12000;
pub(crate) const APP_STATE_STORE_SECTOR_SIZE: u32 = 0x1000;
pub(crate) const APP_STATE_LEGACY_OFFSET: u32 = 0x3ff000;
pub(crate) const BACKLIGHT_MAX_BRIGHTNESS: u8 = 63;
pub(crate) const BACKLIGHT_HOLD_MS: u64 = 3_000;
pub(crate) const BACKLIGHT_FADE_MS: u64 = 2_000;
// State transitions may trigger service teardown/allocation paths that can exceed
// short UART command deadlines on real hardware.
pub(crate) const APP_STATE_APPLY_ACK_TIMEOUT_MS: u64 = 150_000;
// A UI step includes a synchronous e-paper refresh before it is acknowledged.
pub(crate) const UI_CYCLE_STEP_ACK_TIMEOUT_MS: u64 = 150_000;
