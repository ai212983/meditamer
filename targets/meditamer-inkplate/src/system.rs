use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use esp_hal::clock::CpuClock;
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    i2c::master::I2c,
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode as SpiMode,
    },
    time::Rate,
    timer::timg::TimerGroup,
    uart::Uart,
};
use inkplate_tempera::{
    InkplateHal, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES, PANEL_FULL_CL_HIGH_HOLD_CYCLES,
    PANEL_FULL_REFERENCE_SEQUENCE, PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES, PARTIAL_TRANSITION_BYTES,
};

#[cfg(feature = "asset-upload-http")]
use crate::net_host;
use meditamer_product::firmware::config::UART_BAUD;
use meditamer_product::firmware::types::{
    bus_owner, i2c_config, DisplayContext, PanelI2cDevice, PanelPinHold,
};
use meditamer_product::firmware::{
    app_state::{publish_app_state_snapshot, AppStateSnapshot, AppStateStore},
    observability, psram,
};
use sdcard::probe;
use static_cell::StaticCell;

mod stall_watchdog;
mod tasks;
use tasks::{board_runtime_task, external_board_runtime_resources, BoardRuntimeResources};

#[cfg(feature = "asset-upload-http")]
use meditamer_product::firmware::scheduling::{spawn as spawn_task, TaskClass};

struct BootResources {
    board: Option<psram::ExternalValue<BoardRuntimeResources>>,
    watchdog: esp_hal::timer::timg::Wdt<esp_hal::peripherals::TIMG1<'static>>,
    watchdog_self_test_complete: bool,
    watchdog_previous: Option<[u32; stall_watchdog::WORDS]>,
    #[cfg(feature = "ble-foundation")]
    bluetooth: esp_hal::peripherals::BT<'static>,
    #[cfg(feature = "asset-upload-http")]
    wifi: esp_hal::peripherals::WIFI<'static>,
    #[cfg(feature = "asset-upload-http")]
    network: &'static mut embassy_net::StackResources<{ netstack::NET_STACK_SOCKETS }>,
}

#[path = "first_fault.rs"]
mod first_fault;
/// Panic breadcrumb shared with every binary in this package (each links
/// esp-backtrace separately, so each needs its own hook symbol): `#[path]`
/// because there is no shared lib crate between the binaries.
#[path = "panic_crumb.rs"]
mod panic_crumb;

pub fn run() -> ! {
    let resources = initialize();
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| spawn_initial_tasks(spawner, resources));
}

#[inline(never)]
fn initialize() -> BootResources {
    let hal_config = esp_hal::Config::default();
    let hal_config = hal_config.with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    #[cfg(feature = "ble-foundation")]
    let ble_peripheral = peripherals.BT;
    console::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());
    console::println!(
        "PANEL_TIMING partial_cl_high_hold_cycles={} full_timing={} full_cl_high_hold_cycles={} full_row_boundary=official_ordered_ckv_le",
        PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES,
        if PANEL_FULL_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        PANEL_FULL_CL_HIGH_HOLD_CYCLES,
    );
    let reset_reason = esp_hal::system::reset_reason();
    observability::set_boot_reset_reason_code(reset_reason.map(|value| value as u8));
    console::println!(
        "BOOT_RESET reason={:?} code={}",
        reset_reason,
        reset_reason.map(|value| value as u8).unwrap_or(0),
    );
    let (flash_stage, store_stage) = meditamer_product::firmware::flash::take_persistence_trace();
    console::println!(
        "PERSIST_PREVIOUS flash={:?} store={:?}",
        flash_stage,
        store_stage,
    );
    match panic_crumb::take_crumb() {
        (Some(hash), _, frames) => {
            console::println!("PANIC_CRUMB hit=1 loc_hash=0x{hash:08x}");
            console::println!(
                "PANIC_FRAMES a=0x{:08x},0x{:08x},0x{:08x},0x{:08x},0x{:08x} b=0x{:08x},0x{:08x},0x{:08x},0x{:08x},0x{:08x}",
                frames[0], frames[1], frames[2], frames[3], frames[4],
                frames[5], frames[6], frames[7], frames[8], frames[9],
            );
        }
        (None, true, _) => console::println!("PANIC_CRUMB hit=0 base_word0=1"),
        (None, false, _) => console::println!("PANIC_CRUMB hit=0"),
    }
    if let Some(fault) = first_fault::take() {
        console::println!(
            "FIRST_FAULT pc=0x{:08x} ps=0x{:08x} sp=0x{:08x} debugcause=0x{:08x} exccause=0x{:08x} excvaddr=0x{:08x} depc=0x{:08x} a0=0x{:08x} prid=0x{:08x}",
            fault[1], fault[2], fault[3], fault[4], fault[5], fault[6], fault[7], fault[8], fault[9]
        );
        console::println!(
            "FIRST_FAULT_REGS a2=0x{:08x} a3=0x{:08x} a4=0x{:08x} a5=0x{:08x} a6=0x{:08x} a7=0x{:08x} a8=0x{:08x}",
            fault[10], fault[11], fault[12], fault[13], fault[14], fault[15], fault[16]
        );
        console::println!(
            "FIRST_FAULT_REGS a9=0x{:08x} a10=0x{:08x} a11=0x{:08x} a12=0x{:08x} a13=0x{:08x} a14=0x{:08x} a15=0x{:08x}",
            fault[17], fault[18], fault[19], fault[20], fault[21], fault[22], fault[23]
        );
    } else {
        console::println!("FIRST_FAULT hit=0");
    }
    #[cfg(feature = "cpu-load")]
    for (core, trace) in cpu_load::take_task_pointer_trace().into_iter().enumerate() {
        if let Some([stage, before, after, pc, sp, last_irq_marker]) = trace {
            console::println!(
                "TASK_POINTER_TRACE core={} stage={} before=0x{:08x} after=0x{:08x} pc=0x{:08x} sp=0x{:08x} last_irq_marker=0x{:08x}",
                core, stage, before, after, pc, sp, last_irq_marker
            );
        }
    }
    #[cfg(feature = "context-probe")]
    for (core, record) in esp_rtos::context_probe::take().into_iter().enumerate() {
        if let Some([stage, current, next, saved_tp, saved_pc, saved_sp, before, after]) = record {
            console::println!(
                "CONTEXT_COPY_TRACE core={} stage={} current=0x{:08x} next=0x{:08x} saved_tp=0x{:08x} saved_pc=0x{:08x} saved_sp=0x{:08x} frame_before=0x{:08x} frame_after=0x{:08x}",
                core, stage, current, next, saved_tp, saved_pc, saved_sp, before, after
            );
        }
    }
    #[cfg(all(feature = "context-probe", feature = "asset-upload-http"))]
    {
        let state = esp_radio::wifi::connect_probe::take();
        if state != 0 {
            console::println!(
                "CONNECT_PHASE_TRACE attempt={} phase={} state=0x{:08x}",
                state >> 8,
                state & 0xff,
                state
            );
        }
    }
    let watchdog_previous = stall_watchdog::take();
    let watchdog_self_test_complete = if let Some(record) = watchdog_previous {
        stall_watchdog::print_record("STALL_RECORD hit=1", record);
        record[5] != 0
    } else {
        console::println!("STALL_RECORD hit=0");
        false
    };
    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    psram::log_allocator_status();
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        console::println!(
            "psram: allocator initialization failed: {:?}",
            allocator_status
        );
        panic!("psram allocator initialization failed");
    }

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let watchdog = timg1.wdt;
    let touch_retry_timer = timg1.timer0;
    let imu_sample_timer = timg0.timer1;
    // esp-hal 1.2.0 removed `SoftwareInterruptControl`/`SW_INTERRUPT` in favor
    // of one `FROM_CPU_INTRn` peripheral singleton per software interrupt
    // line (esp-rs/esp-hal#6142); esp-rtos 0.4.0's `start` and
    // `start_second_core_with_stack_guard_offset` now take that singleton
    // directly rather than a constructed `SoftwareInterrupt`.
    let software_interrupt1 = peripherals.FROM_CPU_INTR1;
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    observability::log_stack_headroom("boot_after_rtos_start");

    // Serial commands are a control-plane input. Wake the reader while half of
    // the ESP32's 128-byte FIFO is still available instead of using the
    // driver's 120-byte default, which leaves too little scheduling headroom
    // under Wi-Fi and SD load.
    let uart_cfg = crate::serial_uart::config(UART_BAUD);
    let uart = Uart::new(peripherals.UART0, uart_cfg)
        .expect("failed to init UART0")
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1)
        .into_async();

    meditamer_product::firmware::flash::initialize(peripherals.FLASH);
    #[cfg(feature = "asset-upload-http")]
    {
        use meditamer_product::firmware::flash;
        use netstack::config::{NetConfigSet, WifiRuntimePolicy};

        netstack::config::install_credential_store(flash::store_wifi_credentials);
        let credentials = match flash::load_wifi_credentials() {
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
        netstack::wifi::remember_runtime_config(NetConfigSet {
            credentials,
            policy: WifiRuntimePolicy::defaults(),
        });
    }
    let mut app_state_store = AppStateStore::new();
    #[allow(unused_mut)]
    let mut persisted_state = app_state_store.load_state().unwrap_or_default();
    if let Err(error) = meditamer_product::firmware::update::initialize_boot_state() {
        console::println!("FIRMWARE_BOOT status=error reason={}", error.label());
        halt_forever();
    }
    #[allow(unused_mut)]
    let mut initial_snapshot = AppStateSnapshot::from_persisted_sanitized(persisted_state);

    #[cfg(not(feature = "asset-upload-http"))]
    if initial_snapshot.services.upload_enabled {
        initial_snapshot.services.upload_enabled = false;
        persisted_state.services.upload_enabled = false;
        app_state_store.save_state(persisted_state);
        console::println!(
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
        // esp-hal 1.2.0: DMA buffer/descriptor slices must be wrapped in
        // `DmaAlignedMut` before `DmaRxBuf`/`DmaTxBuf::new` accepts them
        // (DMA: DMA buffer types now take `DmaAlignedMut`-wrapped descriptor
        // lists and buffers).
        let rx_descriptors = esp_hal::dma::aligned::DmaAlignedMut::new(rx_descriptors)
            .expect("misaligned SPI2 DMA RX descriptors");
        let rx_buffer = esp_hal::dma::aligned::DmaAlignedMut::new(rx_buffer)
            .expect("misaligned SPI2 DMA RX buffer");
        let tx_descriptors = esp_hal::dma::aligned::DmaAlignedMut::new(tx_descriptors)
            .expect("misaligned SPI2 DMA TX descriptors");
        let tx_buffer = esp_hal::dma::aligned::DmaAlignedMut::new(tx_buffer)
            .expect("misaligned SPI2 DMA TX buffer");
        let rx = DmaRxBuf::new(rx_descriptors, rx_buffer)
            .expect("failed to allocate SPI2 DMA RX buffer");
        let tx = DmaTxBuf::new(tx_descriptors, tx_buffer)
            .expect("failed to allocate SPI2 DMA TX buffer");
        sd_spi
            .with_dma(peripherals.DMA_SPI2)
            .with_buffers(rx, tx)
            .into_async()
    };
    console::println!("sd_spi: mode=dma");
    let sd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let sd_probe = probe::SdCardProbe::new(sd_spi, sd_cs);

    #[cfg(feature = "asset-upload-http")]
    let network_resources = netstack::stack_resources();

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

    let i2c = I2c::new(peripherals.I2C0, i2c_config(100))
        .expect("failed to init I2C0")
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);
    let display_i2c = bus_owner::panel_device();

    // The PCAL6416A register cache panel/frontlight control and the battery
    // provider's fuel-gauge wake now share (typed observation subscriptions
    // plan, Phase 5; see `inkplate_tempera::expander::PcalExpander`'s module
    // doc). One dedicated device handle, like the others above.
    static PCAL_EXPANDER: StaticCell<
        Mutex<CriticalSectionRawMutex, inkplate_tempera::expander::PcalExpander<PanelI2cDevice>>,
    > = StaticCell::new();
    // Convert StaticCell's exclusive reference to a shared static owner for
    // both the panel driver and the battery provider.
    let expander: &'static Mutex<
        CriticalSectionRawMutex,
        inkplate_tempera::expander::PcalExpander<PanelI2cDevice>,
    > = PCAL_EXPANDER.init(Mutex::new(inkplate_tempera::expander::PcalExpander::new(
        bus_owner::panel_device(),
    )));

    let mut inkplate = match InkplateHal::new(
        display_i2c,
        inkplate_tempera::adapters::BusyDelay::new(),
        expander,
    ) {
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
            console::println!("panel: previous framebuffer allocation failed: {:?}", error);
            halt_forever()
        }
    };
    let previous_placement = previous_buffer.placement();
    if !matches!(previous_placement, psram::BufferPlacement::Psram) {
        console::println!(
            "panel: previous framebuffer requires psram actual={:?}",
            previous_placement
        );
        halt_forever();
    }
    if !inkplate.install_previous_framebuffer(previous_buffer.into_static_mut_slice()) {
        console::println!(
            "panel: previous framebuffer size mismatch expected={}",
            FRAMEBUFFER_BYTES
        );
        halt_forever();
    }
    console::println!(
        "panel: previous framebuffer ready bytes={} placement={:?}",
        FRAMEBUFFER_BYTES,
        previous_placement
    );

    let transition_buffer = match psram::alloc_large_byte_buffer(PARTIAL_TRANSITION_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            console::println!("panel: partial transition allocation failed: {:?}", error);
            halt_forever()
        }
    };
    let transition_placement = transition_buffer.placement();
    if !matches!(transition_placement, psram::BufferPlacement::Psram) {
        console::println!(
            "panel: partial transition requires psram actual={:?}",
            transition_placement
        );
        halt_forever();
    }
    let transition = transition_buffer.into_static_mut_slice();
    if !inkplate.install_partial_transition_buffer(transition) {
        console::println!(
            "panel: partial transition size mismatch expected={}",
            PARTIAL_TRANSITION_BYTES
        );
        halt_forever();
    }
    console::println!(
        "panel: partial transition ready bytes={} placement={:?}",
        PARTIAL_TRANSITION_BYTES,
        transition_placement
    );

    // Optional: the panel-native Gray4 framebuffer. Unlike the previous-frame
    // and partial-transition buffers above, a failure here is not fatal -- it
    // only means grayscale refreshes are unavailable, and the binary path is
    // unaffected.
    let gray4_framebuffer = match psram::alloc_large_byte_buffer(GRAYSCALE_FRAMEBUFFER_BYTES) {
        Ok(buffer) => {
            let placement = buffer.placement();
            if matches!(placement, psram::BufferPlacement::Psram) {
                console::println!(
                    "panel: gray4 framebuffer ready bytes={} placement={:?}",
                    GRAYSCALE_FRAMEBUFFER_BYTES,
                    placement
                );
                Some(buffer.into_static_mut_slice())
            } else {
                console::println!(
                    "panel: gray4 framebuffer requires psram actual={:?}; grayscale disabled",
                    placement
                );
                None
            }
        }
        Err(error) => {
            console::println!(
                "panel: gray4 framebuffer allocation failed: {:?}; grayscale disabled",
                error
            );
            None
        }
    };

    let display_context = DisplayContext {
        inkplate,
        app_state_store,
        _panel_pins: panel_pins,
        gray4_framebuffer,
    };

    #[cfg(feature = "asset-upload-http")]
    let boot_scan_only_diag_active = netstack::boot_scan_only_diag_enabled();
    #[cfg(not(feature = "asset-upload-http"))]
    let boot_scan_only_diag_active = false;
    let board = if boot_scan_only_diag_active {
        None
    } else {
        Some(external_board_runtime_resources(|| BoardRuntimeResources {
            display_context,
            i2c,
            expander,
            gpio36_input,
            touch_retry_timer,
            imu_sample_timer,
            sd_probe,
            uart,
            cpu_control: peripherals.CPU_CTRL,
            software_interrupt1,
        }))
    };

    BootResources {
        board,
        watchdog,
        watchdog_self_test_complete,
        watchdog_previous,
        #[cfg(feature = "ble-foundation")]
        bluetooth: ble_peripheral,
        #[cfg(feature = "asset-upload-http")]
        wifi: peripherals.WIFI,
        #[cfg(feature = "asset-upload-http")]
        network: network_resources,
    }
}

// Keep task construction out of the executor's permanent `run_inner` frame.
#[inline(never)]
fn spawn_initial_tasks(spawner: embassy_executor::Spawner, resources: BootResources) {
    stall_watchdog::begin(resources.watchdog_self_test_complete);
    let watchdog_token = stall_watchdog::cpu0_watchdog(
        resources.watchdog,
        resources.watchdog_self_test_complete,
        resources.watchdog_previous,
    )
    .unwrap();
    // The Wi-Fi owner is deliberately continuously ready at priority 1.
    // A default-priority (0) liveness task starves while the system is
    // healthy, creating a false watchdog verdict.
    watchdog_token.metadata().set_priority(2);
    spawner.spawn(watchdog_token);
    #[cfg(all(feature = "asset-upload-http", feature = "ble-foundation"))]
    if resources.board.is_some() {
        spawn_task(
            spawner,
            TaskClass::Wifi,
            net_host::radio_supervisor_task(resources.wifi, resources.bluetooth, resources.network)
                .unwrap(),
        );
    }

    #[cfg(all(feature = "asset-upload-http", not(feature = "ble-foundation")))]
    if resources.board.is_some() {
        spawn_task(
            spawner,
            TaskClass::Wifi,
            net_host::network_owner_task(resources.wifi, resources.network).unwrap(),
        );
    }

    if let Some(board) = resources.board {
        spawner.spawn(board_runtime_task(spawner, board).unwrap());
    } else {
        console::println!("runtime: boot_scan_only_diag active; non-wifi tasks skipped");
        console::println!("sdprobe: boot_scan_only_diag active; sd_task skipped");
    }
}

fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
