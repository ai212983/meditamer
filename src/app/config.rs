use core::sync::atomic::AtomicU32;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use meditamer::{
    inkplate_hal::{E_INK_HEIGHT, E_INK_WIDTH},
    suminagashi::{
        DitherMode as SuminagashiDitherMode, RenderMode as SuminagashiRenderMode, RgssMode,
    },
};
use u8g2_fonts::{fonts, FontRenderer};

#[cfg(feature = "asset-upload-http")]
use super::types::WifiCredentials;
use super::types::{
    AppEvent, SdPowerRequest, SdRequest, SdResult, SdUploadRequest, SdUploadResult, TapTraceSample,
};
#[cfg(feature = "asset-upload-http")]
use super::types::{WifiConfigRequest, WifiConfigResponse};

pub(crate) const SCREEN_WIDTH: i32 = E_INK_WIDTH as i32;
pub(crate) const SCREEN_HEIGHT: i32 = E_INK_HEIGHT as i32;
pub(crate) const REFRESH_INTERVAL_SECONDS: u32 = 300;
pub(crate) const BATTERY_INTERVAL_SECONDS: u32 = 300;
pub(crate) const FULL_REFRESH_EVERY_N_UPDATES: u32 = 20;
pub(crate) const UART_BAUD: u32 = 115_200;
pub(crate) const TIMESET_CMD_BUF_LEN: usize = 320;
pub(crate) const TITLE_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
pub(crate) const META_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
pub(crate) const RENDER_TIME_FONT: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB24_tf>();
pub(crate) const BATTERY_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
pub(crate) const TITLE_Y: i32 = 44;
pub(crate) const BATTERY_TEXT_Y: i32 = 44;
pub(crate) const BATTERY_TEXT_RIGHT_X: i32 = SCREEN_WIDTH - 16;
pub(crate) const DIVIDER_TOP_Y: i32 = 76;
pub(crate) const DIVIDER_BOTTOM_Y: i32 = 466;
pub(crate) const CLOCK_Y: i32 = 280;
pub(crate) const SYNC_Y: i32 = 514;
pub(crate) const UPTIME_Y: i32 = 552;
pub(crate) const CLOCK_REGION_LEFT: i32 = 64;
pub(crate) const CLOCK_REGION_TOP: i32 = 170;
pub(crate) const CLOCK_REGION_WIDTH: u32 = 472;
pub(crate) const CLOCK_REGION_HEIGHT: u32 = 220;
pub(crate) const META_REGION_LEFT: i32 = 72;
pub(crate) const META_REGION_TOP: i32 = 486;
pub(crate) const META_REGION_WIDTH: u32 = 456;
pub(crate) const META_REGION_HEIGHT: u32 = 98;
pub(crate) const BATTERY_REGION_LEFT: i32 = 430;
pub(crate) const BATTERY_REGION_TOP: i32 = 14;
pub(crate) const BATTERY_REGION_WIDTH: u32 = 170;
pub(crate) const BATTERY_REGION_HEIGHT: u32 = 54;
pub(crate) const SUMINAGASHI_RGSS_MODE: RgssMode = RgssMode::X4;
pub(crate) const SUMINAGASHI_CHUNK_ROWS: i32 = 8;
pub(crate) const SUMINAGASHI_USE_GRAY4: bool = false;
pub(crate) const VISUAL_DEFAULT_SEED: u32 = 12_345;
pub(crate) const SUMINAGASHI_DITHER_MODE: SuminagashiDitherMode =
    SuminagashiDitherMode::BlueNoise600;
pub(crate) const SUMINAGASHI_RENDER_MODE: SuminagashiRenderMode = if SUMINAGASHI_USE_GRAY4 {
    SuminagashiRenderMode::Gray4
} else {
    SuminagashiRenderMode::Mono1
};
pub(crate) const SUMINAGASHI_ENABLE_SUN: bool = false;
pub(crate) const SUMINAGASHI_SUN_ONLY: bool = false;
pub(crate) const SUMINAGASHI_BG_ALPHA_50_THRESHOLD: u8 = 128;
pub(crate) const SUN_TARGET_DIAMETER_PX: i32 = 75;
pub(crate) const SUN_FORCE_CENTER: bool = true;
pub(crate) const SUN_RENDER_TIME_Y_OFFSET: i32 = 22;
pub(crate) const SUNRISE_SECONDS_OF_DAY: i64 = 6 * 3_600;
pub(crate) const SUNSET_SECONDS_OF_DAY: i64 = 18 * 3_600;
pub(crate) const FACE_NORMAL_MIN_ABS_AXIS: i32 = 5_500;
pub(crate) const FACE_NORMAL_MIN_GAP: i32 = 1_200;
pub(crate) const FACE_BASELINE_HOLD_MS: u64 = 500;
pub(crate) const FACE_BASELINE_RECALIBRATE_MS: u64 = 1_200;
pub(crate) const FACE_DOWN_HOLD_MS: u64 = 750;
pub(crate) const FACE_DOWN_REARM_MS: u64 = 450;
pub(crate) const MODE_STORE_MAGIC: u32 = 0x4544_4F4D;
pub(crate) const MODE_STORE_VERSION: u8 = 2;
pub(crate) const MODE_STORE_RECORD_LEN: usize = 16;
pub(crate) const UI_TICK_MS: u64 = 50;
pub(crate) const IMU_INIT_RETRY_MS: u64 = 2_000;
pub(crate) const BACKLIGHT_MAX_BRIGHTNESS: u8 = 63;
pub(crate) const BACKLIGHT_HOLD_MS: u64 = 3_000;
pub(crate) const BACKLIGHT_FADE_MS: u64 = 2_000;
pub(crate) const TAP_TRACE_ENABLED: bool = false;
pub(crate) const TAP_TRACE_SAMPLE_MS: u64 = 25;
pub(crate) const TAP_TRACE_AUX_SAMPLE_MS: u64 = 250;

pub(crate) static APP_EVENTS: Channel<CriticalSectionRawMutex, AppEvent, 8> = Channel::new();
pub(crate) static SD_REQUESTS: Channel<CriticalSectionRawMutex, SdRequest, 8> = Channel::new();
pub(crate) static SD_RESULTS: Channel<CriticalSectionRawMutex, SdResult, 16> = Channel::new();
pub(crate) static SD_UPLOAD_REQUESTS: Channel<CriticalSectionRawMutex, SdUploadRequest, 2> =
    Channel::new();
pub(crate) static SD_UPLOAD_RESULTS: Channel<CriticalSectionRawMutex, SdUploadResult, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CREDENTIALS_UPDATES: Channel<CriticalSectionRawMutex, WifiCredentials, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CONFIG_REQUESTS: Channel<CriticalSectionRawMutex, WifiConfigRequest, 1> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CONFIG_RESPONSES: Channel<CriticalSectionRawMutex, WifiConfigResponse, 1> =
    Channel::new();
pub(crate) static SD_POWER_REQUESTS: Channel<CriticalSectionRawMutex, SdPowerRequest, 2> =
    Channel::new();
pub(crate) static SD_POWER_RESPONSES: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();
pub(crate) static TAP_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TapTraceSample, 8> =
    Channel::new();
pub(crate) static LAST_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static MAX_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);
