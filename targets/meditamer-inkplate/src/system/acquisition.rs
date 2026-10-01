//! CPU1 bootstrap and acquisition ownership; CPU0 never receives the local HAL.
use super::super::stall_watchdog;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{with_timeout, Duration};
use esp_hal::{
    gpio::Input,
    i2c::master::I2c,
    peripherals::{CPU_CTRL, FROM_CPU_INTR1},
    system::Stack,
    timer::{timg::Timer, OneShotTimer},
    Blocking,
};
use inkplate_tempera::expander::PcalExpander;
use meditamer_product::firmware::{
    flash, observability,
    scheduling::{spawn as spawn_task, TaskClass},
    types::{self, PanelI2cDevice, SharedI2cBus},
};
use static_cell::StaticCell;

static BUS_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
#[cfg(not(feature = "panel-waveform-fixture"))]
static START_SENSORS: Signal<CriticalSectionRawMutex, ()> = Signal::new();
#[cfg(not(feature = "panel-waveform-fixture"))]
static SENSORS_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
type Expander = &'static Mutex<CriticalSectionRawMutex, PcalExpander<PanelI2cDevice>>;

pub(super) fn start(
    cpu: CPU_CTRL<'static>,
    interrupt: FROM_CPU_INTR1<'static>,
    bus: I2c<'static, Blocking>,
    gpio: Input<'static>,
    retry_timer: Timer<'static>,
    sample_timer: Timer<'static>,
    expander: Expander,
) {
    static STACK: StaticCell<Stack<4096>> = StaticCell::new();
    let stack = STACK.init(Stack::new());
    observability::configure_touch_core_stack(stack.bottom() as usize + 60, stack.top() as usize);
    esp_rtos::start_second_core_with_stack_guard_offset(
        cpu,
        interrupt,
        stack,
        Some(60),
        move || {
            // Bind touch's TIMG1 TIMER0 and IMU's TIMG0 TIMER1 on CPU1,
            // where their acquisition futures are polled. The shared
            // Embassy time driver remains on TIMG0 TIMER0/CPU0.
            let retry_timer = OneShotTimer::new(retry_timer).into_async();
            let sample_timer = OneShotTimer::new(sample_timer).into_async();
            static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
            EXECUTOR
                .init(esp_rtos::embassy::Executor::new())
                .run(move |spawner| {
                    spawner.spawn(stall_watchdog::cpu1_heartbeat().unwrap());
                    spawner.spawn(
                        bootstrap(spawner, bus, gpio, retry_timer, sample_timer, expander).unwrap(),
                    );
                });
        },
    );
}

pub(super) async fn wait_for_bus() -> bool {
    with_timeout(Duration::from_secs(2), BUS_READY.wait())
        .await
        .is_ok()
}
#[cfg(not(feature = "panel-waveform-fixture"))]
pub(super) async fn start_sensors() -> bool {
    START_SENSORS.signal(());
    with_timeout(Duration::from_secs(5), SENSORS_READY.wait())
        .await
        .is_ok()
}

#[embassy_executor::task]
async fn bootstrap(
    spawner: embassy_executor::Spawner,
    blocking: I2c<'static, Blocking>,
    gpio: Input<'static>,
    retry_timer: types::TouchRetryTimer,
    sample_timer: types::ImuSampleTimer,
    expander: Expander,
) {
    // CPU1 startup state stays in internal task storage, keeping acquisition
    // ownership independent of CPU0's external heap and cache availability.
    initialize(spawner, blocking, gpio, retry_timer, sample_timer, expander).await;
}

async fn initialize(
    spawner: embassy_executor::Spawner,
    blocking: I2c<'static, Blocking>,
    gpio: Input<'static>,
    retry_timer: types::TouchRetryTimer,
    sample_timer: types::ImuSampleTimer,
    expander: Expander,
) {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    stall_watchdog::cpu1_phase(1);
    spawn_task(
        spawner,
        TaskClass::FlashQuiesce,
        flash_quiesce_task().unwrap(),
    );
    flash::register_other_core_quiescer();
    static BUS: StaticCell<SharedI2cBus> = StaticCell::new();
    let bus = BUS.init(types::shared_i2c_bus(blocking));
    spawn_task(
        spawner,
        TaskClass::I2cOwner,
        types::bus_owner::owner_task(bus).unwrap(),
    );
    console::println!("I2C_OWNER_STARTUP bus=ready core=1");
    BUS_READY.signal(());
    #[cfg(feature = "panel-waveform-fixture")]
    {
        let _ = (gpio, retry_timer, sample_timer, expander);
        bus.lock().await.finish_startup();
    }
    #[cfg(not(feature = "panel-waveform-fixture"))]
    {
        use inkplate_tempera::{touch::InkplateTouch, TouchInitStatus};
        use meditamer_product::firmware::{battery, environment, imu, touch};
        START_SENSORS.wait().await;
        // CPU0 waits here without holding panel/cache locks. No other sensor
        // task runs until BME688 initialization and exclusive ELAN reset finish.
        let environment = match with_timeout(
            Duration::from_secs(2),
            types::InkplateEnvironmentDriver::initialize(types::shared_i2c_device(bus)),
        )
        .await
        {
            Ok(Ok(sensor)) => {
                console::println!("BME688 init=ready address=0x76");
                Some(sensor)
            }
            Ok(Err(error)) => {
                console::println!("BME688 init=unavailable error={:?}", error);
                None
            }
            Err(_) => {
                console::println!("BME688 init=unavailable timeout=true");
                None
            }
        };
        let mut driver = InkplateTouch::new(types::touch_i2c_device(bus));
        let resolution = if touch::config::GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
            None
        } else {
            match with_timeout(Duration::from_secs(2), driver.init_with_status()).await {
                Ok(Ok(TouchInitStatus::Ready { x_res, y_res })) => Some((x_res, y_res)),
                failure => {
                    console::println!("touch: init_failed phase=bootstrap status={:?}", failure);
                    let _ = with_timeout(Duration::from_millis(200), driver.shutdown()).await;
                    None
                }
            }
        };
        bus.lock().await.finish_startup();
        types::bus_owner::begin_runtime_timing();
        spawn_task(
            spawner,
            TaskClass::TouchAcquisition,
            touch::tasks::touch_acquisition_task(driver, retry_timer, gpio, resolution).unwrap(),
        );
        spawn_task(
            spawner,
            TaskClass::ImuBusOwner,
            types::imu_bus_owner::owner_task(types::imu_i2c_device(bus)).unwrap(),
        );
        spawn_task(
            spawner,
            TaskClass::ImuAcquisition,
            imu::imu_acquisition_task(types::imu_bus_owner::ImuClient, sample_timer).unwrap(),
        );
        // Keep input interpretation off the interrupted CPU0 task's stack.
        // This thread-mode executor has no LVGL poll and owns acquisition.
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
            TaskClass::EnvironmentAcquisition,
            environment::environment_acquisition_task(environment, bus).unwrap(),
        );
        spawn_task(
            spawner,
            TaskClass::Battery,
            battery::battery_acquisition_task(types::shared_i2c_device(bus), expander).unwrap(),
        );
        console::println!("I2C_OWNER_STARTUP sensors=started core=1");
        #[cfg(feature = "firmware-trace")]
        spawn_task(
            spawner,
            TaskClass::TraceProbe,
            meditamer_product::firmware::trace::probe_task().unwrap(),
        );
        SENSORS_READY.signal(());
    }
}

#[embassy_executor::task]
async fn flash_quiesce_task() {
    flash::serve_other_core_quiesce().await;
}
