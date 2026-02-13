#![no_std]
#![no_main]

mod pirata_clock_font;

use core::fmt::Write;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Instant, Ticker};
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
};
use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout},
    time::{Duration as HalDuration, Rate},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
    Async,
};
use meditamer::{
    inkplate_hal::InkplateHal,
    platform::{BusyDelay, HalI2c},
};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    FontRenderer,
};

const SCREEN_WIDTH: i32 = 600;
const REFRESH_INTERVAL_SECONDS: u32 = 300;
const BATTERY_INTERVAL_SECONDS: u32 = 300;
const FULL_REFRESH_EVERY_N_UPDATES: u32 = 20;
const UART_BAUD: u32 = 115_200;
const TIMESET_CMD_BUF_LEN: usize = 64;
const TITLE_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const META_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const BATTERY_FONT: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
const TITLE_Y: i32 = 44;
const BATTERY_TEXT_Y: i32 = 44;
const BATTERY_TEXT_RIGHT_X: i32 = SCREEN_WIDTH - 16;
const DIVIDER_TOP_Y: i32 = 76;
const DIVIDER_BOTTOM_Y: i32 = 466;
const CLOCK_Y: i32 = 280;
const SYNC_Y: i32 = 514;
const UPTIME_Y: i32 = 552;
const CLOCK_REGION_TOP: i32 = 96;
const CLOCK_REGION_HEIGHT: u32 = 340;
const META_REGION_TOP: i32 = 488;
const META_REGION_HEIGHT: u32 = 96;
const BATTERY_REGION_LEFT: i32 = 430;
const BATTERY_REGION_TOP: i32 = 14;
const BATTERY_REGION_WIDTH: u32 = 170;
const BATTERY_REGION_HEIGHT: u32 = 54;
const UI_TICK_MS: u64 = 50;
const IMU_INIT_RETRY_MS: u64 = 2_000;
const BACKLIGHT_MAX_BRIGHTNESS: u8 = 63;
const BACKLIGHT_HOLD_MS: u64 = 3_000;
const BACKLIGHT_FADE_MS: u64 = 2_000;
const TRIPLE_TAP_MIN_MS: u64 = 55;
const TRIPLE_TAP_MAX_MS: u64 = 700;
const TRIPLE_TAP_LAST_MAX_MS: u64 = 900;
const TRIPLE_TAP_COOLDOWN_MS: u64 = 900;
const TAP_DEBOUNCE_MS: u64 = 110;
const TAP_SEQ_FINISH_DEBOUNCE_MS: u64 = 55;
const TAP_JERK_L1_MIN: i32 = 900;
const TAP_JERK_STRONG_L1_MIN: i32 = 2_600;
const TAP_JERK_SEQ_CONT_MIN: i32 = 650;
const TAP_PREV_JERK_QUIET_MAX: i32 = 1_100;
const TAP_GYRO_L1_SWING_MAX: i32 = 14_000;
const TAP_GYRO_VETO_HOLD_MS: u64 = 180;
const TAP_TRACE_ENABLED: bool = true;
const TAP_TRACE_SAMPLE_MS: u64 = 25;
const TAP_TRACE_AUX_SAMPLE_MS: u64 = 250;
const LSM6_TAP_SRC_Z_BIT: u8 = 0x01;
const LSM6_TAP_SRC_Y_BIT: u8 = 0x02;
const LSM6_TAP_SRC_X_BIT: u8 = 0x04;
const LSM6_TAP_SRC_SINGLE_TAP_BIT: u8 = 0x20;
const LSM6_TAP_SRC_TAP_EVENT_BIT: u8 = 0x40;
static APP_EVENTS: Channel<CriticalSectionRawMutex, AppEvent, 4> = Channel::new();
static TAP_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TapTraceSample, 32> = Channel::new();

type InkplateDriver = InkplateHal<HalI2c<'static>, BusyDelay>;
type SerialUart = Uart<'static, Async>;

#[derive(Clone, Copy)]
enum AppEvent {
    Refresh { uptime_seconds: u32 },
    BatteryTick,
    TimeSync(TimeSyncCommand),
}

#[derive(Clone, Copy)]
struct TimeSyncCommand {
    unix_epoch_utc_seconds: u64,
    tz_offset_minutes: i32,
}

#[derive(Clone, Copy)]
struct TimeSyncState {
    unix_epoch_utc_seconds: u64,
    tz_offset_minutes: i32,
    sync_instant: Instant,
}

#[derive(Clone, Copy)]
struct TapTraceSample {
    t_ms: u64,
    tap_src: u8,
    seq_count: u8,
    tap_candidate: u8,
    cand_src: u8,
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

    let display_context = DisplayContext {
        inkplate,
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
                b"tap_trace,ms,tap_src,seq,cand,csrc,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
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
                    if let Some(cmd) = parse_timeset_command(&line_buf[..line_len]) {
                        APP_EVENTS.send(AppEvent::TimeSync(cmd)).await;
                        let _ = uart.write_async(b"TIMESET OK\r\n").await;
                    } else {
                        let _ = uart.write_async(b"TIMESET ERR\r\n").await;
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
        "tap_trace,{},{:#04x},{},{},{:#04x},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        sample.t_ms,
        sample.tap_src,
        sample.seq_count,
        sample.tap_candidate,
        sample.cand_src,
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

fn accel_l1_jerk_and_axis(
    prev: Option<(i16, i16, i16)>,
    current: (i16, i16, i16),
) -> (i32, u8) {
    let Some((px, py, pz)) = prev else {
        return (0, 0);
    };

    let dx = (current.0 as i32 - px as i32).abs();
    let dy = (current.1 as i32 - py as i32).abs();
    let dz = (current.2 as i32 - pz as i32).abs();
    let total = dx + dy + dz;

    let axis = if dx >= dy && dx >= dz {
        LSM6_TAP_SRC_X_BIT
    } else if dy >= dx && dy >= dz {
        LSM6_TAP_SRC_Y_BIT
    } else {
        LSM6_TAP_SRC_Z_BIT
    };

    (total, axis)
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
    let mut screen_initialized = false;
    let mut imu_double_tap_ready = false;
    let mut imu_retry_at = Instant::now();
    let mut tap_sequence: Option<(u8, Instant, u8)> = None;
    let mut last_triple_tap_at: Option<Instant> = None;
    let mut last_tap_candidate_at: Option<Instant> = None;
    let mut last_tap_accel: Option<(i16, i16, i16)> = None;
    let mut last_tap_candidate_flag = 0u8;
    let mut last_tap_candidate_src = 0u8;
    let mut last_tap_jerk_l1 = 0i32;
    let mut last_tap_motion_veto = 0u8;
    let mut last_tap_gyro_l1 = 0i32;
    let mut last_big_gyro_at: Option<Instant> = None;
    let mut last_detect_tap_src = 0u8;
    let mut last_detect_int1 = 0u8;
    let trace_epoch = Instant::now();
    let mut tap_trace_next_sample_at = Instant::now();
    let mut tap_trace_aux_next_sample_at = Instant::now();
    let mut tap_trace_power_good: i16 = -1;
    let mut backlight_cycle_start: Option<Instant> = None;
    let mut backlight_level = 0u8;

    loop {
        match with_timeout(Duration::from_millis(UI_TICK_MS), APP_EVENTS.receive()).await {
            Ok(event) => match event {
                AppEvent::Refresh { uptime_seconds } => {
                    last_uptime_seconds = uptime_seconds;
                    let do_full_refresh =
                        !screen_initialized || update_count % FULL_REFRESH_EVERY_N_UPDATES == 0;
                    render_clock_update(
                        &mut context.inkplate,
                        uptime_seconds,
                        time_sync,
                        battery_percent,
                        do_full_refresh,
                    );
                    screen_initialized = true;
                    update_count = update_count.wrapping_add(1);
                }
                AppEvent::BatteryTick => {
                    if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
                        battery_percent = Some(sampled_percent);
                    }

                    if screen_initialized {
                        render_battery_update(&mut context.inkplate, battery_percent);
                    } else {
                        render_clock_update(
                            &mut context.inkplate,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            true,
                        );
                        screen_initialized = true;
                    }
                }
                AppEvent::TimeSync(cmd) => {
                    time_sync = Some(TimeSyncState {
                        unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
                        tz_offset_minutes: cmd.tz_offset_minutes,
                        sync_instant: Instant::now(),
                    });
                    // Force a full redraw on sync so status/time change is always visible immediately.
                    render_clock_update(
                        &mut context.inkplate,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        true,
                    );
                    screen_initialized = true;
                }
            },
            Err(_) => {}
        }

        if !imu_double_tap_ready && Instant::now() >= imu_retry_at {
            imu_double_tap_ready = context.inkplate.lsm6ds3_init_double_tap().unwrap_or(false);
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
                    last_detect_tap_src = tap_src;
                    last_detect_int1 = if int1 { 1 } else { 0 };
                    let gyro_l1 =
                        i32::from(gx).abs() + i32::from(gy).abs() + i32::from(gz).abs();
                    last_tap_gyro_l1 = gyro_l1;
                    if gyro_l1 >= TAP_GYRO_L1_SWING_MAX {
                        last_big_gyro_at = Some(now);
                    }
                    let gyro_veto_active = last_big_gyro_at.is_some_and(|last| {
                        now.saturating_duration_since(last).as_millis() < TAP_GYRO_VETO_HOLD_MS
                    });
                    last_tap_motion_veto = if gyro_veto_active { 1 } else { 0 };

                    let tap_axis_mask =
                        tap_src & (LSM6_TAP_SRC_X_BIT | LSM6_TAP_SRC_Y_BIT | LSM6_TAP_SRC_Z_BIT);
                    let has_axis_tap = tap_axis_mask != 0;
                    let has_single_tap = (tap_src & LSM6_TAP_SRC_SINGLE_TAP_BIT) != 0;
                    let has_tap_event = (tap_src & LSM6_TAP_SRC_TAP_EVENT_BIT) != 0;
                    let prev_jerk_l1 = last_tap_jerk_l1;
                    let (jerk_l1, jerk_axis) = accel_l1_jerk_and_axis(last_tap_accel, (ax, ay, az));
                    last_tap_accel = Some((ax, ay, az));
                    last_tap_jerk_l1 = jerk_l1;
                    let candidate_axis = if has_axis_tap { tap_axis_mask } else { jerk_axis };
                    let (seq_count, seq_axis) = tap_sequence
                        .map(|(count, _, axis)| (count, axis))
                        .unwrap_or((0, 0));
                    let axis_matches_sequence =
                        seq_axis == 0 || candidate_axis == 0 || (seq_axis & candidate_axis) != 0;

                    let moderate_jerk = jerk_l1 >= TAP_JERK_L1_MIN;
                    let strong_jerk = jerk_l1 >= TAP_JERK_STRONG_L1_MIN;
                    let src_axis = has_axis_tap;
                    let src_single = has_single_tap;
                    let src_int1 = int1;
                    let src_tap_event = has_tap_event;
                    let src_jerk_axis = has_axis_tap && moderate_jerk;
                    // Fallback for weak-side taps where tap_src bits are flaky: strong impulse on quiet baseline.
                    let src_jerk_only =
                        !has_axis_tap && strong_jerk && prev_jerk_l1 <= TAP_PREV_JERK_QUIET_MAX;
                    // If 2 taps already passed, allow weaker 3rd impulse when axis remains consistent.
                    let src_seq_finish_assist =
                        seq_count >= 2 && axis_matches_sequence && jerk_l1 >= TAP_JERK_SEQ_CONT_MIN;
                    let fused_tap_candidate = src_jerk_only
                        || src_seq_finish_assist
                        || (src_axis && (src_single || src_int1 || src_tap_event || src_jerk_axis));

                    let mut cand_src = 0u8;
                    if src_axis {
                        cand_src |= 0x01;
                    }
                    if src_single {
                        cand_src |= 0x02;
                    }
                    if src_int1 {
                        cand_src |= 0x04;
                    }
                    if src_tap_event {
                        cand_src |= 0x08;
                    }
                    if src_jerk_axis {
                        cand_src |= 0x10;
                    }
                    if src_jerk_only {
                        cand_src |= 0x20;
                    }
                    if gyro_veto_active {
                        cand_src |= 0x40;
                    }
                    if src_seq_finish_assist {
                        cand_src |= 0x80;
                    }

                    let debounce_window_ms = if src_seq_finish_assist {
                        TAP_SEQ_FINISH_DEBOUNCE_MS
                    } else {
                        TAP_DEBOUNCE_MS
                    };
                    let debounced = last_tap_candidate_at.is_some_and(|last| {
                        now.saturating_duration_since(last).as_millis() < debounce_window_ms
                    });
                    // Veto only when candidate is motion-derived; keep explicit tap-axis/single events.
                    let motion_only_candidate = src_jerk_only || src_seq_finish_assist;
                    let veto_candidate = gyro_veto_active && motion_only_candidate;
                    let saw_tap_event = fused_tap_candidate && !debounced && !veto_candidate;
                    last_tap_candidate_flag = if saw_tap_event { 1 } else { 0 };
                    last_tap_candidate_src = cand_src;

                    let in_cooldown = last_triple_tap_at.is_some_and(|last| {
                        now.saturating_duration_since(last).as_millis() < TRIPLE_TAP_COOLDOWN_MS
                    });

                    if tap_sequence.is_some_and(|(count, last_at, _)| {
                        let max_gap = if count >= 2 {
                            TRIPLE_TAP_LAST_MAX_MS
                        } else {
                            TRIPLE_TAP_MAX_MS
                        };
                        now.saturating_duration_since(last_at).as_millis() > max_gap
                    }) {
                        tap_sequence = None;
                    }

                    if saw_tap_event {
                        last_tap_candidate_at = Some(now);
                        if let Some((count, last_at, axis)) = tap_sequence {
                            if !axis_matches_sequence {
                                tap_sequence = Some((1, now, candidate_axis));
                                continue;
                            }
                            let dt = now.saturating_duration_since(last_at).as_millis();
                            let max_gap = if count >= 2 {
                                TRIPLE_TAP_LAST_MAX_MS
                            } else {
                                TRIPLE_TAP_MAX_MS
                            };
                            if (TRIPLE_TAP_MIN_MS..=max_gap).contains(&dt) {
                                let next_count = count.saturating_add(1);
                                if next_count >= 3 {
                                    tap_sequence = None;
                                    if !in_cooldown {
                                        last_triple_tap_at = Some(now);
                                        trigger_backlight_cycle(
                                            &mut context.inkplate,
                                            &mut backlight_cycle_start,
                                            &mut backlight_level,
                                        );
                                    }
                                } else {
                                    let next_axis = if axis != 0 { axis } else { candidate_axis };
                                    tap_sequence = Some((next_count, now, next_axis));
                                }
                            } else {
                                tap_sequence = Some((1, now, candidate_axis));
                            }
                        } else {
                            tap_sequence = Some((1, now, candidate_axis));
                        }
                    }
                }
                _ => {
                    imu_double_tap_ready = false;
                    tap_sequence = None;
                    last_triple_tap_at = None;
                    last_tap_candidate_at = None;
                    last_tap_accel = None;
                    last_tap_candidate_flag = 0;
                    last_tap_candidate_src = 0;
                    last_tap_jerk_l1 = 0;
                    last_tap_motion_veto = 0;
                    last_tap_gyro_l1 = 0;
                    last_big_gyro_at = None;
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
                        let seq_count = tap_sequence.map(|(count, _, _)| count).unwrap_or(0);
                        let battery_percent_i16 = battery_percent.map_or(-1, i16::from);
                        let t_ms = now.saturating_duration_since(trace_epoch).as_millis();
                        let sample = TapTraceSample {
                            t_ms,
                            tap_src: last_detect_tap_src,
                            seq_count,
                            tap_candidate: last_tap_candidate_flag,
                            cand_src: last_tap_candidate_src,
                            jerk_l1: last_tap_jerk_l1,
                            motion_veto: last_tap_motion_veto,
                            gyro_l1: last_tap_gyro_l1,
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
    let _ = display.display_bw_partial(false);
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
    let style = PrimitiveStyle::with_fill(BinaryColor::Off);
    let _ = Rectangle::new(
        Point::new(0, CLOCK_REGION_TOP),
        Size::new(SCREEN_WIDTH as u32, CLOCK_REGION_HEIGHT),
    )
    .into_styled(style)
    .draw(display);
    let _ = Rectangle::new(
        Point::new(0, META_REGION_TOP),
        Size::new(SCREEN_WIDTH as u32, META_REGION_HEIGHT),
    )
    .into_styled(style)
    .draw(display);
}

fn erase_battery_region<T>(display: &mut T)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let style = PrimitiveStyle::with_fill(BinaryColor::Off);
    let _ = Rectangle::new(
        Point::new(BATTERY_REGION_LEFT, BATTERY_REGION_TOP),
        Size::new(BATTERY_REGION_WIDTH, BATTERY_REGION_HEIGHT),
    )
    .into_styled(style)
    .draw(display);
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

fn format_clock_text(
    uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
) -> heapless::String<12> {
    let seconds_of_day = if let Some(sync) = time_sync {
        let elapsed = Instant::now()
            .saturating_duration_since(sync.sync_instant)
            .as_secs();
        let utc_now = sync.unix_epoch_utc_seconds.saturating_add(elapsed);
        let local_now = utc_now as i64 + (sync.tz_offset_minutes as i64) * 60;
        local_now.rem_euclid(86_400) as u32
    } else {
        uptime_seconds % 86_400
    };
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day / 60) % 60;

    let mut out = heapless::String::<12>::new();
    let _ = write!(&mut out, "{hours:02}:{minutes:02}");
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
