use crate::drivers::inkplate::{
    imu::InkplateImu, touch::InkplateTouch, InkplateHal, FRAMEBUFFER_BYTES,
    PANEL_CL_HIGH_HOLD_CYCLES, PARTIAL_TRANSITION_BYTES,
};
use embassy_embedded_hal::{adapter::BlockingAsync, shared_bus::asynch::i2c::I2cDevice};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use esp_hal::clock::CpuClock;
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout},
    interrupt::software::SoftwareInterruptControl,
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode as SpiMode,
    },
    time::{Duration as HalDuration, Rate},
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, Uart},
    Blocking,
};

use super::super::config::UART_BAUD;
#[cfg(feature = "asset-upload-http")]
use super::super::net;
#[cfg(feature = "asset-upload-http")]
use super::super::storage;
use super::super::types::{DisplayContext, PanelPinHold};
use super::super::{
    app_state::{publish_app_state_snapshot, AppStateSnapshot, AppStateStore},
    psram, telemetry,
};
use sdcard::probe;
use static_cell::StaticCell;

mod tasks;
use tasks::{board_runtime_task, BoardRuntimeResources};

#[cfg(feature = "asset-upload-http")]
use super::scheduling::{spawn as spawn_task, TaskClass};

pub fn run() -> ! {
    let hal_config = esp_hal::Config::default();
    let hal_config = hal_config.with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    esp_println::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());
    esp_println::println!(
        "PANEL_TIMING cl_high_hold_cycles={}",
        PANEL_CL_HIGH_HOLD_CYCLES
    );
    let reset_reason = esp_hal::system::reset_reason();
    telemetry::set_boot_reset_reason_code(reset_reason.map(|value| value as u8));
    esp_println::println!(
        "BOOT_RESET reason={:?} code={}",
        reset_reason,
        reset_reason.map(|value| value as u8).unwrap_or(0),
    );
    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    psram::log_allocator_status();
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        esp_println::println!(
            "psram: allocator initialization failed: {:?}",
            allocator_status
        );
        panic!("psram allocator initialization failed");
    }

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);
    telemetry::log_stack_headroom("boot_after_rtos_start");

    let uart_cfg = UartConfig::default().with_baudrate(UART_BAUD);
    let uart = Uart::new(peripherals.UART0, uart_cfg)
        .expect("failed to init UART0")
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1)
        .into_async();

    let mut app_state_store = AppStateStore::new(peripherals.FLASH);
    let mut persisted_state = app_state_store.load_state().unwrap_or_default();
    let mut initial_snapshot = AppStateSnapshot::from_persisted_sanitized(persisted_state);

    #[cfg(not(feature = "asset-upload-http"))]
    if initial_snapshot.services.upload_enabled {
        initial_snapshot.services.upload_enabled = false;
        persisted_state.services.upload_enabled = false;
        app_state_store.save_state(persisted_state);
        esp_println::println!(
            "runtime_mode: upload requested but asset-upload-http is disabled; starting normal mode"
        );
    }
    publish_app_state_snapshot(initial_snapshot);

    let sd_spi_cfg = SpiConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(SpiMode::_0);
    let sd_spi = Spi::new(peripherals.SPI2, sd_spi_cfg)
        .expect("failed to init SPI2 for SD probe")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);
    let sd_spi = {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
            esp_hal::dma_buffers!(probe::SD_DMA_BUFFER_SIZE, probe::SD_DMA_BUFFER_SIZE);
        let rx = DmaRxBuf::new(rx_descriptors, rx_buffer)
            .expect("failed to allocate SPI2 DMA RX buffer");
        let tx = DmaTxBuf::new(tx_descriptors, tx_buffer)
            .expect("failed to allocate SPI2 DMA TX buffer");
        sd_spi
            .with_dma(peripherals.DMA_SPI2)
            .with_buffers(rx, tx)
            .into_async()
    };
    esp_println::println!("sd_spi: mode=dma");
    let sd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let sd_probe = probe::SdCardProbe::new(sd_spi, sd_cs);

    #[cfg(feature = "asset-upload-http")]
    let upload_http_runtime = match net::setup(peripherals.WIFI) {
        Ok(runtime) => Some(runtime),
        Err(reason) => {
            esp_println::println!("{}", reason);
            if initial_snapshot.services.upload_enabled {
                initial_snapshot.services.upload_enabled = false;
                persisted_state.services.upload_enabled = false;
                app_state_store.save_state(persisted_state);
                publish_app_state_snapshot(initial_snapshot);
            }
            None
        }
    };

    // GPIO36 is the board's shared active-low WAKE-button/touch-interrupt line.
    // ESP32 input-only GPIOs have no internal pull resistor; the board provides
    // the required external 100 kOhm pull-up.
    let gpio36_input = Input::new(peripherals.GPIO36, InputConfig::default());

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
    static I2C_BUS: StaticCell<
        Mutex<CriticalSectionRawMutex, BlockingAsync<I2c<'static, Blocking>>>,
    > = StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(BlockingAsync::new(i2c)));
    let display_i2c = I2cDevice::new(i2c_bus);
    let touch_i2c = I2cDevice::new(i2c_bus);
    let imu_i2c = I2cDevice::new(i2c_bus);
    let mut inkplate =
        match InkplateHal::new(display_i2c, crate::drivers::platform::BusyDelay::new()) {
            Ok(driver) => driver,
            Err(_) => halt_forever(),
        };
    // The previous-frame buffer is only diffed against outside the
    // interrupt-masked scan passes, so it lives in PSRAM and leaves its
    // 45000 bytes of dram2_seg for the internal heap. Without it partial
    // refreshes fall back to full ones, so treat failure as fatal.
    let previous_buffer = match psram::alloc_large_byte_buffer(FRAMEBUFFER_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            esp_println::println!("panel: previous framebuffer allocation failed: {:?}", error);
            halt_forever()
        }
    };
    let previous_placement = previous_buffer.placement();
    if !matches!(previous_placement, psram::BufferPlacement::Psram) {
        esp_println::println!(
            "panel: previous framebuffer requires psram actual={:?}",
            previous_placement
        );
        halt_forever();
    }
    if !inkplate.install_previous_framebuffer(previous_buffer.into_static_mut_slice()) {
        esp_println::println!(
            "panel: previous framebuffer size mismatch expected={}",
            FRAMEBUFFER_BYTES
        );
        halt_forever();
    }
    esp_println::println!(
        "panel: previous framebuffer ready bytes={} placement={:?}",
        FRAMEBUFFER_BYTES,
        previous_placement
    );

    let transition_buffer = match psram::alloc_large_byte_buffer(PARTIAL_TRANSITION_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            esp_println::println!("panel: partial transition allocation failed: {:?}", error);
            halt_forever()
        }
    };
    let transition_placement = transition_buffer.placement();
    if !matches!(transition_placement, psram::BufferPlacement::Psram) {
        esp_println::println!(
            "panel: partial transition requires psram actual={:?}",
            transition_placement
        );
        halt_forever();
    }
    let transition = transition_buffer.into_static_mut_slice();
    if !inkplate.install_partial_transition_buffer(transition) {
        esp_println::println!(
            "panel: partial transition size mismatch expected={}",
            PARTIAL_TRANSITION_BYTES
        );
        halt_forever();
    }
    esp_println::println!(
        "panel: partial transition ready bytes={} placement={:?}",
        PARTIAL_TRANSITION_BYTES,
        transition_placement
    );
    let touch_driver = InkplateTouch::new(touch_i2c);
    let imu_driver = InkplateImu::new(imu_i2c);

    let display_context = DisplayContext {
        inkplate,
        app_state_store,
        _panel_pins: panel_pins,
    };

    let mut executor = esp_rtos::embassy::Executor::new();
    let executor = unsafe { make_static(&mut executor) };
    executor.run(move |spawner| {
        #[cfg(feature = "asset-upload-http")]
        let boot_scan_only_diag_active = net::boot_scan_only_diag_enabled();
        #[cfg(not(feature = "asset-upload-http"))]
        let boot_scan_only_diag_active = false;

        #[cfg(feature = "asset-upload-http")]
        if let Some(upload_http_runtime) = upload_http_runtime {
            spawn_task(
                spawner,
                TaskClass::Wifi,
                net::wifi_connection_task(
                    upload_http_runtime.wifi_controller,
                    upload_http_runtime.initial_credentials,
                    upload_http_runtime.stack,
                )
                .unwrap(),
            );
            if !boot_scan_only_diag_active {
                spawn_task(
                    spawner,
                    TaskClass::Network,
                    net::net_task(upload_http_runtime.net_runner).unwrap(),
                );
                spawn_task(
                    spawner,
                    TaskClass::Http,
                    storage::upload::http_server_task(upload_http_runtime.stack).unwrap(),
                );
            }
        }

        if boot_scan_only_diag_active {
            esp_println::println!("runtime: boot_scan_only_diag active; non-wifi tasks skipped");
            esp_println::println!("sdprobe: boot_scan_only_diag active; sd_task skipped");
        } else {
            spawner.spawn(
                board_runtime_task(
                    spawner,
                    BoardRuntimeResources {
                        display_context,
                        touch_driver,
                        imu_driver,
                        gpio36_input,
                        sd_probe,
                        uart,
                        cpu_control: peripherals.CPU_CTRL,
                        software_interrupt1: software_interrupts.software_interrupt1,
                    },
                )
                .unwrap(),
            );
        }
    });
}

unsafe fn make_static<T>(value: &mut T) -> &'static mut T {
    unsafe { core::mem::transmute(value) }
}

fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
