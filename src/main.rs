#![no_std]
#![no_main]

mod pirata_clock_font;
mod sd_probe;

use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Instant, Ticker, Timer};
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
};
use embedded_storage::{ReadStorage, Storage};
use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout},
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode as SpiMode,
    },
    time::{Duration as HalDuration, Rate},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
    Async,
};
use esp_storage::FlashStorage;
use meditamer::{
    event_engine::{EngineTraceSample, EventEngine, SensorFrame},
    inkplate_hal::{InkplateHal, E_INK_WIDTH},
    platform::{BusyDelay, HalI2c},
    shanshui,
    sumi_sun::{self, Fx as SunFx, SumiSunParams},
    suminagashi::{
        self, DitherMode as SuminagashiDitherMode, RenderMode as SuminagashiRenderMode, RgssMode,
    },
};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

const SCREEN_WIDTH: i32 = E_INK_WIDTH as i32;
const REFRESH_INTERVAL_SECONDS: u32 = 300;
const BATTERY_INTERVAL_SECONDS: u32 = 300;
const FULL_REFRESH_EVERY_N_UPDATES: u32 = 20;
const UART_BAUD: u32 = 115_200;
const TIMESET_CMD_BUF_LEN: usize = 64;
const TITLE_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const META_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const RENDER_TIME_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB24_tf>();
const BATTERY_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const TITLE_Y: i32 = 44;
const BATTERY_TEXT_Y: i32 = 44;
const BATTERY_TEXT_RIGHT_X: i32 = SCREEN_WIDTH - 16;
const DIVIDER_TOP_Y: i32 = 76;
const DIVIDER_BOTTOM_Y: i32 = 466;
const CLOCK_Y: i32 = 280;
const SYNC_Y: i32 = 514;
const UPTIME_Y: i32 = 552;
const CLOCK_REGION_LEFT: i32 = 64;
const CLOCK_REGION_TOP: i32 = 170;
const CLOCK_REGION_WIDTH: u32 = 472;
const CLOCK_REGION_HEIGHT: u32 = 220;
const META_REGION_LEFT: i32 = 72;
const META_REGION_TOP: i32 = 486;
const META_REGION_WIDTH: u32 = 456;
const META_REGION_HEIGHT: u32 = 98;
const BATTERY_REGION_LEFT: i32 = 430;
const BATTERY_REGION_TOP: i32 = 14;
const BATTERY_REGION_WIDTH: u32 = 170;
const BATTERY_REGION_HEIGHT: u32 = 54;
const SUMINAGASHI_RGSS_MODE: RgssMode = RgssMode::X4;
const SUMINAGASHI_CHUNK_ROWS: i32 = 8;
const SUMINAGASHI_USE_GRAY4: bool = false;
const VISUAL_DEFAULT_SEED: u32 = 12_345;
const SUMINAGASHI_DITHER_MODE: SuminagashiDitherMode = SuminagashiDitherMode::BlueNoise600;
const SUMINAGASHI_RENDER_MODE: SuminagashiRenderMode = if SUMINAGASHI_USE_GRAY4 {
    SuminagashiRenderMode::Gray4
} else {
    SuminagashiRenderMode::Mono1
};
const SUMINAGASHI_ENABLE_SUN: bool = false;
const SUMINAGASHI_SUN_ONLY: bool = false;
const SUMINAGASHI_BG_ALPHA_50_THRESHOLD: u8 = 128;
const SUN_TARGET_DIAMETER_PX: i32 = 75;
const SUN_FORCE_CENTER: bool = true;
const SUN_RENDER_TIME_Y_OFFSET: i32 = 22;
const SUNRISE_SECONDS_OF_DAY: i64 = 6 * 3_600;
const SUNSET_SECONDS_OF_DAY: i64 = 18 * 3_600;
const FACE_NORMAL_MIN_ABS_AXIS: i32 = 5_500;
const FACE_NORMAL_MIN_GAP: i32 = 1_200;
const FACE_BASELINE_HOLD_MS: u64 = 500;
const FACE_BASELINE_RECALIBRATE_MS: u64 = 1_200;
const FACE_DOWN_HOLD_MS: u64 = 750;
const FACE_DOWN_REARM_MS: u64 = 450;
const MODE_STORE_MAGIC: u32 = 0x4544_4F4D;
const MODE_STORE_VERSION: u8 = 1;
const MODE_STORE_RECORD_LEN: usize = 16;
const UI_TICK_MS: u64 = 50;
const IMU_INIT_RETRY_MS: u64 = 2_000;
const BACKLIGHT_MAX_BRIGHTNESS: u8 = 63;
const BACKLIGHT_HOLD_MS: u64 = 3_000;
const BACKLIGHT_FADE_MS: u64 = 2_000;
const TAP_TRACE_ENABLED: bool = false;
const TAP_TRACE_SAMPLE_MS: u64 = 25;
const TAP_TRACE_AUX_SAMPLE_MS: u64 = 250;
static APP_EVENTS: Channel<CriticalSectionRawMutex, AppEvent, 4> = Channel::new();
static TAP_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TapTraceSample, 32> = Channel::new();
static LAST_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);
static MAX_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);

type InkplateDriver = InkplateHal<HalI2c<'static>, BusyDelay>;
type SerialUart = Uart<'static, Async>;
type SdProbeDriver = sd_probe::SdCardProbe<'static>;

#[derive(Clone, Copy)]
enum AppEvent {
    Refresh { uptime_seconds: u32 },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    ForceRepaint,
    ForceMarbleRepaint,
    SdProbe,
}

#[derive(Clone, Copy)]
struct TimeSyncCommand {
    unix_epoch_utc_seconds: u64,
    tz_offset_minutes: i32,
}

#[derive(Clone, Copy)]
enum SerialCommand {
    TimeSync(TimeSyncCommand),
    Repaint,
    RepaintMarble,
    Metrics,
    SdProbe,
}

#[derive(Clone, Copy)]
struct TimeSyncState {
    unix_epoch_utc_seconds: u64,
    tz_offset_minutes: i32,
    sync_instant: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Clock,
    Suminagashi,
    Shanshui,
}

impl DisplayMode {
    fn toggled(self) -> Self {
        match self {
            Self::Clock => Self::Suminagashi,
            Self::Suminagashi => Self::Shanshui,
            Self::Shanshui => Self::Clock,
        }
    }

    fn as_persisted(self) -> u8 {
        match self {
            Self::Clock => 0,
            Self::Suminagashi => 1,
            Self::Shanshui => 2,
        }
    }

    fn from_persisted(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clock),
            1 => Some(Self::Suminagashi),
            2 => Some(Self::Shanshui),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct FaceDownToggleState {
    baseline_pose: Option<FacePose>,
    baseline_candidate: Option<FacePose>,
    baseline_candidate_since: Option<Instant>,
    face_down_since: Option<Instant>,
    rearm_since: Option<Instant>,
    latched: bool,
}

impl FaceDownToggleState {
    fn new() -> Self {
        Self {
            baseline_pose: None,
            baseline_candidate: None,
            baseline_candidate_since: None,
            face_down_since: None,
            rearm_since: None,
            latched: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FacePose {
    axis: u8,
    sign: i8,
}

struct ModeStore<'d> {
    flash: FlashStorage<'d>,
    offset: u32,
}

impl<'d> ModeStore<'d> {
    fn new(flash_peripheral: esp_hal::peripherals::FLASH<'d>) -> Self {
        let flash = FlashStorage::new(flash_peripheral).multicore_auto_park();
        let capacity = flash.capacity() as u32;
        let offset = capacity.saturating_sub(FlashStorage::SECTOR_SIZE);
        Self { flash, offset }
    }

    fn load_mode(&mut self) -> Option<DisplayMode> {
        let mut record = [0u8; MODE_STORE_RECORD_LEN];
        self.flash.read(self.offset, &mut record).ok()?;
        if record.iter().all(|&byte| byte == 0xFF) {
            return None;
        }
        if u32::from_le_bytes([record[0], record[1], record[2], record[3]]) != MODE_STORE_MAGIC {
            return None;
        }
        if record[4] != MODE_STORE_VERSION {
            return None;
        }
        let expected = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        if record[MODE_STORE_RECORD_LEN - 1] != expected {
            return None;
        }
        DisplayMode::from_persisted(record[5])
    }

    fn save_mode(&mut self, mode: DisplayMode) {
        if self.load_mode() == Some(mode) {
            return;
        }

        let mut record = [0xFFu8; MODE_STORE_RECORD_LEN];
        record[0..4].copy_from_slice(&MODE_STORE_MAGIC.to_le_bytes());
        record[4] = MODE_STORE_VERSION;
        record[5] = mode.as_persisted();
        record[MODE_STORE_RECORD_LEN - 1] = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        let _ = self.flash.write(self.offset, &record);
    }
}

#[derive(Clone, Copy)]
struct TapTraceSample {
    t_ms: u64,
    tap_src: u8,
    seq_count: u8,
    tap_candidate: u8,
    cand_src: u8,
    state_id: u8,
    reject_reason: u8,
    candidate_score: u16,
    window_ms: u16,
    cooldown_active: u8,
    jerk_l1: i32,
    motion_veto: u8,
    gyro_l1: i32,
    int1: u8,
    int2: u8,
    power_good: i16,
    battery_percent: i16,
    gx: i16,
    gy: i16,
    gz: i16,
    ax: i16,
    ay: i16,
    az: i16,
}

struct DisplayContext {
    inkplate: InkplateDriver,
    sd_probe: SdProbeDriver,
    mode_store: ModeStore<'static>,
    _panel_pins: PanelPinHold<'static>,
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let uart_cfg = UartConfig::default().with_baudrate(UART_BAUD);
    let uart = Uart::new(peripherals.UART0, uart_cfg)
        .expect("failed to init UART0")
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1)
        .into_async();

    // Keep panel pins configured as GPIO outputs while fast-register path is active.
    let panel_pins = PanelPinHold {
        _cl: Output::new(peripherals.GPIO0, Level::Low, OutputConfig::default()),
        _le: Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default()),
        _d0: Output::new(peripherals.GPIO4, Level::Low, OutputConfig::default()),
        _d1: Output::new(peripherals.GPIO5, Level::Low, OutputConfig::default()),
        _d2: Output::new(peripherals.GPIO18, Level::Low, OutputConfig::default()),
        _d3: Output::new(peripherals.GPIO19, Level::Low, OutputConfig::default()),
        _d4: Output::new(peripherals.GPIO23, Level::Low, OutputConfig::default()),
        _d5: Output::new(peripherals.GPIO25, Level::Low, OutputConfig::default()),
        _d6: Output::new(peripherals.GPIO26, Level::Low, OutputConfig::default()),
        _d7: Output::new(peripherals.GPIO27, Level::Low, OutputConfig::default()),
        _ckv: Output::new(peripherals.GPIO32, Level::Low, OutputConfig::default()),
        _sph: Output::new(peripherals.GPIO33, Level::Low, OutputConfig::default()),
    };

    let sd_spi_cfg = SpiConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(SpiMode::_0);
    let sd_spi = Spi::new(peripherals.SPI2, sd_spi_cfg)
        .expect("failed to init SPI2 for SD probe")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);
    let sd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let sd_probe = sd_probe::SdCardProbe::new(sd_spi, sd_cs);

    let i2c_cfg = I2cConfig::default()
        .with_frequency(Rate::from_khz(100))
        .with_software_timeout(SoftwareTimeout::Transaction(HalDuration::from_millis(40)));
    let i2c = I2c::new(peripherals.I2C0, i2c_cfg)
        .expect("failed to init I2C0")
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);
    let i2c = HalI2c::new(i2c);
    let mut inkplate = match InkplateHal::new(i2c, BusyDelay::new()) {
        Ok(driver) => driver,
        Err(_) => halt_forever(),
    };

    if inkplate.init_core().is_err() {
        halt_forever();
    }

    let _ = inkplate.set_wakeup(true);
    let _ = inkplate.frontlight_off();
    let mode_store = ModeStore::new(peripherals.FLASH);

    let display_context = DisplayContext {
        inkplate,
        sd_probe,
        mode_store,
        _panel_pins: panel_pins,
    };

    let mut executor = esp_rtos::embassy::Executor::new();
    // Executor must live forever after startup.
    let executor = unsafe { make_static(&mut executor) };
    executor.run(move |spawner| {
        spawner.must_spawn(display_task(display_context));
        spawner.must_spawn(clock_task());
        spawner.must_spawn(battery_task());
        spawner.must_spawn(time_sync_task(uart));
    });
}

#[embassy_executor::task]
async fn clock_task() {
    let boot_instant = Instant::now();
    APP_EVENTS
        .send(AppEvent::Refresh { uptime_seconds: 0 })
        .await;
    let mut ticker = Ticker::every(Duration::from_secs(REFRESH_INTERVAL_SECONDS as u64));

    loop {
        ticker.next().await;
        let uptime_seconds = Instant::now()
            .saturating_duration_since(boot_instant)
            .as_secs()
            .min(u32::MAX as u64) as u32;
        APP_EVENTS.send(AppEvent::Refresh { uptime_seconds }).await;
    }
}

#[embassy_executor::task]
async fn battery_task() {
    APP_EVENTS.send(AppEvent::BatteryTick).await;
    let mut ticker = Ticker::every(Duration::from_secs(BATTERY_INTERVAL_SECONDS as u64));

    loop {
        ticker.next().await;
        APP_EVENTS.send(AppEvent::BatteryTick).await;
    }
}

#[embassy_executor::task]
async fn time_sync_task(mut uart: SerialUart) {
    let mut line_buf = [0u8; TIMESET_CMD_BUF_LEN];
    let mut line_len = 0usize;
    let mut rx = [0u8; 1];

    if TAP_TRACE_ENABLED {
        let _ = uart
            .write_async(
                b"tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
            )
            .await;
    }

    loop {
        if TAP_TRACE_ENABLED {
            while let Ok(sample) = TAP_TRACE_SAMPLES.try_receive() {
                write_tap_trace_sample(&mut uart, sample).await;
            }
        }

        match with_timeout(Duration::from_millis(10), uart.read_async(&mut rx)).await {
            Ok(Ok(1)) => {
                let byte = rx[0];
                if byte == b'\r' || byte == b'\n' {
                    if line_len == 0 {
                        continue;
                    }
                    if let Some(cmd) = parse_serial_command(&line_buf[..line_len]) {
                        match cmd {
                            SerialCommand::TimeSync(cmd) => {
                                if APP_EVENTS.try_send(AppEvent::TimeSync(cmd)).is_ok() {
                                    let _ = uart.write_async(b"TIMESET OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"TIMESET BUSY\r\n").await;
                                }
                            }
                            SerialCommand::Repaint => {
                                if APP_EVENTS.try_send(AppEvent::ForceRepaint).is_ok() {
                                    let _ = uart.write_async(b"REPAINT OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"REPAINT BUSY\r\n").await;
                                }
                            }
                            SerialCommand::RepaintMarble => {
                                if APP_EVENTS.try_send(AppEvent::ForceMarbleRepaint).is_ok() {
                                    let _ = uart.write_async(b"REPAINT_MARBLE OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"REPAINT_MARBLE BUSY\r\n").await;
                                }
                            }
                            SerialCommand::Metrics => {
                                let last_ms = LAST_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                                let max_ms = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
                                let mut line = heapless::String::<96>::new();
                                let _ = write!(
                                    &mut line,
                                    "METRICS MARBLE_REDRAW_MS={} MAX_MS={}\r\n",
                                    last_ms, max_ms
                                );
                                let _ = uart.write_async(line.as_bytes()).await;
                            }
                            SerialCommand::SdProbe => {
                                if APP_EVENTS.try_send(AppEvent::SdProbe).is_ok() {
                                    let _ = uart.write_async(b"SDPROBE OK\r\n").await;
                                } else {
                                    let _ = uart.write_async(b"SDPROBE BUSY\r\n").await;
                                }
                            }
                        }
                    } else {
                        let _ = uart.write_async(b"CMD ERR\r\n").await;
                    }
                    line_len = 0;
                } else if line_len < line_buf.len() {
                    line_buf[line_len] = byte;
                    line_len += 1;
                } else {
                    // Reset on overflow and wait for next line terminator.
                    line_len = 0;
                }
            }
            _ => {}
        }
    }
}

async fn write_tap_trace_sample(uart: &mut SerialUart, sample: TapTraceSample) {
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "tap_trace,{},{:#04x},{},{},{:#04x},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        sample.t_ms,
        sample.tap_src,
        sample.seq_count,
        sample.tap_candidate,
        sample.cand_src,
        sample.state_id,
        sample.reject_reason,
        sample.candidate_score,
        sample.window_ms,
        sample.cooldown_active,
        sample.jerk_l1,
        sample.motion_veto,
        sample.gyro_l1,
        sample.int1,
        sample.int2,
        sample.power_good,
        sample.battery_percent,
        sample.gx,
        sample.gy,
        sample.gz,
        sample.ax,
        sample.ay,
        sample.az
    );
    let _ = uart.write_async(line.as_bytes()).await;
}

fn parse_timeset_command(line: &[u8]) -> Option<TimeSyncCommand> {
    let cmd_idx = find_subslice(line, b"TIMESET")?;
    let mut i = cmd_idx + b"TIMESET".len();
    let len = line.len();

    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (unix_epoch_utc_seconds, next_i) = parse_u64_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (tz_offset_minutes, next_i) = parse_i32_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != len {
        return None;
    }
    if !(-720..=840).contains(&tz_offset_minutes) {
        return None;
    }

    Some(TimeSyncCommand {
        unix_epoch_utc_seconds,
        tz_offset_minutes,
    })
}

fn parse_serial_command(line: &[u8]) -> Option<SerialCommand> {
    if parse_repaint_marble_command(line) {
        return Some(SerialCommand::RepaintMarble);
    }
    if parse_repaint_command(line) {
        return Some(SerialCommand::Repaint);
    }
    if parse_metrics_command(line) {
        return Some(SerialCommand::Metrics);
    }
    if parse_sdprobe_command(line) {
        return Some(SerialCommand::SdProbe);
    }

    parse_timeset_command(line).map(SerialCommand::TimeSync)
}

fn parse_repaint_command(line: &[u8]) -> bool {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let cmd = &line[start..end];
    cmd == b"REPAINT" || cmd == b"REFRESH"
}

fn parse_repaint_marble_command(line: &[u8]) -> bool {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let cmd = &line[start..end];
    cmd == b"REPAINT_MARBLE" || cmd == b"MARBLE"
}

fn parse_metrics_command(line: &[u8]) -> bool {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let cmd = &line[start..end];
    cmd == b"METRICS" || cmd == b"PERF"
}

fn parse_sdprobe_command(line: &[u8]) -> bool {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    let cmd = &line[start..end];
    cmd == b"SDPROBE"
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&idx| &haystack[idx..idx + needle.len()] == needle)
}

fn parse_u64_ascii(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value
            .checked_mul(10)?
            .checked_add((bytes[i] - b'0') as u64)?;
        i += 1;
    }
    if i == start {
        None
    } else {
        Some((value, i))
    }
}

fn parse_i32_ascii(bytes: &[u8], i: usize) -> Option<(i32, usize)> {
    if i >= bytes.len() {
        return None;
    }
    let mut idx = i;
    let mut sign = 1i64;
    if bytes[idx] == b'-' {
        sign = -1;
        idx += 1;
    } else if bytes[idx] == b'+' {
        idx += 1;
    }

    let (unsigned, next_idx) = parse_u64_ascii(bytes, idx)?;
    let signed = sign.checked_mul(unsigned as i64)?;
    if signed < i32::MIN as i64 || signed > i32::MAX as i64 {
        return None;
    }
    Some((signed as i32, next_idx))
}

#[embassy_executor::task]
async fn display_task(mut context: DisplayContext) {
    let mut update_count = 0u32;
    let mut last_uptime_seconds = 0u32;
    let mut time_sync: Option<TimeSyncState> = None;
    let mut battery_percent: Option<u8> = None;
    let mut display_mode = DisplayMode::Shanshui;
    let mut screen_initialized = false;
    let mut pattern_nonce = 0u32;
    let mut first_visual_seed_pending = true;
    let mut face_down_toggle = FaceDownToggleState::new();
    let mut imu_double_tap_ready = false;
    let mut imu_retry_at = Instant::now();
    let mut event_engine = EventEngine::default();
    let mut last_engine_trace = EngineTraceSample::default();
    let mut last_detect_tap_src = 0u8;
    let mut last_detect_int1 = 0u8;
    let trace_epoch = Instant::now();
    let mut tap_trace_next_sample_at = Instant::now();
    let mut tap_trace_aux_next_sample_at = Instant::now();
    let mut tap_trace_power_good: i16 = -1;
    let mut backlight_cycle_start: Option<Instant> = None;
    let mut backlight_level = 0u8;

    run_sd_probe("boot", &mut context.inkplate, &mut context.sd_probe);

    loop {
        match with_timeout(Duration::from_millis(UI_TICK_MS), APP_EVENTS.receive()).await {
            Ok(event) => match event {
                AppEvent::Refresh { uptime_seconds } => {
                    last_uptime_seconds = uptime_seconds;
                    if display_mode == DisplayMode::Clock {
                        let do_full_refresh =
                            !screen_initialized || update_count % FULL_REFRESH_EVERY_N_UPDATES == 0;
                        render_clock_update(
                            &mut context.inkplate,
                            uptime_seconds,
                            time_sync,
                            battery_percent,
                            do_full_refresh,
                        );
                        update_count = update_count.wrapping_add(1);
                    } else {
                        render_visual_update(
                            &mut context.inkplate,
                            display_mode,
                            uptime_seconds,
                            time_sync,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                        )
                        .await;
                        update_count = 0;
                    }
                    screen_initialized = true;
                }
                AppEvent::BatteryTick => {
                    if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
                        battery_percent = Some(sampled_percent);
                    }

                    if screen_initialized {
                        if display_mode == DisplayMode::Clock {
                            render_battery_update(&mut context.inkplate, battery_percent);
                        }
                    } else {
                        // Avoid a startup double-render in non-clock modes:
                        // `clock_task` sends an immediate Refresh event, so initializing
                        // here as well can flash one frame and immediately overwrite it.
                        if display_mode == DisplayMode::Clock {
                            render_active_mode(
                                &mut context.inkplate,
                                display_mode,
                                last_uptime_seconds,
                                time_sync,
                                battery_percent,
                                &mut pattern_nonce,
                                &mut first_visual_seed_pending,
                                true,
                            )
                            .await;
                            screen_initialized = true;
                        }
                    }
                }
                AppEvent::TimeSync(cmd) => {
                    let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
                    last_uptime_seconds = last_uptime_seconds.max(uptime_now);
                    time_sync = Some(TimeSyncState {
                        unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
                        tz_offset_minutes: cmd.tz_offset_minutes,
                        sync_instant: Instant::now(),
                    });
                    // Force a full redraw on sync so timestamp/status updates immediately
                    // across all modes, including Shanshui/Suminagashi.
                    update_count = 0;
                    render_active_mode(
                        &mut context.inkplate,
                        display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                        true,
                    )
                    .await;
                    screen_initialized = true;
                }
                AppEvent::ForceRepaint => {
                    update_count = 0;
                    render_active_mode(
                        &mut context.inkplate,
                        display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                        true,
                    )
                    .await;
                    screen_initialized = true;
                }
                AppEvent::ForceMarbleRepaint => {
                    let seed = next_visual_seed(
                        last_uptime_seconds,
                        time_sync,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                    );
                    if display_mode == DisplayMode::Shanshui {
                        render_shanshui_update(
                            &mut context.inkplate,
                            seed,
                            last_uptime_seconds,
                            time_sync,
                        )
                        .await;
                    } else {
                        render_suminagashi_update(
                            &mut context.inkplate,
                            seed,
                            last_uptime_seconds,
                            time_sync,
                        )
                        .await;
                    }
                    screen_initialized = true;
                }
                AppEvent::SdProbe => {
                    run_sd_probe("manual", &mut context.inkplate, &mut context.sd_probe);
                }
            },
            Err(_) => {}
        }

        if !imu_double_tap_ready && Instant::now() >= imu_retry_at {
            imu_double_tap_ready = context.inkplate.lsm6ds3_init_double_tap().unwrap_or(false);
            if imu_double_tap_ready {
                let now_ms = Instant::now()
                    .saturating_duration_since(trace_epoch)
                    .as_millis();
                last_engine_trace = event_engine.imu_recovered(now_ms).trace;
            }
            imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
        }

        if imu_double_tap_ready {
            match (
                context.inkplate.lsm6ds3_read_tap_src(),
                context.inkplate.lsm6ds3_int1_level(),
                context.inkplate.lsm6ds3_read_motion_raw(),
            ) {
                (Ok(tap_src), Ok(int1), Ok((gx, gy, gz, ax, ay, az))) => {
                    let now = Instant::now();
                    let now_ms = now.saturating_duration_since(trace_epoch).as_millis();
                    last_detect_tap_src = tap_src;
                    last_detect_int1 = if int1 { 1 } else { 0 };

                    let output = event_engine.tick(SensorFrame {
                        now_ms,
                        tap_src,
                        int1,
                        gx,
                        gy,
                        gz,
                        ax,
                        ay,
                        az,
                    });
                    last_engine_trace = output.trace;

                    if output.actions.contains_backlight_trigger() {
                        trigger_backlight_cycle(
                            &mut context.inkplate,
                            &mut backlight_cycle_start,
                            &mut backlight_level,
                        );
                    }

                    if update_face_down_toggle(&mut face_down_toggle, now, ax, ay, az) {
                        display_mode = display_mode.toggled();
                        context.mode_store.save_mode(display_mode);
                        update_count = 0;
                        render_active_mode(
                            &mut context.inkplate,
                            display_mode,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                            true,
                        )
                        .await;
                        screen_initialized = true;
                    }
                }
                _ => {
                    imu_double_tap_ready = false;
                    let now_ms = Instant::now()
                        .saturating_duration_since(trace_epoch)
                        .as_millis();
                    last_engine_trace = event_engine.imu_fault(now_ms).trace;
                    last_detect_tap_src = 0;
                    last_detect_int1 = 0;
                    imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
                }
            }
        }

        if TAP_TRACE_ENABLED && imu_double_tap_ready {
            let now = Instant::now();

            if now >= tap_trace_aux_next_sample_at {
                tap_trace_power_good = context
                    .inkplate
                    .read_power_good()
                    .ok()
                    .map(|v| v as i16)
                    .unwrap_or(-1);
                tap_trace_aux_next_sample_at = now + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
            }

            if now >= tap_trace_next_sample_at {
                match (
                    context.inkplate.lsm6ds3_int2_level(),
                    context.inkplate.lsm6ds3_read_motion_raw(),
                ) {
                    (Ok(int2), Ok((gx, gy, gz, ax, ay, az))) => {
                        let battery_percent_i16 = battery_percent.map_or(-1, i16::from);
                        let t_ms = now.saturating_duration_since(trace_epoch).as_millis();
                        let sample = TapTraceSample {
                            t_ms,
                            tap_src: last_detect_tap_src,
                            seq_count: last_engine_trace.seq_count,
                            tap_candidate: last_engine_trace.tap_candidate,
                            cand_src: last_engine_trace.candidate_source_mask,
                            state_id: last_engine_trace.state_id.as_u8(),
                            reject_reason: last_engine_trace.reject_reason.as_u8(),
                            candidate_score: last_engine_trace.candidate_score.0,
                            window_ms: last_engine_trace.window_ms,
                            cooldown_active: last_engine_trace.cooldown_active,
                            jerk_l1: last_engine_trace.jerk_l1,
                            motion_veto: last_engine_trace.motion_veto,
                            gyro_l1: last_engine_trace.gyro_l1,
                            int1: last_detect_int1,
                            int2: if int2 { 1 } else { 0 },
                            power_good: tap_trace_power_good,
                            battery_percent: battery_percent_i16,
                            gx,
                            gy,
                            gz,
                            ax,
                            ay,
                            az,
                        };
                        let _ = TAP_TRACE_SAMPLES.try_send(sample);
                    }
                    _ => {
                        // Ignore transient I2C sample failures during trace capture.
                    }
                }
                tap_trace_next_sample_at = now + Duration::from_millis(TAP_TRACE_SAMPLE_MS);
            }
        }

        run_backlight_timeline(
            &mut context.inkplate,
            &mut backlight_cycle_start,
            &mut backlight_level,
        );
    }
}

unsafe fn make_static<T>(value: &mut T) -> &'static mut T {
    unsafe { core::mem::transmute(value) }
}

fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn run_sd_probe(reason: &str, inkplate: &mut InkplateDriver, sd_probe: &mut SdProbeDriver) {
    if inkplate.sd_card_power_on().is_err() {
        esp_println::println!("sdprobe[{}]: power_on_error", reason);
        return;
    }

    let result = sd_probe.probe();

    match result {
        Ok(status) => {
            let version = match status.version {
                sd_probe::SdCardVersion::V1 => "v1.x",
                sd_probe::SdCardVersion::V2 => "v2+",
            };
            let capacity = if status.high_capacity {
                "sdhc_or_sdxc"
            } else {
                "sdsc"
            };
            let filesystem = match status.filesystem {
                sd_probe::SdFilesystem::ExFat => "exfat",
                sd_probe::SdFilesystem::Fat32 => "fat32",
                sd_probe::SdFilesystem::Fat16 => "fat16",
                sd_probe::SdFilesystem::Fat12 => "fat12",
                sd_probe::SdFilesystem::Ntfs => "ntfs",
                sd_probe::SdFilesystem::Unknown => "unknown",
            };
            let gib_x100 = status
                .capacity_bytes
                .saturating_mul(100)
                .saturating_div(1024 * 1024 * 1024);
            let gib_int = gib_x100 / 100;
            let gib_frac = gib_x100 % 100;
            esp_println::println!(
                "sdprobe[{}]: card_detected version={} capacity={} fs={} bytes={} size_gib={}.{:02}",
                reason,
                version,
                capacity,
                filesystem,
                status.capacity_bytes,
                gib_int,
                gib_frac
            );
        }
        Err(err) => match err {
            sd_probe::SdProbeError::Spi(spi_err) => {
                esp_println::println!("sdprobe[{}]: not_detected spi_error={:?}", reason, spi_err);
            }
            sd_probe::SdProbeError::Cmd0Failed(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd0_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd8Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd8_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd8EchoMismatch(r7) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd8_echo={:02x}{:02x}{:02x}{:02x}",
                    reason,
                    r7[0],
                    r7[1],
                    r7[2],
                    r7[3]
                );
            }
            sd_probe::SdProbeError::Acmd41Timeout(r1) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected acmd41_last_r1=0x{:02x}",
                    reason,
                    r1
                );
            }
            sd_probe::SdProbeError::Cmd58Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd58_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd9Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd9_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd17Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd17_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::NoResponse(cmd) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd{}_no_response", reason, cmd);
            }
            sd_probe::SdProbeError::DataTokenTimeout(cmd) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token_timeout",
                    reason,
                    cmd
                );
            }
            sd_probe::SdProbeError::DataTokenUnexpected(cmd, token) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token=0x{:02x}",
                    reason,
                    cmd,
                    token
                );
            }
            sd_probe::SdProbeError::CapacityDecodeFailed => {
                esp_println::println!("sdprobe[{}]: not_detected capacity_decode_failed", reason);
            }
        },
    }

    if inkplate.sd_card_power_off().is_err() {
        esp_println::println!("sdprobe[{}]: power_off_error", reason);
    }
}

fn render_clock_update(
    display: &mut InkplateDriver,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    full_refresh: bool,
) {
    if full_refresh {
        draw_clock_static(display);
        draw_clock_dynamic(display, uptime_seconds, time_sync);
        draw_battery_status(display, battery_percent);
        let _ = display.display_bw(false);
        return;
    }

    erase_clock_dynamic_regions(display);
    draw_clock_dynamic(display, uptime_seconds, time_sync);
    let _ = display.display_bw(false);
}

async fn render_active_mode(
    display: &mut InkplateDriver,
    mode: DisplayMode,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    force_full: bool,
) {
    match mode {
        DisplayMode::Clock => render_clock_update(
            display,
            uptime_seconds,
            time_sync,
            battery_percent,
            force_full,
        ),
        DisplayMode::Suminagashi => {
            let seed = next_visual_seed(
                uptime_seconds,
                time_sync,
                pattern_nonce,
                first_visual_seed_pending,
            );
            render_suminagashi_update(display, seed, uptime_seconds, time_sync).await;
        }
        DisplayMode::Shanshui => {
            let seed = next_visual_seed(
                uptime_seconds,
                time_sync,
                pattern_nonce,
                first_visual_seed_pending,
            );
            render_shanshui_update(display, seed, uptime_seconds, time_sync).await;
        }
    }
}

async fn render_visual_update(
    display: &mut InkplateDriver,
    mode: DisplayMode,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
) {
    let seed = next_visual_seed(
        uptime_seconds,
        time_sync,
        pattern_nonce,
        first_visual_seed_pending,
    );
    match mode {
        DisplayMode::Clock => {}
        DisplayMode::Suminagashi => {
            render_suminagashi_update(display, seed, uptime_seconds, time_sync).await
        }
        DisplayMode::Shanshui => {
            render_shanshui_update(display, seed, uptime_seconds, time_sync).await
        }
    }
}

async fn render_suminagashi_update(
    display: &mut InkplateDriver,
    seed: u32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) {
    let started = Instant::now();
    let width = display.width() as i32;
    let height = display.height() as i32;
    let scene = suminagashi::build_seeded_scene(seed, Size::new(width as u32, height as u32));
    let render_time_text = format_render_time_text(uptime_seconds, time_sync);
    let sun_params = if SUMINAGASHI_ENABLE_SUN {
        Some(build_sun_params(
            seed,
            sun_center_for_time(width, height, uptime_seconds, time_sync),
        ))
    } else {
        None
    };

    let _ = display.clear(BinaryColor::Off);

    let mut y = 0i32;
    while y < height {
        let y_end = (y + SUMINAGASHI_CHUNK_ROWS).min(height);
        if !SUMINAGASHI_SUN_ONLY {
            suminagashi::render_scene_rows_bw_masked(
                &scene,
                width,
                y,
                y_end,
                SUMINAGASHI_RGSS_MODE,
                SUMINAGASHI_RENDER_MODE,
                SUMINAGASHI_DITHER_MODE,
                |x, py| background_alpha_50_mask(x, py, seed),
                |x, py| display.set_pixel_bw(x as usize, py as usize, true),
            );
        }
        if let Some(sun_params) = sun_params {
            sumi_sun::render_sumi_sun_rows_bw(
                width,
                height,
                y,
                y_end,
                sun_params,
                SUMINAGASHI_RENDER_MODE,
                SUMINAGASHI_DITHER_MODE,
                |x, py| display.set_pixel_bw(x as usize, py as usize, true),
            );
        }
        y = y_end;
        if y < height {
            Timer::after_millis(1).await;
        }
    }

    draw_centered_bitmap_text_with_white_rim(
        display,
        &RENDER_TIME_FONT,
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw(false);

    let elapsed_ms = Instant::now()
        .saturating_duration_since(started)
        .as_millis()
        .min(u32::MAX as u64) as u32;
    LAST_MARBLE_REDRAW_MS.store(elapsed_ms, Ordering::Relaxed);

    let mut current_max = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
    while elapsed_ms > current_max {
        match MAX_MARBLE_REDRAW_MS.compare_exchange_weak(
            current_max,
            elapsed_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(seen) => current_max = seen,
        }
    }
}

async fn render_shanshui_update(
    display: &mut InkplateDriver,
    seed: u32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) {
    let started = Instant::now();
    let width = display.width() as i32;
    let height = display.height() as i32;
    let render_time_text = format_render_time_text(uptime_seconds, time_sync);

    let _ = display.clear(BinaryColor::Off);

    shanshui::render_shanshui_bw_atkinson(width, height, seed, |x, py| {
        display.set_pixel_bw(x as usize, py as usize, true)
    });

    draw_centered_bitmap_text_with_white_rim(
        display,
        &RENDER_TIME_FONT,
        render_time_text.as_str(),
        height - SUN_RENDER_TIME_Y_OFFSET,
        2,
    );

    let _ = display.display_bw(false);

    let elapsed_ms = Instant::now()
        .saturating_duration_since(started)
        .as_millis()
        .min(u32::MAX as u64) as u32;
    LAST_MARBLE_REDRAW_MS.store(elapsed_ms, Ordering::Relaxed);

    let mut current_max = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
    while elapsed_ms > current_max {
        match MAX_MARBLE_REDRAW_MS.compare_exchange_weak(
            current_max,
            elapsed_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(seen) => current_max = seen,
        }
    }
}

fn update_face_down_toggle(
    state: &mut FaceDownToggleState,
    now: Instant,
    ax: i16,
    ay: i16,
    az: i16,
) -> bool {
    let ax_i32 = ax as i32;
    let ay_i32 = ay as i32;
    let az_i32 = az as i32;
    let Some(pose) = detect_face_pose(ax_i32, ay_i32, az_i32) else {
        state.face_down_since = None;
        state.rearm_since = None;
        return false;
    };

    if state.baseline_pose.is_none() {
        if update_baseline_candidate(state, now, pose, FACE_BASELINE_HOLD_MS) {
            state.baseline_pose = Some(pose);
        }
        return false;
    }

    let baseline_pose = state.baseline_pose.unwrap_or(pose);
    if !state.latched && pose.axis != baseline_pose.axis {
        if update_baseline_candidate(state, now, pose, FACE_BASELINE_RECALIBRATE_MS) {
            state.baseline_pose = Some(pose);
        }
        state.face_down_since = None;
        state.rearm_since = None;
        return false;
    }
    clear_baseline_candidate(state);

    let is_face_down = pose.axis == baseline_pose.axis && pose.sign == -baseline_pose.sign;
    if is_face_down {
        state.rearm_since = None;
        if state.latched {
            return false;
        }
        let since = state.face_down_since.get_or_insert(now);
        if now.saturating_duration_since(*since).as_millis() >= FACE_DOWN_HOLD_MS {
            state.latched = true;
            state.face_down_since = None;
            return true;
        }
        return false;
    }

    state.face_down_since = None;
    if state.latched {
        let since = state.rearm_since.get_or_insert(now);
        if now.saturating_duration_since(*since).as_millis() >= FACE_DOWN_REARM_MS {
            state.latched = false;
            state.rearm_since = None;
            // Re-anchor baseline to current stable face orientation after a completed flip cycle.
            state.baseline_pose = Some(pose);
        }
    } else {
        state.rearm_since = None;
    }
    false
}

fn update_baseline_candidate(
    state: &mut FaceDownToggleState,
    now: Instant,
    pose: FacePose,
    hold_ms: u64,
) -> bool {
    if state.baseline_candidate != Some(pose) {
        state.baseline_candidate = Some(pose);
        state.baseline_candidate_since = Some(now);
        return false;
    }
    let Some(since) = state.baseline_candidate_since else {
        state.baseline_candidate_since = Some(now);
        return false;
    };
    if now.saturating_duration_since(since).as_millis() >= hold_ms {
        clear_baseline_candidate(state);
        return true;
    }
    false
}

fn clear_baseline_candidate(state: &mut FaceDownToggleState) {
    state.baseline_candidate = None;
    state.baseline_candidate_since = None;
}

fn detect_face_pose(ax: i32, ay: i32, az: i32) -> Option<FacePose> {
    let x = ax.abs();
    let y = ay.abs();
    let z = az.abs();
    let (axis, major, secondary) = if x >= y && x >= z {
        (0u8, x, y.max(z))
    } else if y >= x && y >= z {
        (1u8, y, x.max(z))
    } else {
        (2u8, z, x.max(y))
    };

    if major < FACE_NORMAL_MIN_ABS_AXIS || (major - secondary) < FACE_NORMAL_MIN_GAP {
        return None;
    }

    let signed = match axis {
        0 => ax,
        1 => ay,
        _ => az,
    };

    Some(FacePose {
        axis,
        sign: if signed >= 0 { 1 } else { -1 },
    })
}

fn draw_clock_static<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = display.clear(BinaryColor::Off);
    draw_divider(display, DIVIDER_TOP_Y);
    draw_divider(display, DIVIDER_BOTTOM_Y);
    draw_centered_bitmap_text(display, &TITLE_FONT, "MEDITAMER CLOCK", TITLE_Y);
}

fn erase_clock_dynamic_regions<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    clear_region(
        display,
        CLOCK_REGION_LEFT,
        CLOCK_REGION_TOP,
        CLOCK_REGION_WIDTH,
        CLOCK_REGION_HEIGHT,
    );
    clear_region(
        display,
        META_REGION_LEFT,
        META_REGION_TOP,
        META_REGION_WIDTH,
        META_REGION_HEIGHT,
    );
}

fn erase_battery_region<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    clear_region(
        display,
        BATTERY_REGION_LEFT,
        BATTERY_REGION_TOP,
        BATTERY_REGION_WIDTH,
        BATTERY_REGION_HEIGHT,
    );
}

fn render_battery_update(display: &mut InkplateDriver, battery_percent: Option<u8>) {
    erase_battery_region(display);
    draw_battery_status(display, battery_percent);
    let _ = display.display_bw_partial(false);
}

fn draw_clock_dynamic<T>(display: &mut T, uptime_seconds: u32, time_sync: Option<TimeSyncState>)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let clock_text = format_clock_text(uptime_seconds, time_sync);
    let uptime_text = format_uptime_text(uptime_seconds);
    let sync_text = format_sync_text(time_sync);

    pirata_clock_font::draw_time_centered(
        display,
        clock_text.as_str(),
        Point::new(SCREEN_WIDTH / 2, CLOCK_Y),
    );
    draw_centered_bitmap_text(display, &META_FONT, sync_text.as_str(), SYNC_Y);
    draw_centered_bitmap_text(display, &META_FONT, uptime_text.as_str(), UPTIME_Y);
}

fn draw_battery_status<T>(display: &mut T, battery_percent: Option<u8>)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let battery_text = format_battery_text(battery_percent);
    draw_right_aligned_bitmap_text(
        display,
        &BATTERY_FONT,
        battery_text.as_str(),
        BATTERY_TEXT_RIGHT_X,
        BATTERY_TEXT_Y,
    );
}

fn draw_centered_bitmap_text<T>(display: &mut T, renderer: &FontRenderer, text: &str, center_y: i32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = renderer.render_aligned(
        text,
        Point::new(SCREEN_WIDTH / 2, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_centered_bitmap_text_with_white_rim<T>(
    display: &mut T,
    renderer: &FontRenderer,
    text: &str,
    center_y: i32,
    rim_px: i32,
) where
    T: DrawTarget<Color = BinaryColor>,
{
    let cx = SCREEN_WIDTH / 2;
    let mut dy = -rim_px;
    while dy <= rim_px {
        let mut dx = -rim_px;
        while dx <= rim_px {
            if dx != 0 || dy != 0 {
                let _ = renderer.render_aligned(
                    text,
                    Point::new(cx + dx, center_y + dy),
                    VerticalPosition::Center,
                    HorizontalAlignment::Center,
                    FontColor::Transparent(BinaryColor::Off),
                    display,
                );
            }
            dx += 1;
        }
        dy += 1;
    }

    let _ = renderer.render_aligned(
        text,
        Point::new(cx, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_right_aligned_bitmap_text<T>(
    display: &mut T,
    renderer: &FontRenderer,
    text: &str,
    right_x: i32,
    center_y: i32,
) where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = renderer.render_aligned(
        text,
        Point::new(right_x, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Right,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_divider<T>(display: &mut T, y: i32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let _ = Line::new(Point::new(40, y), Point::new(SCREEN_WIDTH - 40, y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
}

fn clear_region<T>(display: &mut T, x: i32, y: i32, width: u32, height: u32)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let style = PrimitiveStyle::with_fill(BinaryColor::Off);
    let _ = Rectangle::new(Point::new(x, y), Size::new(width, height))
        .into_styled(style)
        .draw(display);
}

fn pattern_seed(uptime_seconds: u32, time_sync: Option<TimeSyncState>, nonce: u32) -> u32 {
    let local_now = local_seconds_since_epoch(uptime_seconds, time_sync);
    let refresh_step = (local_now / REFRESH_INTERVAL_SECONDS as u64) as u32;
    refresh_step ^ refresh_step.rotate_left(13) ^ nonce.wrapping_mul(0x85EB_CA6B) ^ 0x9E37_79B9
}

fn next_visual_seed(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
) -> u32 {
    if *first_visual_seed_pending {
        *first_visual_seed_pending = false;
        return VISUAL_DEFAULT_SEED;
    }

    *pattern_nonce = pattern_nonce.wrapping_add(1);
    pattern_seed(uptime_seconds, time_sync, *pattern_nonce)
}

fn local_seconds_since_epoch(uptime_seconds: u32, time_sync: Option<TimeSyncState>) -> u64 {
    if let Some(sync) = time_sync {
        let elapsed = Instant::now()
            .saturating_duration_since(sync.sync_instant)
            .as_secs();
        let utc_now = sync.unix_epoch_utc_seconds.saturating_add(elapsed);
        (utc_now as i64 + (sync.tz_offset_minutes as i64) * 60).max(0) as u64
    } else {
        let monotonic = Instant::now().as_secs().min(u32::MAX as u64) as u32;
        monotonic.max(uptime_seconds) as u64
    }
}

fn background_alpha_50_mask(x: i32, y: i32, seed: u32) -> bool {
    let mixed =
        mix32(seed ^ (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B));
    (mixed as u8) < SUMINAGASHI_BG_ALPHA_50_THRESHOLD
}

fn sun_center_for_time(
    width: i32,
    height: i32,
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> Point {
    if SUN_FORCE_CENTER {
        return Point::new(width / 2, height / 2);
    }

    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as i64;
    let margin = (width / 12).clamp(24, 72);
    let left_x = margin;
    let right_x = (width - 1 - margin).max(left_x + 1);
    let horizon_y = (height * 83 / 100).clamp(0, height - 1);
    let arc_height = (height * 50 / 100).clamp(1, height - 1);
    let below_horizon_y = (horizon_y + height / 12).clamp(0, height - 1);

    let (x, y) = if seconds_of_day < SUNRISE_SECONDS_OF_DAY {
        // Pre-dawn: hold near left horizon, below the visible arc.
        (left_x, below_horizon_y)
    } else if seconds_of_day > SUNSET_SECONDS_OF_DAY {
        // Post-sunset: hold near right horizon, below the visible arc.
        (right_x, below_horizon_y)
    } else {
        let day_span = (SUNSET_SECONDS_OF_DAY - SUNRISE_SECONDS_OF_DAY).max(1);
        let t = (seconds_of_day - SUNRISE_SECONDS_OF_DAY).clamp(0, day_span);
        let x = left_x + (((right_x - left_x) as i64 * t) / day_span) as i32;

        // Daylight arc: lowest at sunrise/sunset, highest near noon.
        let u = t * 2 - day_span;
        let denom_sq = day_span * day_span;
        let profile = (denom_sq - u * u).max(0);
        let lift = ((arc_height as i64 * profile) / denom_sq) as i32;
        let y = (horizon_y - lift).clamp(0, height - 1);
        (x, y)
    };

    Point::new(x, y)
}

fn build_sun_params(seed: u32, center: Point) -> SumiSunParams {
    let mut state = mix32(seed ^ 0xA1C3_4D27);
    let mut params = SumiSunParams::default();
    params.center = center;
    params.radius_px = ((SUN_TARGET_DIAMETER_PX / 2) + rand_i32(&mut state, -3, 3)).max(10);
    params.edge_softness_px = SunFx::from_bits(rand_i32(&mut state, 45_875, 98_304)); // 0.7..1.5 px
    params.bleed_px = SunFx::from_bits(rand_i32(&mut state, 19_661, 98_304)); // 0.3..1.5 px
    params.dry_brush = SunFx::from_bits(rand_i32(&mut state, 9_000, 26_000)); // stronger macro highlight breakup
    params.completeness = SunFx::from_bits(65_536); // fully present
    params.completeness_softness = SunFx::from_bits(rand_i32(&mut state, 600, 1_800));
    params.completeness_warp = SunFx::from_bits(rand_i32(&mut state, 0, 600));
    params.completeness_rotation = SunFx::from_bits(rand_i32(&mut state, 0, 65_535));
    params.stroke_strength = SunFx::from_bits(rand_i32(&mut state, 24_000, 56_000));
    params.stroke_anisotropy = SunFx::from_bits(rand_i32(&mut state, 65_536, 196_608)); // 1.0..3.0
    params.ink_luma = SunFx::from_bits(rand_i32(&mut state, 0, 30_000)); // wider dark/light range
    params
}

fn rand_i32(state: &mut u32, min: i32, max: i32) -> i32 {
    if min >= max {
        return min;
    }
    let span = (max - min + 1) as u32;
    min + (next_rand_u32(state) % span) as i32
}

fn next_rand_u32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn mix32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x85EB_CA6B);
    v ^= v >> 13;
    v = v.wrapping_mul(0xC2B2_AE35);
    v ^ (v >> 16)
}

fn checksum8(bytes: &[u8]) -> u8 {
    let mut acc = 0x5Au8;
    for &byte in bytes {
        acc ^= byte.rotate_left(1);
    }
    acc
}

fn format_clock_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<12> {
    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as u32;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;

    let mut out = heapless::String::<12>::new();
    let _ = write!(&mut out, "{hours:02}:{minutes:02}");
    out
}

fn format_render_time_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<24> {
    let seconds_of_day = (local_seconds_since_epoch(uptime_seconds, time_sync) % 86_400) as u32;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;
    let seconds = seconds_of_day % 60;
    let mut out = heapless::String::<24>::new();
    let _ = write!(&mut out, "RENDER {hours:02}:{minutes:02}:{seconds:02}");
    out
}

fn format_uptime_text(uptime_seconds: u32) -> heapless::String<32> {
    let days = uptime_seconds / 86_400;
    let hours = (uptime_seconds / 3_600) % 24;
    let minutes = (uptime_seconds / 60) % 60;
    let mut out = heapless::String::<32>::new();
    let _ = write!(&mut out, "UPTIME {days}d {hours:02}h {minutes:02}m");
    out
}

fn format_sync_text(time_sync: Option<TimeSyncState>) -> heapless::String<32> {
    let mut out = heapless::String::<32>::new();
    if let Some(sync) = time_sync {
        let sign = if sync.tz_offset_minutes >= 0 {
            '+'
        } else {
            '-'
        };
        let abs = sync.tz_offset_minutes.unsigned_abs();
        let hours = abs / 60;
        let minutes = abs % 60;
        let _ = write!(&mut out, "SYNCED UTC{sign}{hours:02}:{minutes:02}");
    } else {
        let _ = write!(&mut out, "UNSYNCED");
    }
    out
}

fn format_battery_text(battery_percent: Option<u8>) -> heapless::String<16> {
    let mut out = heapless::String::<16>::new();
    if let Some(percent) = battery_percent {
        let _ = write!(&mut out, "BAT {percent:>3}%");
    } else {
        let _ = write!(&mut out, "BAT --%");
    }
    out
}

fn sample_battery_percent(display: &mut InkplateDriver) -> Option<u8> {
    let soc = display.fuel_gauge_soc().ok()?;
    if soc > 100 {
        return None;
    }
    Some(soc as u8)
}

fn trigger_backlight_cycle(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
) {
    *backlight_cycle_start = Some(Instant::now());
    apply_backlight_level(display, backlight_level, BACKLIGHT_MAX_BRIGHTNESS);
}

fn run_backlight_timeline(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
) {
    let Some(cycle_start) = *backlight_cycle_start else {
        return;
    };

    let elapsed_ms = Instant::now()
        .saturating_duration_since(cycle_start)
        .as_millis();
    let target_level = if elapsed_ms < BACKLIGHT_HOLD_MS {
        BACKLIGHT_MAX_BRIGHTNESS
    } else if elapsed_ms < BACKLIGHT_HOLD_MS + BACKLIGHT_FADE_MS {
        let fade_elapsed = elapsed_ms - BACKLIGHT_HOLD_MS;
        let fade_remaining = BACKLIGHT_FADE_MS.saturating_sub(fade_elapsed);
        ((BACKLIGHT_MAX_BRIGHTNESS as u64 * fade_remaining) / BACKLIGHT_FADE_MS) as u8
    } else {
        *backlight_cycle_start = None;
        0
    };

    apply_backlight_level(display, backlight_level, target_level);
}

fn apply_backlight_level(display: &mut InkplateDriver, current_level: &mut u8, next_level: u8) {
    if *current_level == next_level {
        return;
    }

    let _ = display.set_brightness(next_level);
    if next_level == 0 {
        let _ = display.frontlight_off();
    }
    *current_level = next_level;
}

struct PanelPinHold<'d> {
    _cl: Output<'d>,
    _le: Output<'d>,
    _d0: Output<'d>,
    _d1: Output<'d>,
    _d2: Output<'d>,
    _d3: Output<'d>,
    _d4: Output<'d>,
    _d5: Output<'d>,
    _d6: Output<'d>,
    _d7: Output<'d>,
    _ckv: Output<'d>,
    _sph: Output<'d>,
}
