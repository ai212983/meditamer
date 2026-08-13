use esp_hal::{
    gpio::Input, interrupt::software::SoftwareInterrupt, peripherals::CPU_CTRL, system::Stack,
};
use static_cell::StaticCell;

use crate::firmware::{
    display::display_task,
    imu, observability,
    power::battery_task,
    self_test::diagnostics_task,
    serial::serial_task,
    storage, touch,
    types::{DisplayContext, InkplateImuDriver, InkplateTouchDriver, SdProbeDriver, SerialUart},
    update::firmware_health_task,
};
use crate::{
    firmware::touch::config::GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED,
    platform::inkplate::TouchInitStatus,
};

use crate::firmware::scheduling::{spawn as spawn_task, TaskClass};

use super::halt_forever;

const TOUCH_CORE_STACK_BYTES: usize = 4 * 1024;
const TOUCH_CORE_STACK_GUARD_OFFSET: usize = 60;

pub(super) struct BoardRuntimeResources {
    pub(super) display_context: DisplayContext,
    pub(super) touch_driver: InkplateTouchDriver,
    pub(super) imu_driver: InkplateImuDriver,
    pub(super) gpio36_input: Input<'static>,
    pub(super) sd_probe: SdProbeDriver,
    pub(super) uart: SerialUart,
    pub(super) cpu_control: CPU_CTRL<'static>,
    pub(super) software_interrupt1: SoftwareInterrupt<'static, 1>,
}

#[embassy_executor::task]
pub(super) async fn board_runtime_task(
    spawner: embassy_executor::Spawner,
    resources: BoardRuntimeResources,
) {
    let BoardRuntimeResources {
        mut display_context,
        mut touch_driver,
        imu_driver,
        gpio36_input,
        sd_probe,
        uart,
        cpu_control,
        software_interrupt1,
    } = resources;

    if display_context.inkplate.init_core().await.is_err() {
        halt_forever();
    }
    let _ = display_context.inkplate.set_wakeup(true).await;
    let _ = display_context.inkplate.frontlight_off().await;

    // Bring ELAN up before the IMU and display tasks start using the shared I2C
    // bus. Per-transaction locking cannot make this multi-step reset/hello
    // sequence atomic, and concurrent startup produced repeatable arbitration
    // failures on hardware.
    let initial_touch_resolution = if GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
        None
    } else {
        match touch_driver.init_with_status().await {
            Ok(TouchInitStatus::Ready { x_res, y_res }) => Some((x_res, y_res)),
            status => {
                esp_println::println!("touch: init_failed phase=bootstrap status={:?}", status);
                let _ = touch_driver.shutdown().await;
                None
            }
        }
    };

    start_touch_core(
        cpu_control,
        software_interrupt1,
        touch_driver,
        gpio36_input,
        initial_touch_resolution,
    );

    spawn_task(
        spawner,
        TaskClass::TouchPipeline,
        touch::tasks::touch_pipeline_task().unwrap(),
    );
    spawn_task(
        spawner,
        TaskClass::ImuPipeline,
        imu::imu_pipeline_task().unwrap(),
    );
    spawn_task(
        spawner,
        TaskClass::ImuAcquisition,
        imu::imu_acquisition_task(imu_driver).unwrap(),
    );
    spawn_task(
        spawner,
        TaskClass::Display,
        display_task(display_context).unwrap(),
    );
    spawn_task(spawner, TaskClass::Diagnostics, diagnostics_task().unwrap());
    spawn_task(spawner, TaskClass::Battery, battery_task().unwrap());
    spawn_task(spawner, TaskClass::Sd, storage::sd_task(sd_probe).unwrap());
    spawn_task(spawner, TaskClass::Serial, serial_task(uart).unwrap());
    spawn_task(
        spawner,
        TaskClass::FirmwareHealth,
        firmware_health_task().unwrap(),
    );
}

fn start_touch_core(
    cpu_control: CPU_CTRL<'static>,
    software_interrupt1: SoftwareInterrupt<'static, 1>,
    touch_driver: InkplateTouchDriver,
    gpio36_input: Input<'static>,
    initial_touch_resolution: Option<(u16, u16)>,
) {
    static TOUCH_CORE_STACK: StaticCell<Stack<TOUCH_CORE_STACK_BYTES>> = StaticCell::new();
    let stack = TOUCH_CORE_STACK.init(Stack::new());
    observability::configure_touch_core_stack(
        stack.bottom() as usize + TOUCH_CORE_STACK_GUARD_OFFSET,
        stack.top() as usize,
    );

    esp_rtos::start_second_core_with_stack_guard_offset(
        cpu_control,
        software_interrupt1,
        stack,
        Some(TOUCH_CORE_STACK_GUARD_OFFSET),
        move || run_touch_core(touch_driver, gpio36_input, initial_touch_resolution),
    );
}

fn run_touch_core(
    touch_driver: InkplateTouchDriver,
    gpio36_input: Input<'static>,
    initial_touch_resolution: Option<(u16, u16)>,
) -> ! {
    static TOUCH_CORE_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = TOUCH_CORE_EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| {
        spawn_task(
            spawner,
            TaskClass::TouchAcquisition,
            touch::tasks::touch_acquisition_task(
                touch_driver,
                gpio36_input,
                initial_touch_resolution,
            )
            .unwrap(),
        );
    })
}
