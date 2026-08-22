//! Bring-up for the Waveshare ESP32-S3-RLCD-4.2, and the first evidence that
//! ADR-0015's platform crates are not tied to the chip they grew up on.
//!
//! Deliberately not a product. It boots the RTOS, drives the platform layer
//! through a few real operations, and reports over the S3's native
//! USB-Serial-JTAG. What it proves is portability: `console`, `shell`, and
//! `arbitration` compile and run on Xtensa LX7 with no source changes, having
//! only ever run on the LX6 Inkplate.
//!
//! Medinote's UI will grow from here; the panel driver does not exist yet.

#![no_std]
#![no_main]

mod jtag_rx;
mod panel;
mod ui;

use board::{Panel, RefreshMode};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_backtrace as _;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::ram;
use esp_hal::rtc_cntl::sleep::TimerWakeupSource;
use esp_hal::rtc_cntl::Rtc;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::system::SleepSource;
use esp_hal::time::Duration as HalDuration;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use static_cell::StaticCell;

use arbitration::claim::{self, Ownership};
use shell::registry::SurfaceRegistry;
use shell::types::{
    ProviderId, RefreshHint, SurfaceCapabilities, SurfaceRef, SurfaceRole, SurfaceSpec,
};

/// Matches Meditamer's shell sizing closely enough to exercise the same code
/// paths; Medinote will pick its own once it has screens.
const PROVIDER_CAPACITY: usize = 4;
const SURFACE_CAPACITY: usize = 8;

/// Deep sleep between samples instead of idling in the executor. Set false to
/// get the always-on build back for an A/B.
const DEEP_SLEEP: bool = false;

/// How long the SoC stays down between samples.
/// Measured warm-wake cost is ~1.28s (0.8s CDC settle + 0.24s panel init +
/// 0.07s sensor + 0.14s flush), so at 60s this is a 98% duty-cycle saving
/// against the always-on build. There is no console on a battery build, so the
/// 0.8s settle is pure diagnostic overhead there and the real cost is closer
/// to 0.5s -- worth cutting once this build stops needing a serial log.
const SLEEP_INTERVAL_S: u64 = 60;

/// Skip the panel's 240ms init on a timer wake.
///
/// False, because it was tried and the glass went blank. The premise was that
/// only the SoC powers down -- the 3V3 rail stays up, so the ST7305 should keep
/// its configuration and its GRAM, making the 240ms of fixed reset and
/// sleep-out delay pure waste on every wake.
///
/// It does not survive. RST is on GPIO41, and the S3's RTC-capable pads stop at
/// GPIO21, so nothing holds that line while the SoC is down: the controller
/// sees a reset, drops its configuration and clears GRAM, and the writes that
/// follow land on a panel that is no longer listening.
///
/// Reclaiming the 240ms needs the pad held through sleep, which on a non-RTC
/// pin means digital pad hold rather than the RTC path -- worth doing, but not
/// something to leave on unverified, since the failure mode is a dark screen.
const RETAIN_PANEL_ACROSS_SLEEP: bool = false;

/// The value this build stamps into the RTC the one time it is genuinely
/// unprovisioned -- a dead RTC backup cell, or the very first flash ever.
/// Every other boot reads the RTC first and, if it already reports a valid
/// running time, leaves it alone: writing this constant unconditionally on
/// every reflash was the bug that made the on-screen clock jump back to
/// 14:00 (this epoch, at the fixed UTC+2 offset below) each time the board
/// was reflashed, discarding however much real time the PCF85063A -- which
/// keeps ticking across a reflash; only the ESP32-S3 resets -- had correctly
/// accumulated in between.
const KNOWN_EPOCH: u32 = 1_787_314_589; // 2026-08-21T12:16:29Z, compensated +22s for cold-boot lag
const OFFSET_MINUTES: i16 = 120; // UTC+2

/// How long to let the USB CDC host re-enumerate before trusting the console.
///
/// Diagnostic cost only: a battery build has no console and pays none of it.
/// It is excluded from the reported active time for exactly that reason.
const CONSOLE_SETTLE_MS: u32 = 0;

/// Stay awake this long after the frame is on the glass, purely so the USB
/// console can be attached and read.
///
/// A wake is otherwise ~1.3s, of which the useful part is a few hundred
/// milliseconds -- too narrow to reliably catch a monitor on, because the CDC
/// device only exists while the chip is up. This is applied strictly after the
/// active time is measured, so it inflates the observed window without
/// disturbing the number being reported. Zero for a battery build.
const DIAGNOSTIC_HOLD_MS: u32 = 0;

/// Zero, deliberately: this was 800ms of every wake spent waiting for a USB
/// host that is not there on a battery-powered device. `esp-println`'s
/// jtag-serial backend does not need it to be safe -- a write into a full FIFO
/// times out after a bounded spin (tens of microseconds) and gives up for the
/// rest of that wake rather than blocking forever, so the `console::println!`
/// calls below are left in place rather than stripped out. If a host *is*
/// attached when a wake happens, they still print; this only removes the
/// unconditional wait for one that might not be.

/// State that has to outlive the SoC powering down. RTC fast RAM survives deep
/// sleep on the S3 (8K of it, alongside 8K of RTC slow); ordinary SRAM does not.
///
/// The timings live here because the cycle they describe finishes by powering
/// the chip down -- there is no console attached at that point, and none until
/// the host re-enumerates on the next wake. So each wake reports the previous
/// wake's cost.
#[ram(unstable(rtc_fast, persistent))]
static mut WAKE_COUNT: u32 = 0;
#[ram(unstable(rtc_fast, persistent))]
static mut LAST_ACTIVE_MS: u32 = 0;
#[ram(unstable(rtc_fast, persistent))]
static mut LAST_SENSOR_MS: u32 = 0;
#[ram(unstable(rtc_fast, persistent))]
static mut LAST_FLUSH_MS: u32 = 0;

/// A ring of recent cycle costs, dumped on the next attach.
///
/// Watching a wake live is close to impossible over native USB-Serial-JTAG: the
/// CDC device only exists while the chip is up, so the port appears and
/// vanishes with every cycle and the monitor is racing a window it usually
/// loses. Recording into RTC RAM sidesteps the race -- let the board run
/// unattended, then reset it and read the history back.
const HISTORY_LEN: usize = 16;
#[ram(unstable(rtc_fast, persistent))]
static mut HISTORY_ACTIVE_MS: [u32; HISTORY_LEN] = [0; HISTORY_LEN];
#[ram(unstable(rtc_fast, persistent))]
static mut HISTORY_AT: u32 = 0;

/// Marks RTC RAM as ours. `persistent` survives deep sleep *and* resets, but is
/// never zeroed at power-on, so without a marker the first cold boot reads
/// whatever the cells happened to hold -- which is how the wake counter first
/// printed as 3809139387.
const RTC_MAGIC: u32 = 0x4D45_4431;
#[ram(unstable(rtc_fast, persistent))]
static mut RTC_MARKER: u32 = 0;

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz));

    // Boot-phase timing. Deep sleep makes the whole of `main` the wake cost, so
    // what each phase costs decides the achievable sample cadence -- measured
    // here rather than estimated. Timestamps are collected and printed once at
    // the end, because the console write is itself milliseconds over USB CDC.
    let t0 = esp_hal::time::Instant::now();

    // A timer wake means the diagnostics below have already run once and the
    // panel is already configured; everything that is not the sample itself is
    // dead weight on the battery.
    let warm = matches!(esp_hal::rtc_cntl::wakeup_cause(), SleepSource::Timer);

    // Initialise RTC RAM only when it is not already ours. A reset from the
    // flasher is not a timer wake, but it does preserve these cells, so keying
    // this off `warm` would wipe the history exactly when it is about to be
    // read back.
    let rtc_ram_valid = unsafe { core::ptr::read_volatile(&raw const RTC_MARKER) } == RTC_MAGIC;
    if !rtc_ram_valid {
        unsafe {
            core::ptr::write_volatile(&raw mut WAKE_COUNT, 0);
            core::ptr::write_volatile(&raw mut HISTORY_AT, 0);
            core::ptr::write_volatile(&raw mut HISTORY_ACTIVE_MS, [0; HISTORY_LEN]);
            core::ptr::write_volatile(&raw mut RTC_MARKER, RTC_MAGIC);
        }
    }

    // esp-println's jtag-serial backend drops output when the host has not
    // finished attaching; give the CDC port time to enumerate after the reset
    // that got us here.
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);

    let t_cdc = esp_hal::time::Instant::now();
    console::println!("BOARD_BOOT board=waveshare-rlcd42 chip=esp32s3");
    console::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);
    console::println!("RTOS_STARTED core=0");
    let t_rtos = esp_hal::time::Instant::now();

    exercise_arbitration();
    exercise_shell();
    console::println!("PLATFORM_OK crates=console,shell,arbitration chip=esp32s3");

    // SCK=11, MOSI=12, DC=5, CS=40, RST=41 per Waveshare's user_config.h.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(24))
            .with_mode(SpiMode::_0),
    )
    .expect("spi2")
    .with_sck(peripherals.GPIO11)
    .with_mosi(peripherals.GPIO12);
    let output = OutputConfig::default();
    static FRAMEBUFFER: StaticCell<[u8; panel::FRAMEBUFFER_BYTES]> = StaticCell::new();
    let framebuffer = FRAMEBUFFER.init([0u8; panel::FRAMEBUFFER_BYTES]);
    let mut display = panel::St7305::new(
        framebuffer,
        spi,
        Output::new(peripherals.GPIO5, Level::Low, output),
        Output::new(peripherals.GPIO40, Level::High, output),
        Output::new(peripherals.GPIO41, Level::High, output),
    );
    let t_pre_panel = esp_hal::time::Instant::now();
    let panel_reinitialised = !(warm && RETAIN_PANEL_ACROSS_SLEEP);
    if panel_reinitialised {
        display.init();
    }
    let t_panel = esp_hal::time::Instant::now();
    console::println!(
        "PANEL_INIT controller=st7305 {}x{}",
        panel::WIDTH,
        panel::HEIGHT
    );

    // The glyph test, the trait probe and the power-mode sweep are all
    // bring-up diagnostics. They cost 21 seconds between them, which is fine
    // once at cold boot and absurd on every 60-second wake.
    if !warm {
        draw_border(display.framebuffer_mut());

        let panel: &mut dyn Panel = &mut display;

        // Draw a large blocky "F" through `blit_l8`, the trait's real entry point.
        // An F has no symmetry in either axis, so a mirror, a flip and a transpose
        // are each obvious at a glance -- which a border and a diagonal are not.
        //
        //   x=60  100        240
        //   +-----+-----------+   y=60
        //   |#####|###########|
        //   |#####+-----------+   y=100
        //   |#####|
        //   |#####+--------+      y=180
        //   |#####|########|
        //   |#####+--------+      y=220   (mid bar ends at x=200)
        //   |#####|
        //   +-----+               y=340
        //
        // Uniform ink, so one buffer serves every rectangle.
        static INK: StaticCell<[u8; 40 * 280]> = StaticCell::new();
        let ink = INK.init([0x00u8; 40 * 280]);

        let strokes = [
            (60, 40, 100, 260),  // stem
            (60, 40, 280, 80),   // top bar
            (60, 130, 220, 170), // middle bar
        ];
        let mut all_ok = true;
        for (x1, y1, x2, y2) in strokes {
            let ok = unsafe {
                panel.blit_l8(
                    board::DirtyArea {
                        x1,
                        y1,
                        x2: x2 - 1,
                        y2: y2 - 1,
                    },
                    ink.as_ptr(),
                )
            };
            all_ok &= ok;
        }

        // A rectangle straddling the right edge must clip rather than overrun. If
        // it renders on the left instead, the X axis is mirrored.
        let clipped = unsafe {
            panel.blit_l8(
                board::DirtyArea {
                    x1: panel::WIDTH as i32 - 10,
                    y1: 270,
                    x2: panel::WIDTH as i32 + 29,
                    y2: 289,
                },
                ink.as_ptr(),
            )
        };

        let geometry = panel.geometry();
        let partial_supported = panel.supports(RefreshMode::Partial);

        // Full first: the glass holds unknown content at boot, so the whole surface
        // has to be written before a partial update means anything.
        let full_ok = panel.refresh(RefreshMode::Full).is_ok();
        let full_bytes = panel::last_flush_bytes();

        // Then a partial with nothing dirty, which must send nothing at all rather
        // than quietly repeating the frame.
        let partial_ok = panel.refresh(RefreshMode::Partial).is_ok();
        let idle_bytes = panel::last_flush_bytes();

        console::println!(
        "PANEL_TRAIT {}x{} glyph=F strokes_ok={} clip_ok={} full_ok={} full_bytes={} partial_supported={} partial_ok={} idle_bytes={}",
        geometry.width,
        geometry.height,
        all_ok,
        clipped,
        full_ok,
        full_bytes,
        partial_supported,
        partial_ok,
        idle_bytes
    );
        assert!(all_ok && clipped && full_ok && partial_supported && partial_ok);
        assert!(full_bytes == panel::FRAMEBUFFER_BYTES);
        // An empty dirty box must not touch the panel; `last_flush_bytes` is left
        // at the previous value because no flush happened.
        assert!(idle_bytes == full_bytes);
    }

    // Before LVGL takes the panel: measure what the two power modes actually do.
    // The TE pin pulses once per panel frame, so its frequency is the panel's
    // real update rate -- which settles which of 0x38/0x39 is high power
    // without a current meter, and shows how fast the glass can actually go.
    let te = Input::new(peripherals.GPIO6, InputConfig::default());
    let t_pre_bench = esp_hal::time::Instant::now();
    if !warm {
        benchmark_power_modes(&mut display, &te);
    }
    let t_bench = esp_hal::time::Instant::now();

    // Hand the panel to LVGL. The display outlives the UI, so it is promoted to
    // 'static rather than borrowed across the executor.
    static DISPLAY: StaticCell<panel::St7305<'static>> = StaticCell::new();
    let display: &'static mut panel::St7305<'static> = DISPLAY.init(display);
    unsafe { ui::set_active_panel(display) };
    let lv_display = unsafe { ui::init(panel::WIDTH as i32, panel::HEIGHT as i32) };
    let t_lvgl = esp_hal::time::Instant::now();
    console::println!("LVGL_INIT {}x{} color=L8", panel::WIDTH, panel::HEIGHT);

    // The previous cycle ended by powering the chip down, so this is the first
    // moment it can be reported.
    let wake_count = unsafe { core::ptr::read_volatile(&raw const WAKE_COUNT) };
    if warm {
        console::println!(
            "WAKE n={} cause=timer panel_reinit={} prev_active_ms={} prev_sensor_ms={} prev_flush_ms={}",
            wake_count,
            panel_reinitialised,
            unsafe { core::ptr::read_volatile(&raw const LAST_ACTIVE_MS) },
            unsafe { core::ptr::read_volatile(&raw const LAST_SENSOR_MS) },
            unsafe { core::ptr::read_volatile(&raw const LAST_FLUSH_MS) }
        );
    } else {
        console::println!("WAKE n={} cause=cold panel_reinit=true", wake_count);
    }

    // Everything the board did while nobody was attached.
    let history_at = unsafe { core::ptr::read_volatile(&raw const HISTORY_AT) } as usize;
    let history = unsafe { core::ptr::read_volatile(&raw const HISTORY_ACTIVE_MS) };
    let recorded = (history_at).min(HISTORY_LEN);
    console::println!(
        "HISTORY entries={} (oldest first, active_ms per wake)",
        recorded
    );
    for index in 0..recorded {
        let slot = if history_at > HISTORY_LEN {
            (history_at - recorded + index) % HISTORY_LEN
        } else {
            index
        };
        console::println!("  HIST[{}] active_ms={}", index, history[slot]);
    }
    console::println!(
        "BOOT_PHASES_MS cdc_settle={} to_rtos={} platform_probes={} panel_init={} glyph_test={} benchmark={} lvgl={} total={}",
        (t_cdc - t0).as_millis(),
        (t_rtos - t_cdc).as_millis(),
        (t_pre_panel - t_rtos).as_millis(),
        (t_panel - t_pre_panel).as_millis(),
        (t_pre_bench - t_panel).as_millis(),
        (t_bench - t_pre_bench).as_millis(),
        (t_lvgl - t_bench).as_millis(),
        (t_lvgl - t0).as_millis()
    );

    // A software timeout matters more than the frequency: without one, a device
    // that never ACKs hangs the transaction forever, which reads on the console
    // as the board simply stopping. Meditamer sets the same 40ms.
    let i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default()
            .with_frequency(Rate::from_khz(100))
            .with_software_timeout(SoftwareTimeout::Transaction(HalDuration::from_millis(40))),
    )
    .expect("i2c0")
    .with_sda(peripherals.GPIO13)
    .with_scl(peripherals.GPIO14)
    .into_async();

    // The RTC driver is embedded-hal-async, so the board needs a running
    // executor rather than the blocking loop this used to end in.
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    // One bus, two devices. Each task borrows it per transaction rather than
    // owning it, which is what lets the sensor and the clock coexist.
    static I2C_BUS: StaticCell<Mutex<CriticalSectionRawMutex, I2c<'static, esp_hal::Async>>> =
        StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(i2c));

    let rtc = Rtc::new(peripherals.LPWR);

    executor.run(|spawner| {
        if DEEP_SLEEP {
            spawner.spawn(
                cycle_task(
                    I2cDevice::new(i2c_bus),
                    I2cDevice::new(i2c_bus),
                    rtc,
                    peripherals.GPIO4,
                    peripherals.ADC1,
                    t0,
                )
                .unwrap(),
            );
        } else {
            // `clock_task` used to run alongside this to prove the RTC
            // round-trip works; that is now `sensor_task`'s job too (with the
            // read-before-write fix above), and having both touch the RTC
            // concurrently is redundant rather than harmless -- the two would
            // race to decide whether the clock was already provisioned.
            spawner.spawn(
                sensor_task(
                    I2cDevice::new(i2c_bus),
                    I2cDevice::new(i2c_bus),
                    peripherals.GPIO4,
                    peripherals.ADC1,
                )
                .unwrap(),
            );
            spawner.spawn(ui_task(lv_display).unwrap());
        }
    });
}

/// One sample, then power the SoC down until the next one.
///
/// The whole of `main` is the wake cost under deep sleep, so this is deliberately
/// the shortest path from reset to a number on the glass: no tickers, no idle
/// backstop, no second pass. It ends in `sleep_deep`, which does not return.
#[embassy_executor::task]
#[allow(clippy::too_many_arguments)]
async fn cycle_task(
    sensor_i2c: SharedI2c,
    rtc_i2c: SharedI2c,
    mut rtc: Rtc<'static>,
    battery_pin: esp_hal::peripherals::GPIO4<'static>,
    adc1: esp_hal::peripherals::ADC1<'static>,
    t0: esp_hal::time::Instant,
) {
    /// Waveshare's `SHTC3_PETP_VOL`, in millidegrees.
    const SELF_HEATING_MC: i32 = 4_000;

    // Same registry round-trip the always-on path does, so the ADR-0015
    // evidence survives the mode change: the surface reaching the glass is one
    // the shell resolved, not a layout hardcoded here.
    let mut registry: SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY> = SurfaceRegistry::new();
    let token = registry
        .register_provider(
            ProviderId(7),
            &[SurfaceSpec::new(
                1,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Content,
            )],
        )
        .expect("ui provider");
    let surface = SurfaceRef {
        owner: token,
        id: shell::types::SurfaceId(1),
    };
    let _ = registry.resolve(surface).expect("ui surface");

    unsafe { ui::build_screen(c"Medinote", c"platform/render prototype") };

    // Battery voltage first: it is a synchronous ADC read (no I2C, no settle
    // time), so it costs microseconds against the sensor's tens of
    // milliseconds -- there is no reason to pay for it after the slower work.
    //
    // GPIO4 taps a 200K/100K divider off VBAT (schematic-confirmed: R21/R23,
    // net BAT_ADC), so the pin sees VBAT/3. `enable_pin_with_cal` applies
    // Espressif's curve-fitting scheme, the same one Waveshare's own
    // `Adc_GetBatteryVoltage` uses, so `read_blocking` returns calibrated
    // millivolts directly rather than a raw code this board would have to
    // curve-fit by hand.
    let t_battery_start = esp_hal::time::Instant::now();
    let mut adc_config = AdcConfig::new();
    let mut battery_adc_pin = adc_config
        .enable_pin_with_cal::<_, AdcCalCurve<esp_hal::peripherals::ADC1<'static>>>(
            battery_pin,
            Attenuation::_11dB,
        );
    let mut adc1 = Adc::new(adc1, adc_config);
    let pin_mv = adc1.read_blocking(&mut battery_adc_pin) as u32;
    let battery_mv = pin_mv * 3;
    let battery_percent = battery_percent_from_mv(battery_mv);
    let t_battery_done = esp_hal::time::Instant::now();

    let mut battery_text = [0u8; 24];
    let battery_len = format_battery(&mut battery_text, battery_percent, battery_mv);
    if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&battery_text[..battery_len]) {
        unsafe { ui::set_battery(c_text) };
    }

    // Provisioned only if the RTC does not already have a valid running
    // time -- read first, and only write `KNOWN_EPOCH` if that read says
    // there is nothing trustworthy there yet (oscillator never started, or
    // the offset marker was never set). Gating this on `warm` instead, as
    // this used to, meant every reflash re-stamped `KNOWN_EPOCH` regardless
    // of what the RTC already had: a reflash is not a timer wake, so `!warm`
    // was true on every single one, silently discarding real elapsed time
    // the PCF85063A had kept correctly across it. The RTC survives a reflash
    // even though the ESP32-S3 does not; only start over when it is actually
    // unprovisioned.
    let t_rtc_start = esp_hal::time::Instant::now();
    let mut clock = rtc::driver::Pcf85063a::new(rtc_i2c);
    let mut clock_text = [0u8; 8];
    let initial_snapshot = clock.read_snapshot().await;
    let snapshot = match initial_snapshot {
        Ok(snapshot) if snapshot.valid => Ok(snapshot),
        _ => {
            // See `sensor_task`'s identical provisioning block for why this
            // asks a host rather than trusting `KNOWN_EPOCH` outright.
            console::println!("TIME_REQUEST");
            let epoch = jtag_rx::read_epoch_line(embassy_time::Duration::from_secs(3))
                .await
                .unwrap_or(KNOWN_EPOCH);
            if let Err(error) = clock.time_set(epoch, OFFSET_MINUTES).await {
                console::println!("RTC_SET error={:?}", error);
            }
            clock.read_snapshot().await
        }
    };
    match snapshot {
        Ok(snapshot) if snapshot.valid => {
            let len = format_time_hm(&mut clock_text, snapshot.local_epoch_seconds);
            if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&clock_text[..len]) {
                unsafe { ui::set_clock(c_text) };
            }
        }
        Ok(_) => unsafe { ui::set_clock(c"--:--") },
        Err(error) => {
            console::println!("RTC_READ error={:?}", error);
            unsafe { ui::set_clock(c"--:--") };
        }
    }
    let t_rtc_done = esp_hal::time::Instant::now();

    let t_sensor_start = esp_hal::time::Instant::now();
    let mut sensor = shtc3::Shtc3::new(sensor_i2c);
    let reading = async {
        sensor.wakeup().await?;
        embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;
        sensor.start_measurement().await?;
        embassy_time::Timer::after(embassy_time::Duration::from_millis(20)).await;
        let measurement = sensor.read_measurement().await?;
        let _ = sensor.sleep().await;
        Ok::<_, shtc3::Error<_>>(measurement)
    }
    .await;
    let t_sensor_done = esp_hal::time::Instant::now();

    match reading {
        Ok(measurement) => {
            let corrected = measurement.temperature_millicelsius - SELF_HEATING_MC;
            console::println!(
                "SHTC3 t_mC={} rh_m%={} corrected_t_mC={} battery_mv={} battery_pct={}",
                measurement.temperature_millicelsius,
                measurement.humidity_millipercent,
                corrected,
                battery_mv,
                battery_percent
            );
            let mut text = [0u8; 32];
            let len = format_reading(&mut text, corrected, measurement.humidity_millipercent);
            if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&text[..len]) {
                unsafe { ui::set_reading(c_text) };
            }
            unsafe { ui::set_status(c"deep sleep, 60s") };
        }
        Err(error) => {
            console::println!("SHTC3 error={:?}", error);
            unsafe { ui::set_status(c"sensor error") };
        }
    }

    // Drive LVGL until it has nothing left in hand. The flush callback writes
    // the SPI transaction inline, so when this returns the pixels are on the
    // glass and it is safe to cut the power.
    unsafe { lightvgl_sys::lv_tick_inc(1) };
    let mut passes = 0;
    loop {
        let idle_ms = unsafe { lightvgl_sys::lv_timer_handler() };
        passes += 1;
        if idle_ms > 0 || passes >= 8 {
            break;
        }
    }
    let t_flushed = esp_hal::time::Instant::now();

    let active_ms = (t_flushed - t0).as_millis() as u32;
    console::println!(
        "CYCLE_DONE active_ms={} of_which_console_settle_ms={} battery_ms={} rtc_ms={} sensor_ms={} flush_ms={} flush_bytes={} passes={} sleeping_s={}",
        active_ms,
        CONSOLE_SETTLE_MS,
        (t_battery_done - t_battery_start).as_millis(),
        (t_rtc_done - t_rtc_start).as_millis(),
        (t_sensor_done - t_sensor_start).as_millis(),
        (t_flushed - t_sensor_done).as_millis(),
        panel::last_flush_bytes(),
        passes,
        SLEEP_INTERVAL_S
    );

    // Handed to the next wake through RTC RAM, since the console dies with the
    // chip a few microseconds from now.
    unsafe {
        core::ptr::write_volatile(
            &raw mut WAKE_COUNT,
            core::ptr::read_volatile(&raw const WAKE_COUNT) + 1,
        );
        core::ptr::write_volatile(&raw mut LAST_ACTIVE_MS, active_ms);
        core::ptr::write_volatile(
            &raw mut LAST_SENSOR_MS,
            (t_sensor_done - t_sensor_start).as_millis() as u32,
        );
        core::ptr::write_volatile(
            &raw mut LAST_FLUSH_MS,
            (t_flushed - t_sensor_done).as_millis() as u32,
        );

        let at = core::ptr::read_volatile(&raw const HISTORY_AT) as usize;
        let mut history = core::ptr::read_volatile(&raw const HISTORY_ACTIVE_MS);
        history[at % HISTORY_LEN] = active_ms;
        core::ptr::write_volatile(&raw mut HISTORY_ACTIVE_MS, history);
        core::ptr::write_volatile(&raw mut HISTORY_AT, (at + 1) as u32);
    }

    // Strictly after `active_ms` is taken and stored, so it costs the battery
    // build nothing and the measurement nothing.
    if DIAGNOSTIC_HOLD_MS > 0 {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(
            DIAGNOSTIC_HOLD_MS as u64,
        ))
        .await;
    }

    rtc.sleep_deep(&[&TimerWakeupSource::new(core::time::Duration::from_secs(
        SLEEP_INTERVAL_S,
    ))]);
}

/// Raised when something the UI shows has changed. The UI task sleeps on this
/// rather than polling LVGL on a timer.
static UI_DIRTY: embassy_sync::signal::Signal<CriticalSectionRawMutex, ()> =
    embassy_sync::signal::Signal::new();

/// One borrow of the shared bus, as handed to each device task.
type SharedI2c = I2cDevice<'static, CriticalSectionRawMutex, I2c<'static, esp_hal::Async>>;

/// Read the SHTC3 and put it on the glass. Applies this board's self-heating
/// correction here rather than in the driver: 4 C is a property of where the
/// part sits relative to warm components, which Waveshare's own code subtracts
/// and which would be wrong on any other layout.
#[embassy_executor::task]
#[allow(clippy::too_many_arguments)]
async fn sensor_task(
    i2c: SharedI2c,
    rtc_i2c: SharedI2c,
    battery_pin: esp_hal::peripherals::GPIO4<'static>,
    adc1: esp_hal::peripherals::ADC1<'static>,
) {
    /// Waveshare's `SHTC3_PETP_VOL`, in millidegrees.
    const SELF_HEATING_MC: i32 = 4_000;
    /// Room temperature and humidity move slowly, and every sample costs a
    /// wakeup, a 50ms settle, a 20ms conversion and a screen redraw. Exact
    /// cadence does not matter here; battery life does.
    const SAMPLE_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_secs(60);

    let mut sensor = shtc3::Shtc3::new(i2c);

    if sensor.wakeup().await.is_err() {
        console::println!("SHTC3 wakeup failed");
        unsafe { ui::set_status(c"sensor not responding") };
        return;
    }
    embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;

    match sensor.read_id().await {
        Ok(id) => console::println!("SHTC3_ID 0x{:04x}", id),
        Err(error) => console::println!("SHTC3_ID error={:?}", error),
    }

    // Battery ADC and the RTC set up once, alongside the sensor -- the panel
    // stays lit continuously on this path, so the same loop that redraws
    // temp/humidity also carries battery and clock, one wake covering all
    // three rather than three independent cadences.
    let mut adc_config = AdcConfig::new();
    let mut battery_adc_pin = adc_config
        .enable_pin_with_cal::<_, AdcCalCurve<esp_hal::peripherals::ADC1<'static>>>(
            battery_pin,
            Attenuation::_11dB,
        );
    let mut adc1 = Adc::new(adc1, adc_config);

    // Provisioned only if the RTC does not already have a valid running time
    // -- read first, and only write `KNOWN_EPOCH` if that read says there is
    // nothing trustworthy there yet. Writing it unconditionally, as this used
    // to, meant every reflash re-stamped `KNOWN_EPOCH` and threw away
    // whatever real time the PCF85063A had correctly kept in between: it
    // survives a reflash even though the ESP32-S3 does not.
    let mut clock = rtc::driver::Pcf85063a::new(rtc_i2c);
    let already_provisioned = matches!(clock.read_snapshot().await, Ok(snapshot) if snapshot.valid);
    if !already_provisioned {
        // Ask whatever host is attached for the real time, rather than
        // trusting a compiled-in constant that is stale by however long
        // build+flash+this cold boot's bring-up took. A host script watches
        // for this line and replies immediately with its own clock, so the
        // round trip is milliseconds, not the ~20s of bring-up that made the
        // constant-only approach fragile. No host attached (a battery
        // deployment, most of the time) just times out to `KNOWN_EPOCH`.
        console::println!("TIME_REQUEST");
        let (epoch, source) =
            match jtag_rx::read_epoch_line(embassy_time::Duration::from_secs(3)).await {
                Some(epoch) => (epoch, "host"),
                None => (KNOWN_EPOCH, "fallback"),
            };
        console::println!("RTC_PROVISION epoch={} source={}", epoch, source);
        if let Err(error) = clock.time_set(epoch, OFFSET_MINUTES).await {
            console::println!("RTC_SET error={:?}", error);
        }
    }

    // A Ticker, not a sleep: the wakeup, the 20ms conversion and the bus traffic
    // all sit inside the loop, so sleeping 5s after them drifts by ~80ms every
    // cycle. Ticker holds the cadence regardless of how long the work took.
    let mut ticker = embassy_time::Ticker::every(SAMPLE_INTERVAL);
    let mut samples: u32 = 0;
    loop {
        let pin_mv = adc1.read_blocking(&mut battery_adc_pin) as u32;
        let battery_mv = pin_mv * 3;
        let battery_percent = battery_percent_from_mv(battery_mv);
        let mut battery_text = [0u8; 24];
        let battery_len = format_battery(&mut battery_text, battery_percent, battery_mv);
        if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&battery_text[..battery_len]) {
            unsafe { ui::set_battery(c_text) };
        }

        // A bounded wait, not an unbounded one: the I2C driver's own
        // SoftwareTimeout covers a single bus transaction, but `read_snapshot`
        // is several transactions back to back, and this task runs for the
        // life of the board -- one stuck read should not cost it forever.
        let snapshot_result = embassy_time::with_timeout(
            embassy_time::Duration::from_millis(500),
            clock.read_snapshot(),
        )
        .await;
        match snapshot_result {
            Ok(Ok(snapshot)) if snapshot.valid => {
                let mut clock_text = [0u8; 8];
                let len = format_time_hm(&mut clock_text, snapshot.local_epoch_seconds);
                if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&clock_text[..len]) {
                    unsafe { ui::set_clock(c_text) };
                }
            }
            Ok(Ok(_)) => unsafe { ui::set_clock(c"--:--") },
            Ok(Err(error)) => console::println!("RTC_READ error={:?}", error),
            Err(_) => console::println!("RTC_READ timed out"),
        }

        let reading = async {
            sensor.wakeup().await?;
            embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;
            sensor.start_measurement().await?;
            // The part needs the conversion time before the read; polling mode
            // means it will NACK rather than stretch the clock if we are early.
            embassy_time::Timer::after(embassy_time::Duration::from_millis(20)).await;
            let measurement = sensor.read_measurement().await?;
            let _ = sensor.sleep().await;
            Ok::<_, shtc3::Error<_>>(measurement)
        }
        .await;

        match reading {
            Ok(measurement) => {
                let temperature = measurement.temperature_millicelsius - SELF_HEATING_MC;
                let humidity = measurement.humidity_millipercent;
                samples += 1;
                console::println!(
                    "SHTC3 t_mC={} rh_m%={} corrected_t_mC={} samples={} battery_mv={} battery_pct={}",
                    measurement.temperature_millicelsius,
                    humidity,
                    temperature,
                    samples,
                    battery_mv,
                    battery_percent
                );

                let mut text = [0u8; 32];
                let len = format_reading(&mut text, temperature, humidity);
                if let Ok(c_text) = core::ffi::CStr::from_bytes_with_nul(&text[..len]) {
                    unsafe {
                        ui::set_reading(c_text);
                        ui::set_status(c"SHTC3");
                    }
                    UI_DIRTY.signal(());
                }
            }
            Err(error) => {
                console::println!("SHTC3 read error={:?}", error);
                unsafe { ui::set_status(c"sensor read failed") };
                UI_DIRTY.signal(());
            }
        }

        ticker.next().await;
    }
}

/// 0% at 3.0V, 100% at 4.12V, linear between. Waveshare's own firmware uses
/// exactly this curve (`Adc_GetBatteryLevel` in their ADC example) for the
/// same 18650 cell this board holds, so it is reused rather than re-derived --
/// a fuel gauge this is not, but it is the vendor's own answer for this pack.
fn battery_percent_from_mv(battery_mv: u32) -> u8 {
    const EMPTY_MV: u32 = 3_000;
    const FULL_MV: u32 = 4_120;
    if battery_mv <= EMPTY_MV {
        0
    } else if battery_mv >= FULL_MV {
        100
    } else {
        (((battery_mv - EMPTY_MV) * 100) / (FULL_MV - EMPTY_MV)) as u8
    }
}

/// Render `84%  4.05V` into `buffer`, NUL-terminated, returning the length
/// including the NUL.
fn format_battery(buffer: &mut [u8; 24], percent: u8, battery_mv: u32) -> usize {
    struct Writer<'a> {
        buffer: &'a mut [u8; 24],
        at: usize,
    }

    impl Writer<'_> {
        fn push(&mut self, byte: u8) {
            if self.at < 23 {
                self.buffer[self.at] = byte;
                self.at += 1;
            }
        }

        fn str(&mut self, text: &str) {
            for byte in text.as_bytes() {
                self.push(*byte);
            }
        }

        fn u8(&mut self, value: u8) {
            if value >= 100 {
                self.push(b'0' + (value / 100) % 10);
            }
            if value >= 10 {
                self.push(b'0' + (value / 10) % 10);
            }
            self.push(b'0' + value % 10);
        }

        /// One decimal place, from millivolts, as volts.
        fn volts(&mut self, millivolts: u32) {
            self.push(b'0' + ((millivolts / 1000) % 10) as u8);
            self.push(b'.');
            self.push(b'0' + ((millivolts / 100) % 10) as u8);
            self.push(b'0' + ((millivolts / 10) % 10) as u8);
        }
    }

    let mut writer = Writer { buffer, at: 0 };
    writer.u8(percent);
    writer.str("%  ");
    writer.volts(battery_mv);
    writer.str("V");
    writer.push(0);
    writer.at
}

/// `HH:MM` from an epoch timestamp, NUL-terminated; returns the length
/// including the NUL. Minutes only: this screen redraws once a minute, so a
/// seconds digit would just sit there implying a precision the update rate
/// does not deliver.
fn format_time_hm(buffer: &mut [u8; 8], epoch_seconds: u32) -> usize {
    let seconds_today = epoch_seconds % 86_400;
    let hours = seconds_today / 3_600;
    let minutes = (seconds_today % 3_600) / 60;
    buffer[0] = b'0' + (hours / 10) as u8;
    buffer[1] = b'0' + (hours % 10) as u8;
    buffer[2] = b':';
    buffer[3] = b'0' + (minutes / 10) as u8;
    buffer[4] = b'0' + (minutes % 10) as u8;
    buffer[5] = 0;
    6
}

/// Render `21.4 C   47.8 %` into `buffer`, NUL-terminated, returning the length
/// including the NUL. Hand-rolled because `core::fmt` into a fixed buffer costs
/// more code than these two fixed-point conversions do.
fn format_reading(buffer: &mut [u8; 32], temperature_mc: i32, humidity_mpct: i32) -> usize {
    struct Writer<'a> {
        buffer: &'a mut [u8; 32],
        at: usize,
    }

    impl Writer<'_> {
        fn push(&mut self, byte: u8) {
            if self.at < 31 {
                self.buffer[self.at] = byte;
                self.at += 1;
            }
        }

        /// One decimal place, from thousandths.
        fn tenths(&mut self, value: i32) {
            let magnitude = value.unsigned_abs();
            let whole = magnitude / 1000;
            let tenth = (magnitude % 1000) / 100;
            if value < 0 {
                self.push(b'-');
            }
            if whole >= 100 {
                self.push(b'0' + (whole / 100 % 10) as u8);
            }
            if whole >= 10 {
                self.push(b'0' + (whole / 10 % 10) as u8);
            }
            self.push(b'0' + (whole % 10) as u8);
            self.push(b'.');
            self.push(b'0' + tenth as u8);
        }

        fn str(&mut self, text: &str) {
            for byte in text.as_bytes() {
                self.push(*byte);
            }
        }
    }

    let mut writer = Writer { buffer, at: 0 };
    writer.tenths(temperature_mc);
    writer.str(" C   ");
    writer.tenths(humidity_mpct);
    writer.str(" %");
    writer.push(0);
    writer.at
}

/// Drive LVGL. The screen's content comes from a shell provider registration,
/// so what reaches the glass is a surface the registry resolved rather than a
/// layout hardcoded in the backend -- the property the Inkplate's closed
/// `SurfaceModel` enum currently lacks.
#[embassy_executor::task]
async fn ui_task(_display: *mut lightvgl_sys::lv_display_t) {
    let mut registry: SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY> = SurfaceRegistry::new();
    let token = registry
        .register_provider(
            ProviderId(7),
            &[SurfaceSpec::new(
                1,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Content,
            )],
        )
        .expect("ui provider");
    let surface = SurfaceRef {
        owner: token,
        id: shell::types::SurfaceId(1),
    };
    let definition = registry.resolve(surface).expect("ui surface");

    unsafe {
        ui::build_screen(c"Medinote", c"platform/render prototype");
    }
    console::println!(
        "UI_SURFACE role={:?} refresh_hint={:?} from=registry",
        definition.role,
        definition.refresh_hint
    );

    // Event-driven, not polled. Ticking LVGL every 10ms costs 100 CPU wakeups a
    // second forever, which on a battery dwarfs everything else here -- the
    // sensor reads once per sample period. Instead the UI sleeps until
    // something marks it dirty, advances LVGL's clock by however long it
    // actually slept, and drains the work LVGL has pending.
    //
    // The timeout is a backstop, not a cadence: LVGL's own timers (label
    // scrolling, style transitions) would otherwise never run. Nothing here
    // animates, so it is deliberately long -- and longer than the sensor
    // period, or it would fire as often as real work does and double the idle
    // wakeups it is supposed to be a rare safety net against.
    const IDLE_BACKSTOP: embassy_time::Duration = embassy_time::Duration::from_secs(300);

    let mut last_tick = embassy_time::Instant::now();
    let mut redraws: u32 = 0;
    loop {
        let woken_by_change = embassy_time::with_timeout(IDLE_BACKSTOP, UI_DIRTY.wait())
            .await
            .is_ok();

        let now = embassy_time::Instant::now();
        let elapsed_ms = (now - last_tick).as_millis().min(u32::MAX as u64) as u32;
        last_tick = now;

        unsafe { lightvgl_sys::lv_tick_inc(elapsed_ms) };

        // lv_timer_handler returns the milliseconds until it next needs to run;
        // zero means it still has work in hand. Bounded so a misbehaving timer
        // cannot spin the task forever.
        let mut passes = 0;
        loop {
            let idle_ms = unsafe { lightvgl_sys::lv_timer_handler() };
            passes += 1;
            if idle_ms > 0 || passes >= 8 {
                break;
            }
        }

        if woken_by_change {
            redraws += 1;
            // The payload is the point: a full frame is 15,000 bytes, so this
            // shows what narrowing the window to the changed label actually
            // saved, rather than asserting a saving.
            console::println!(
                "UI_REDRAW n={} slept_ms={} passes={} flush_bytes={} of {}",
                redraws,
                elapsed_ms,
                passes,
                panel::last_flush_bytes(),
                panel::FRAMEBUFFER_BYTES
            );
        }
    }
}

/// Compare the panel's two power modes by what they actually do, rather than by
/// what a comment claims.
///
/// Two independent measurements per mode. The TE pin pulses once per panel
/// frame, giving the rate the glass updates at. Separately, a block is moved
/// across the screen with a partial refresh each step, giving the rate the whole
/// path sustains. A visibly faster block and a higher TE frequency are the same
/// finding from two directions.
fn benchmark_power_modes(display: &mut panel::St7305<'static>, te: &Input<'static>) {
    use esp_hal::time::Instant;

    const SAMPLE_MS: u64 = 1_000;
    const SWEEP_MS: u64 = 3_000;
    const BLOCK: usize = 24;

    // Sweep the frame-rate settings too, not just the mode. FRCTRL's defaults
    // are the vendor's choice, not the panel's ceiling: bit 4 doubles the
    // high-power rate, and the low-power field reaches from 0.25Hz to 8Hz.
    let cases = [
        (
            panel::PowerMode::High,
            panel::HighPowerRate::Half,
            panel::LowPowerRate::Hz1,
        ),
        (
            panel::PowerMode::High,
            panel::HighPowerRate::Full,
            panel::LowPowerRate::Hz1,
        ),
        (
            panel::PowerMode::Low,
            panel::HighPowerRate::Full,
            panel::LowPowerRate::Hz1,
        ),
        (
            panel::PowerMode::Low,
            panel::HighPowerRate::Full,
            panel::LowPowerRate::Hz8,
        ),
        (
            panel::PowerMode::Low,
            panel::HighPowerRate::Full,
            panel::LowPowerRate::Hz0_25,
        ),
    ];

    for (mode, high_rate, low_rate) in cases {
        display.set_frame_rates(high_rate, low_rate);
        display.set_power_mode(mode);
        esp_hal::delay::Delay::new().delay_millis(200);

        // Count TE edges over a fixed window. Polling is crude but the rates in
        // question are tens of hertz, far below the poll rate.
        let started = Instant::now();
        let mut edges: u32 = 0;
        let mut previous = te.is_high();
        while started.elapsed().as_millis() < SAMPLE_MS {
            let current = te.is_high();
            if current && !previous {
                edges += 1;
            }
            previous = current;
        }

        // Then drive the panel as hard as the path allows: move a block one
        // step per frame, refreshing only what changed.
        let started = Instant::now();
        let mut frames: u32 = 0;
        let mut x = 0usize;
        while started.elapsed().as_millis() < SWEEP_MS {
            // Per-pixel writes so the dirty box stays tight: erasing the old
            // block and drawing the new one together span far less than the
            // panel, and a partial refresh then ships only that.
            for row in 0..BLOCK {
                for column in 0..BLOCK {
                    display.set_pixel(x + column, 140 + row, false);
                }
            }
            x = (x + 8) % (panel::WIDTH - BLOCK);
            for row in 0..BLOCK {
                for column in 0..BLOCK {
                    display.set_pixel(x + column, 140 + row, true);
                }
            }
            let _ = <panel::St7305 as Panel>::refresh(display, RefreshMode::Partial);
            frames += 1;
        }

        console::println!(
            "PANEL_MODE {:?} hfra={:?} lfra={:?} te_hz={} sweep_fps={} bytes_per_frame={}",
            mode,
            high_rate,
            low_rate,
            edges * 1000 / SAMPLE_MS as u32,
            frames * 1000 / SWEEP_MS as u32,
            panel::last_flush_bytes()
        );
    }

    // Leave the panel in low power. Measured above, via TE: the glass
    // self-refreshes at 0.25Hz there against 52Hz in high power, while the
    // write path sustains the same rate in both -- so the mode governs how
    // often the panel's own update circuitry runs, not how fast we can write to
    // it. Writes land immediately either way, so the low rate costs nothing on
    // a screen that changes once a minute.
    //
    // Anything that animates should switch back to High for its duration.
    // Both fields away from the vendor defaults, in opposite directions.
    //
    // High power is raised to its maximum (51Hz measured, against the 25.5Hz
    // the vendor init selects) because it costs nothing while the panel sits in
    // low power, and it is the ceiling anything animated will want.
    //
    // Low power is dropped to its minimum, 0.25Hz -- and costs nothing to do so.
    //
    // FRCTRL sets the panel's *self-refresh* rate: how often it re-scans to
    // maintain the image it is already holding, like a DRAM refresh. It does
    // not gate writes. Measured with a one-second clock on the glass: TE pulses
    // at 0.25Hz while the seconds still tick one by one, because writing a
    // region updates that region there and then.
    //
    // So the low end is free. There is no latency penalty to trade against,
    // which an earlier comment here wrongly assumed there was.
    display.set_frame_rates(panel::HighPowerRate::Full, panel::LowPowerRate::Hz0_25);
    display.set_power_mode(panel::PowerMode::Low);
}

/// A one-pixel frame, so the edges are visible without competing with the
/// glyph the blit draws.
fn draw_border(framebuffer: &mut [u8; panel::FRAMEBUFFER_BYTES]) {
    for x in 0..panel::WIDTH {
        panel::set_pixel(framebuffer, x, 0, true);
        panel::set_pixel(framebuffer, x, panel::HEIGHT - 1, true);
    }
    for y in 0..panel::HEIGHT {
        panel::set_pixel(framebuffer, 0, y, true);
        panel::set_pixel(framebuffer, panel::WIDTH - 1, y, true);
    }
}

/// Drive the claim registry through the transitions the Inkplate performs, and
/// check the arbiter answers as the model says it should.
fn exercise_arbitration() {
    // Nothing published yet: ownership must read Unknown, never "free".
    let initial = claim::ble_ownership();

    claim::set_ble_ownership(Ownership::Active);
    let active = claim::ble_ownership();

    claim::publish_exclusive_lease(0xABCD, 7);
    let lease_ok = claim::exclusive_lease_matches(0xABCD, 7);
    let lease_wrong_epoch = claim::exclusive_lease_matches(0xABCD, 8);

    // Exclusive ownership must be refused while the supervisor is resident.
    claim::set_residency(true, true);
    claim::set_wifi_link(true);
    claim::set_service_listening(true);
    let confirmed_while_busy = claim::exclusive_ownership_confirmed(0xABCD, 7);

    // ... and granted once everything is down.
    claim::set_residency(false, false);
    claim::set_wifi_link(false);
    claim::set_service_listening(false);
    let confirmed_when_idle = claim::exclusive_ownership_confirmed(0xABCD, 7);

    console::println!(
        "ARBITRATION initial={:?} active={:?} lease_ok={} lease_wrong_epoch={} busy={} idle={}",
        initial,
        active,
        lease_ok,
        lease_wrong_epoch,
        confirmed_while_busy,
        confirmed_when_idle
    );
    assert!(matches!(initial, Ownership::Unknown));
    assert!(matches!(active, Ownership::Active));
    assert!(lease_ok && !lease_wrong_epoch);
    assert!(!confirmed_while_busy && confirmed_when_idle);
}

/// Register a provider and resolve a surface through the same registry the
/// Inkplate's launcher uses.
fn exercise_shell() {
    let mut registry: SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY> = SurfaceRegistry::new();

    let token = registry
        .register_provider(
            ProviderId(1),
            &[SurfaceSpec::new(
                1,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Content,
            )],
        )
        .expect("provider registration");

    let resolved = registry
        .resolve(SurfaceRef {
            owner: token,
            id: shell::types::SurfaceId(1),
        })
        .is_ok();
    console::println!(
        "SHELL provider_registered=true surface_resolved={} capacity={}x{}",
        resolved,
        PROVIDER_CAPACITY,
        SURFACE_CAPACITY
    );
    assert!(resolved);
}
