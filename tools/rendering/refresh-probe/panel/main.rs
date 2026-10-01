//! Stand-alone Inkplate panel performance and transition-soak app.
//!
//! This binary deliberately excludes product UI, touch, RTC, sensors, storage,
//! and radio tasks. It qualifies one compile-time waveform timing candidate at
//! a time using deterministic full-frame patterns and machine-readable logs.
#![no_std]
#![no_main]

mod probe_config;

mod draw;
mod gray_pattern;
mod marker;
mod probe_mode;
mod refresh;
mod resources;
mod soak;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use esp_backtrace as _;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::{clock::CpuClock, i2c::master::I2c, timer::timg::TimerGroup};
use inkplate_tempera::{
    adapters::BusyDelay, expander::PcalExpander, InkplateHal, E_INK_HEIGHT, E_INK_WIDTH,
};
use meditamer_product::firmware::types::PanelPinHold;
use meditamer_product::firmware::{
    psram::{self, AllocatorState, ExternalValue},
    types::{configured_i2c_device, i2c_config, shared_i2c_bus, SharedI2cBus},
};
use static_cell::StaticCell;

use probe_mode::PANEL_I2C_KHZ;
use resources::{install_partial_buffers, SoakResources};
use soak::panel_soak_task;

esp_bootloader_esp_idf::esp_app_desc!();

const _: () = assert!(E_INK_WIDTH == probe_config::WIDTH as usize);
const _: () = assert!(E_INK_HEIGHT == probe_config::HEIGHT as usize);
const _: () = assert!(probe_config::BAND_WIDTH == 50);

#[esp_hal::main]
fn main() -> ! {
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    console::println!(
        "PANEL_SOAK event=boot app=panel-refresh-probe spec_version={} cpu_hz={}",
        probe_config::SPEC_VERSION,
        esp_hal::clock::cpu_clock().as_hz(),
    );

    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    if !matches!(allocator_status.state, AllocatorState::Initialized) {
        console::println!(
            "PANEL_SOAK event=halt stage=allocator status={:?}",
            allocator_status
        );
        resources::halt()
    }

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

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

    let i2c = I2c::new(peripherals.I2C0, i2c_config(100))
        .expect("failed to initialize panel I2C")
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);
    static I2C_BUS: StaticCell<SharedI2cBus> = StaticCell::new();
    let i2c_bus = I2C_BUS.init(shared_i2c_bus(i2c));
    static EXPANDER: StaticCell<
        Mutex<CriticalSectionRawMutex, PcalExpander<resources::ProbeI2cDevice>>,
    > = StaticCell::new();
    let expander = EXPANDER.init(Mutex::new(PcalExpander::new(configured_i2c_device::<
        PANEL_I2C_KHZ,
    >(i2c_bus))));
    let mut inkplate = match InkplateHal::new(
        configured_i2c_device::<PANEL_I2C_KHZ>(i2c_bus),
        BusyDelay::new(),
        expander,
    ) {
        Ok(driver) => driver,
        Err(error) => {
            console::println!(
                "PANEL_SOAK event=halt stage=driver_create error={:?}",
                error
            );
            resources::halt()
        }
    };

    install_partial_buffers(&mut inkplate);
    let resources = match ExternalValue::try_new_with(|| SoakResources {
        inkplate,
        _panel_pins: panel_pins,
    }) {
        Ok(resources) => resources,
        Err(error) => {
            console::println!("PANEL_SOAK event=halt stage=resources error={:?}", error);
            resources::halt()
        }
    };

    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(panel_soak_task(resources).unwrap());
    })
}
