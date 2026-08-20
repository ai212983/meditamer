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

mod panel;
mod ui;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::time::Duration as HalDuration;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use board::{Panel, RefreshMode};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
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

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz));

    // esp-println's jtag-serial backend drops output when the host has not
    // finished attaching; give the CDC port time to enumerate after the reset
    // that got us here.
    esp_hal::delay::Delay::new().delay_millis(800);

    console::println!("BOARD_BOOT board=waveshare-rlcd42 chip=esp32s3");
    console::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);
    console::println!("RTOS_STARTED core=0");

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
    display.init();
    console::println!("PANEL_INIT controller=st7305 {}x{}", panel::WIDTH, panel::HEIGHT);

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

    // Hand the panel to LVGL. The display outlives the UI, so it is promoted to
    // 'static rather than borrowed across the executor.
    static DISPLAY: StaticCell<panel::St7305<'static>> = StaticCell::new();
    let display: &'static mut panel::St7305<'static> = DISPLAY.init(display);
    unsafe { ui::set_active_panel(display) };
    let lv_display = unsafe { ui::init(panel::WIDTH as i32, panel::HEIGHT as i32) };
    console::println!("LVGL_INIT {}x{} color=L8", panel::WIDTH, panel::HEIGHT);

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
    static I2C_BUS: StaticCell<
        Mutex<CriticalSectionRawMutex, I2c<'static, esp_hal::Async>>,
    > = StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(i2c));

    executor.run(|spawner| {
        spawner.spawn(clock_task(I2cDevice::new(i2c_bus)).unwrap());
        spawner.spawn(sensor_task(I2cDevice::new(i2c_bus)).unwrap());
        spawner.spawn(ui_task(lv_display).unwrap());
    });
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
async fn sensor_task(i2c: SharedI2c) {
    /// Waveshare's `SHTC3_PETP_VOL`, in millidegrees.
    const SELF_HEATING_MC: i32 = 4_000;
    /// Room temperature and humidity move slowly, and every sample costs a
    /// wakeup, a 50ms settle, a 20ms conversion and a screen redraw. Exact
    /// cadence does not matter here; battery life does.
    const SAMPLE_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_secs(30);

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

    // A Ticker, not a sleep: the wakeup, the 20ms conversion and the bus traffic
    // all sit inside the loop, so sleeping 5s after them drifts by ~80ms every
    // cycle. Ticker holds the cadence regardless of how long the work took.
    let mut ticker = embassy_time::Ticker::every(SAMPLE_INTERVAL);
    let mut samples: u32 = 0;
    loop {
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
                    "SHTC3 t_mC={} rh_m%={} corrected_t_mC={} samples={}",
                    measurement.temperature_millicelsius,
                    humidity,
                    temperature,
                    samples
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
    // animates, so it is deliberately long.
    const IDLE_BACKSTOP: embassy_time::Duration = embassy_time::Duration::from_secs(60);

    let mut last_tick = embassy_time::Instant::now();
    let mut redraws: u32 = 0;
    loop {
        let woken_by_change =
            embassy_time::with_timeout(IDLE_BACKSTOP, UI_DIRTY.wait())
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

/// Read the wall clock through `packages/rtc` -- the same PCF85063A driver the
/// Inkplate runs, unchanged. Sets a known time first, because a board whose
/// oscillator has never been started reports the clock as stopped and there
/// would be nothing to read back.
#[embassy_executor::task]
async fn clock_task(i2c: SharedI2c) {
    const KNOWN_EPOCH: u32 = 1_787_227_200; // 2026-08-20T12:00:00Z
    const OFFSET_MINUTES: i16 = 120; // UTC+2

    console::println!("RTC_TASK_START");

    // Prove the bus before trusting the driver: a scan separates "nothing at
    // 0x51" from "driver decoded something wrong".
    let mut i2c = i2c;
    let mut found = 0u32;
    for address in 0x08u8..0x78 {
        if embedded_hal_async::i2c::I2c::write(&mut i2c, address, &[])
            .await
            .is_ok()
        {
            console::println!("I2C_FOUND addr=0x{:02x}", address);
            found += 1;
        }
    }
    console::println!("I2C_SCAN devices={} expect_rtc_at=0x51", found);

    let mut clock = rtc::driver::Pcf85063a::new(i2c);

    match clock.read_snapshot().await {
        Ok(snapshot) => console::println!(
            "RTC_BEFORE valid={} reason={:?}",
            snapshot.valid,
            snapshot.reason
        ),
        Err(error) => console::println!("RTC_BEFORE error={:?}", error),
    }

    match clock.time_set(KNOWN_EPOCH, OFFSET_MINUTES).await {
        Ok(outcome) => console::println!(
            "RTC_SET utc={} offset={}",
            outcome.utc_epoch_seconds,
            outcome.offset_minutes
        ),
        Err(error) => {
            console::println!("RTC_SET error={:?}", error);
            return;
        }
    }

    // Read back through the whole decode path: registers, BCD calendar, and the
    // offset marker in the chip's one free RAM byte.
    match clock.read_snapshot().await {
        Ok(snapshot) => {
            let drift = snapshot.utc_epoch_seconds.abs_diff(KNOWN_EPOCH);
            let local_ok = snapshot.local_epoch_seconds
                == snapshot.utc_epoch_seconds + (OFFSET_MINUTES as u32) * 60;
            console::println!(
                "RTC_READBACK valid={} utc={} local={} offset={} drift_s={}",
                snapshot.valid,
                snapshot.utc_epoch_seconds,
                snapshot.local_epoch_seconds,
                snapshot.offset_minutes,
                drift
            );
            console::println!(
                "RTC_OK valid={} offset_kept={} local_derived={} drift_ok={}",
                snapshot.valid,
                snapshot.offset_minutes == OFFSET_MINUTES,
                local_ok,
                drift <= 2
            );
        }
        Err(error) => console::println!("RTC_READBACK error={:?}", error),
    }

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(5)).await;
        if let Ok(snapshot) = clock.read_snapshot().await {
            console::println!("RTC_TICK utc={}", snapshot.utc_epoch_seconds);
        }
    }
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
