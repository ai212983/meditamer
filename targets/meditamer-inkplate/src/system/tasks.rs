use super::halt_forever;
use super::stall_watchdog;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use esp_hal::{
    gpio::Input,
    i2c::master::I2c,
    peripherals::{CPU_CTRL, FROM_CPU_INTR1},
    timer::timg::Timer,
    Blocking,
};
use inkplate_tempera::expander::PcalExpander;
#[cfg(not(feature = "panel-waveform-fixture"))]
use meditamer_product::firmware::{
    display::display_task,
    scheduling::{spawn as spawn_task, TaskClass},
    self_test::diagnostics_task,
    serial::serial_task,
    storage,
    update::firmware_health_task,
};
use meditamer_product::firmware::{
    observability, psram,
    types::{DisplayContext, PanelI2cDevice, SdProbeDriver, SerialUart},
};

#[path = "acquisition.rs"]
mod acquisition;

pub(super) struct BoardRuntimeResources {
    pub(super) display_context: DisplayContext,
    pub(super) i2c: I2c<'static, Blocking>,
    pub(super) expander: &'static Mutex<CriticalSectionRawMutex, PcalExpander<PanelI2cDevice>>,
    pub(super) gpio36_input: Input<'static>,
    pub(super) touch_retry_timer: Timer<'static>,
    pub(super) imu_sample_timer: Timer<'static>,
    pub(super) sd_probe: SdProbeDriver,
    pub(super) uart: SerialUart,
    pub(super) cpu_control: CPU_CTRL<'static>,
    pub(super) software_interrupt1: FROM_CPU_INTR1<'static>,
}

#[embassy_executor::task]
pub(super) async fn board_runtime_task(
    spawner: embassy_executor::Spawner,
    resources: psram::ExternalValue<BoardRuntimeResources>,
) {
    let mut future = external_value(|| run_board_runtime(spawner, resources));
    observability::record_stack_headroom();
    future.pin_mut().await;
    observability::record_stack_headroom();
}

async fn run_board_runtime(
    spawner: embassy_executor::Spawner,
    resources: psram::ExternalValue<BoardRuntimeResources>,
) {
    let BoardRuntimeResources {
        mut display_context,
        i2c,
        expander,
        gpio36_input,
        touch_retry_timer,
        imu_sample_timer,
        sd_probe,
        uart,
        cpu_control,
        software_interrupt1,
    } = resources.into_inner();
    stall_watchdog::cpu0_phase(1);
    #[cfg(feature = "firmware-trace")]
    let _ = meditamer_product::firmware::trace::init();
    console::enable_deferred_logs();
    meditamer_product::firmware::scheduling::spawn(
        spawner,
        meditamer_product::firmware::scheduling::TaskClass::Console,
        meditamer_product::firmware::serial::console_task().unwrap(),
    );
    acquisition::start(
        cpu_control,
        software_interrupt1,
        i2c,
        gpio36_input,
        touch_retry_timer,
        imu_sample_timer,
        expander,
    );
    if !acquisition::wait_for_bus().await {
        console::println!("I2C_OWNER_STARTUP failed=bus_ready");
        meditamer_product::firmware::reset_pending_update_or_halt();
    }
    stall_watchdog::cpu0_phase(2);
    if display_context.inkplate.init_core().await.is_err() {
        halt_forever();
    }
    let _ = display_context.inkplate.set_wakeup(true).await;
    let _ = display_context.inkplate.frontlight_off().await;

    #[cfg(feature = "panel-waveform-fixture")]
    {
        let _ = (spawner, sd_probe, uart);
        crate::panel_waveform_fixture::run(&mut display_context).await;
    }
    #[cfg(not(feature = "panel-waveform-fixture"))]
    {
        if !acquisition::start_sensors().await {
            console::println!("I2C_OWNER_STARTUP failed=sensor_startup");
            meditamer_product::firmware::reset_pending_update_or_halt();
        }
        // Touch and IMU pipelines run on the CPU1 thread-mode executor with
        // acquisition. Polling them from a level-1 interrupt used the stack
        // of whichever CPU0 task was interrupted (often the radio task).
        #[cfg(feature = "firmware-trace")]
        meditamer_product::firmware::scheduling::spawn(
            spawner,
            TaskClass::TraceDrain,
            meditamer_product::firmware::trace::drain_task().unwrap(),
        );
        spawn_task(
            spawner,
            TaskClass::Display,
            display_task(display_context).unwrap(),
        );
        #[cfg(feature = "cpu-load")]
        spawn_task(
            spawner,
            TaskClass::CpuObservation,
            meditamer_product::firmware::cpu_observation::acquisition_task().unwrap(),
        );
        spawn_task(spawner, TaskClass::Diagnostics, diagnostics_task().unwrap());
        spawn_task(spawner, TaskClass::Sd, storage::sd_task(sd_probe).unwrap());
        spawn_task(
            spawner,
            TaskClass::Serial,
            serial_task(
                uart,
                meditamer_product::firmware::types::bus_owner::rtc_device(),
            )
            .unwrap(),
        );
        spawn_task(
            spawner,
            TaskClass::FirmwareHealth,
            firmware_health_task().unwrap(),
        );
    }
}

pub(super) fn external_board_runtime_resources<Make>(
    make: Make,
) -> psram::ExternalValue<BoardRuntimeResources>
where
    Make: FnOnce() -> BoardRuntimeResources,
{
    external_value(make)
}

fn external_value<T, Make>(make: Make) -> psram::ExternalValue<T>
where
    Make: FnOnce() -> T,
{
    match psram::ExternalValue::try_new_with(make) {
        Ok(value) => value,
        Err(_) => {
            console::println!("runtime: external startup allocation failed");
            meditamer_product::firmware::reset_pending_update_or_halt();
        }
    }
}
