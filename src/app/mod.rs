pub(crate) mod config;
mod display;
mod render;
mod runtime;
mod serial;
pub(crate) mod store;
mod touch;
mod touch_calibration_wizard;
pub(crate) mod types;

use embassy_time::{Duration, Instant, Ticker};
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
};
use meditamer::{inkplate_hal::InkplateHal, platform::HalI2c};

use self::{
    config::{APP_EVENTS, BATTERY_INTERVAL_SECONDS, REFRESH_INTERVAL_SECONDS, UART_BAUD},
    store::ModeStore,
    types::{AppEvent, DisplayContext, PanelPinHold},
};
use crate::sd_probe;

pub(crate) fn run() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let uart_cfg = UartConfig::default().with_baudrate(UART_BAUD);
    let uart = Uart::new(peripherals.UART0, uart_cfg)
        .expect("failed to init UART0")
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1)
        .into_async();

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
    let mut inkplate = match InkplateHal::new(i2c, meditamer::platform::BusyDelay::new()) {
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
    let executor = unsafe { make_static(&mut executor) };
    executor.run(move |spawner| {
        spawner.must_spawn(display::touch_pipeline_task());
        spawner.must_spawn(display::display_task(display_context));
        spawner.must_spawn(clock_task());
        spawner.must_spawn(battery_task());
        spawner.must_spawn(serial::time_sync_task(uart));
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

unsafe fn make_static<T>(value: &mut T) -> &'static mut T {
    unsafe { core::mem::transmute(value) }
}

fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
