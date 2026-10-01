//! Medinote firmware for the Waveshare ESP32-S3-RLCD-4.2.
//!
//! This target composes exactly one product with one board, per ADR-0015:
//! `medinote`'s content and cadence policy over `waveshare_rlcd42`'s panel
//! and USB-Serial-JTAG hardware. This target owns chip startup, peripheral
//! wiring, and task spawning.

#![no_std]
#![no_main]

mod battery;
#[cfg(feature = "wifi-storage")]
mod net_commands;
#[cfg(feature = "wifi-storage")]
mod net_host;
#[cfg(feature = "wifi-storage")]
mod net_http;
#[cfg(feature = "wifi-storage")]
mod network_memory;
#[cfg(feature = "wifi-storage")]
mod network_retention;
#[cfg(feature = "wifi-storage")]
mod wifi_credentials;
#[cfg(feature = "wifi-storage")]
use esp_bootloader_esp_idf as _;
#[cfg(feature = "cheertok-controls")]
mod cheertok;
#[cfg(feature = "crash-screen")]
mod crash_screen;
mod environment;
mod observations;
mod runtime_ui;
#[cfg(feature = "sd-storage")]
#[path = "sd_probe/recovery.rs"]
mod sd_recovery;
mod sleep;
mod sleep_fixture;
#[cfg(feature = "sd-storage")]
mod storage;
mod wall_clock;

#[cfg(feature = "sd-storage")]
use aligned::{Aligned, A4};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_backtrace as _;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::rtc_cntl::sleep::LowPower;
#[cfg(feature = "sd-storage")]
use esp_hal::sdmmc::SdHostController;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Duration as HalDuration;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use static_cell::{ConstStaticCell, StaticCell};

use waveshare_rlcd42::{panel, panel_lvgl};

/// One borrow of the shared bus, as handed to the runtime UI's two devices.
type SharedI2c = I2cDevice<'static, CriticalSectionRawMutex, I2c<'static, esp_hal::Async>>;

/// How long to let the USB CDC host re-enumerate before trusting the console.
///
/// Diagnostic cost only: a battery build has no console and pays none of it.
/// It is excluded from the reported active time for exactly that reason.
const CONSOLE_SETTLE_MS: u32 = 800;

#[cfg(all(feature = "cheertok-controls", not(feature = "wifi-storage")))]
// Physical proactive-pairing capture peaked at 35,700 bytes. Keep 13,452
// bytes (37.7%) of allocator headroom while returning 16 KiB to the CPU0
// stack for LVGL's deepest label-render path.
const BLE_INTERNAL_HEAP_BYTES: usize = 49_152;

#[esp_hal::main]
fn main() -> ! {
    // esp-hal selects the CPU clock during chip initialisation. Runtime power
    // policy therefore controls workload, panel self-refresh, and deep sleep;
    // the CPU ceiling stays available for Hourglass without a reboot.
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // Boot-phase timing. Deep sleep makes the whole of `main` the wake cost, so
    // what each phase costs decides the achievable sample cadence -- measured
    // here rather than estimated. Timestamps are collected and printed once at
    // the end, because the console write is itself milliseconds over USB CDC.
    let t0 = esp_hal::time::Instant::now();

    let reset_reason = esp_hal::system::reset_reason();
    let wake_cause = esp_hal::rtc_cntl::wakeup_cause();

    #[cfg(feature = "wifi-storage")]
    network_retention::init(reset_reason);

    // esp-println's jtag-serial backend drops output when the host has not
    // finished attaching; give the CDC port time to enumerate after the reset
    // that got us here.
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);

    let t_cdc = esp_hal::time::Instant::now();
    console::println!("BOARD_BOOT board=waveshare-rlcd42 chip=esp32s3");
    console::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());
    console::println!("RESET reason={:?} wake={:?}", reset_reason, wake_cause);

    #[cfg(feature = "wifi-storage")]
    network_memory::init_heap();
    #[cfg(feature = "wifi-storage")]
    let network_buffers = network_memory::init_bytes(peripherals.PSRAM, net_http::BUFFER_BYTES);
    #[cfg(all(feature = "cheertok-controls", not(feature = "wifi-storage")))]
    esp_alloc::heap_allocator!(size: BLE_INTERNAL_HEAP_BYTES);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("RTOS_STARTED core=0");
    #[cfg(feature = "wifi-storage")]
    {
        wifi_credentials::initialize(peripherals.FLASH);
        netstack::config::install_credential_store(wifi_credentials::store);
        let credentials = match wifi_credentials::load() {
            Ok(Some(credentials)) => {
                console::println!("WIFI_CONFIG status=loaded source=internal_flash");
                Some(credentials)
            }
            Ok(None) => {
                console::println!("WIFI_CONFIG status=unprovisioned source=internal_flash");
                None
            }
            Err(error) => {
                console::println!("WIFI_CONFIG status=read_error error={:?}", error);
                None
            }
        };
        netstack::wifi::remember_runtime_config(netstack::config::NetConfigSet {
            credentials,
            policy: netstack::config::WifiRuntimePolicy::defaults(),
        });
    }
    let t_rtos = esp_hal::time::Instant::now();

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
    // Waveshare's factory driver enables a pull-up on RST. Apply the same
    // active-mode fail-safe to both idle-high panel controls. These software
    // pulls are not retained when the digital pad domain powers down.
    let idle_high_output = OutputConfig::default().with_pull(Pull::Up);
    static FRAMEBUFFER: ConstStaticCell<[u8; panel::FRAMEBUFFER_BYTES]> =
        ConstStaticCell::new([0; panel::FRAMEBUFFER_BYTES]);
    let framebuffer = FRAMEBUFFER.take();
    let mut display = panel::St7305::new(
        framebuffer,
        spi,
        Output::new(peripherals.GPIO5, Level::Low, output),
        Output::new(peripherals.GPIO40, Level::High, idle_high_output),
        Output::new(peripherals.GPIO41, Level::High, idle_high_output),
    );
    let t_pre_panel = esp_hal::time::Instant::now();
    display.init();
    display.set_frame_rates(panel::HighPowerRate::Full, panel::LowPowerRate::Hz0_25);
    let t_panel = esp_hal::time::Instant::now();
    console::println!(
        "PANEL_INIT controller=st7305 {}x{}",
        panel::WIDTH,
        panel::HEIGHT
    );

    // Hand the panel to LVGL. The display outlives the UI, so it is promoted to
    // 'static rather than borrowed across the executor.
    static DISPLAY: StaticCell<panel::St7305<'static>> = StaticCell::new();
    let display: &'static mut panel::St7305<'static> = DISPLAY.init(display);
    let panel_session =
        panel_lvgl::PanelLvglSession::init(display, panel::WIDTH as i32, panel::HEIGHT as i32);
    // The board bridge initialized LVGL; the runtime session claims that epoch
    // without taking over the board's deinitialization policy.
    let runtime_session = render::lvgl_adapter::RuntimeSession::adopt_initialized()
        .expect("claim board-initialized LVGL runtime");
    let ui_token = runtime_session.access_token();
    let bootstrap_root = ui_token
        .capture_unmanaged_active_screen()
        .expect("LVGL boot screen access")
        .expect("LVGL boot screen");
    let t_lvgl = esp_hal::time::Instant::now();
    console::println!("LVGL_INIT {}x{} color=L8", panel::WIDTH, panel::HEIGHT);

    console::println!(
        "WAKE cause={:?} panel_reinit=true destination=home",
        wake_cause
    );
    console::println!(
        "BOOT_PHASES_MS cdc_settle={} to_rtos={} platform_probes={} panel_init={} lvgl={} total={}",
        (t_cdc - t0).as_millis(),
        (t_rtos - t_cdc).as_millis(),
        (t_pre_panel - t_rtos).as_millis(),
        (t_panel - t_pre_panel).as_millis(),
        (t_lvgl - t_panel).as_millis(),
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

    // The async RTC driver requires a running executor.
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    // One bus, two devices. Each task borrows it per transaction rather than
    // owning it, which is what lets the sensor and the clock coexist.
    static I2C_BUS: StaticCell<Mutex<CriticalSectionRawMutex, I2c<'static, esp_hal::Async>>> =
        StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(i2c));

    // esp-hal 1.2.0 split sleep control into `LowPower` (over `LPWR`), separate
    // from `Rtc` (over `RTC_TIMER`, timekeeping only) -- this target only ever
    // used `Rtc` for sleep, never for timekeeping.
    let low_power = LowPower::new(peripherals.LPWR);

    let mut adc_config = AdcConfig::new();
    let battery_adc_pin = adc_config
        .enable_pin_with_cal::<_, AdcCalCurve<esp_hal::peripherals::ADC1<'static>>>(
            peripherals.GPIO4,
            Attenuation::_11dB,
        );
    let adc1 = Adc::new(peripherals.ADC1, adc_config);

    #[cfg(feature = "sd-storage")]
    let sd_slot = {
        static SD_HOST: StaticCell<SdHostController<'static>> = StaticCell::new();
        SD_HOST
            .init(SdHostController::new(peripherals.SDHOST, Default::default()).expect("sdhost"))
            .slot::<1>(Default::default())
            .expect("sd slot")
            .with_clk(peripherals.GPIO38)
            .with_cmd(peripherals.GPIO21)
            .with_data0(peripherals.GPIO39)
            .into_async()
    };
    #[cfg(feature = "sd-storage")]
    static SD_ENGINE: ConstStaticCell<sdcard::fat::FatEngine> =
        ConstStaticCell::new(sdcard::fat::FatEngine::new());
    #[cfg(feature = "sd-storage")]
    static SD_SECTOR: ConstStaticCell<Aligned<A4, [u8; 512]>> =
        ConstStaticCell::new(Aligned([0; 512]));

    executor.run(|spawner| {
        #[cfg(feature = "wifi-storage")]
        {
            net_http::install_buffers(network_buffers);
            #[cfg(feature = "cheertok-controls")]
            spawner.spawn(
                net_host::radio_supervisor_task(
                    peripherals.WIFI,
                    peripherals.BT,
                    netstack::stack_resources(),
                )
                .expect("radio supervisor task pool"),
            );
            #[cfg(not(feature = "cheertok-controls"))]
            spawner.spawn(net_host::run(peripherals.WIFI).expect("network task pool"));
            spawner.spawn(net_commands::run().expect("network commands task pool"));
        }
        #[cfg(all(feature = "cheertok-controls", not(feature = "wifi-storage")))]
        spawner.spawn(cheertok::input_task(peripherals.BT).unwrap());
        spawner.spawn(
            environment::environment_provider_task(
                I2cDevice::new(i2c_bus),
                medinote::config::SELF_HEATING_MC,
                medinote::config::HUMIDITY_SCALE_PERMILLE,
            )
            .unwrap(),
        );
        spawner.spawn(battery::battery_provider_task(adc1, battery_adc_pin).unwrap());
        #[cfg(feature = "sd-storage")]
        spawner.spawn(storage::run(sd_slot, SD_ENGINE.take(), SD_SECTOR.take()).unwrap());
        spawner.spawn(
            runtime_ui::runtime_ui_task(runtime_ui::RuntimeUiResources {
                runtime_session,
                ui_token,
                bootstrap_root,
                rtc_i2c: I2cDevice::new(i2c_bus),
                low_power,
                key_pin: peripherals.GPIO18,
                boot_pin: peripherals.GPIO0,
                panel_session,
            })
            .unwrap(),
        );
    });
}
