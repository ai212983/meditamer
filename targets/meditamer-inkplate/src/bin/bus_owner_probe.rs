//! Focused CPU1 I2C affinity, queue expiry and recovery probe; no panel writes.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{with_timeout, Duration, Timer};
use embedded_hal_async::i2c::{I2c, Operation};
use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::{
    clock::CpuClock,
    i2c::master::I2c as HalI2c,
    system::{Cpu, Stack},
    Blocking,
};
use inkplate_tempera::bus_proxy::{BusProxy, BusRate, OwnerBus, ProxyError};
use meditamer_product::firmware::types::{
    i2c_config, panel_i2c_device, shared_i2c_bus, shared_i2c_device, ConfiguredI2cDevice,
    SharedI2cBus, SharedI2cDevice,
};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();
type BusError = embassy_embedded_hal::shared_bus::I2cDeviceError<esp_hal::i2c::master::Error>;
static PROXY: BusProxy<BusError> = BusProxy::new();
static READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static TRANSFER_MAX_US: AtomicU32 = AtomicU32::new(0);

struct LocalBus {
    standard: SharedI2cDevice,
    panel: ConfiguredI2cDevice<400>,
}
impl OwnerBus for LocalBus {
    type Error = BusError;
    async fn transaction(
        &mut self,
        rate: BusRate,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        assert_eq!(Cpu::current(), Cpu::AppCpu);
        // Synthetic stalled-future case: no physical access to this address.
        if address == 0x7e {
            core::future::pending::<()>().await;
        }
        let started = embassy_time::Instant::now();
        let result = match rate {
            BusRate::Standard => self.standard.transaction(address, operations).await,
            BusRate::Panel => self.panel.transaction(address, operations).await,
        };
        TRANSFER_MAX_US.fetch_max(started.elapsed().as_micros() as u32, Ordering::Relaxed);
        result
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));
    let _ = meditamer_product::firmware::psram::init_allocator(p.PSRAM);
    let timers = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    esp_rtos::start(timers.timer0, p.FROM_CPU_INTR0);
    let bus = HalI2c::new(p.I2C0, i2c_config(100))
        .unwrap()
        .with_sda(p.GPIO21)
        .with_scl(p.GPIO22);
    static STACK: StaticCell<Stack<4096>> = StaticCell::new();
    esp_rtos::start_second_core(
        p.CPU_CTRL,
        p.FROM_CPU_INTR1,
        STACK.init(Stack::new()),
        move || {
            static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
            EXECUTOR
                .init(esp_rtos::embassy::Executor::new())
                .run(move |spawner| {
                    spawner.spawn(owner(bus).unwrap());
                });
        },
    );
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR
        .init(esp_rtos::embassy::Executor::new())
        .run(|spawner| {
            spawner.spawn(exercise().unwrap());
        })
}

#[embassy_executor::task]
async fn owner(blocking: HalI2c<'static, Blocking>) {
    assert_eq!(Cpu::current(), Cpu::AppCpu);
    static BUS: StaticCell<SharedI2cBus> = StaticCell::new();
    // Conversion binds the HAL interrupt on CPU1. No async driver is sent.
    let bus = BUS.init(shared_i2c_bus(blocking));
    bus.lock().await.finish_startup();
    let mut local = LocalBus {
        standard: shared_i2c_device(bus),
        panel: panel_i2c_device(bus),
    };
    console::println!("BUS_OWNER_PROBE ready core=1");
    READY.signal(());
    // Deliberately exceed the queue deadline for the first harmless read.
    Timer::after_millis(70).await;
    PROXY.serve(&mut local).await
}

#[embassy_executor::task]
async fn exercise() {
    assert_eq!(Cpu::current(), Cpu::ProCpu);
    if with_timeout(Duration::from_secs(2), READY.wait())
        .await
        .is_err()
    {
        console::println!("BUS_OWNER_PROBE failed stage=startup");
        return;
    }
    let mut standard = PROXY.device(BusRate::Standard);
    let mut panel = PROXY.device(BusRate::Panel);
    let mut rtc = [0; 11];
    let expired = standard.write_read(0x51, &[0], &mut rtc).await;
    assert!(matches!(expired, Err(ProxyError::QueueTimeout)));
    Timer::after_millis(50).await;
    standard.write_read(0x51, &[0], &mut rtc).await.unwrap();
    let (result, ()) =
        embassy_futures::join::join(standard.write_read(0x51, &[0], &mut rtc), async {
            Timer::after_micros(100).await;
            // Same local mask as a scan: no cross-core critical-section lock.
            use esp_sync::raw::{RawLock, SingleCoreInterruptLock};
            let lock = SingleCoreInterruptLock;
            let state = unsafe { lock.enter() };
            let start = esp_hal::time::Instant::now();
            while start.elapsed().as_millis() < 25 {
                core::hint::spin_loop();
            }
            unsafe { lock.exit(state) };
        })
        .await;
    result.unwrap();
    let transfer_max = TRANSFER_MAX_US.load(Ordering::Relaxed);
    assert!(transfer_max < 4_000);
    console::println!(
        "BUS_OWNER_PROBE masked_cpu0_ms=25 transfer_max_us={}",
        transfer_max
    );
    let mut chip_id = [0; 1];
    standard
        .write_read(0x76, &[0xd0], &mut chip_id)
        .await
        .unwrap();
    assert_eq!(chip_id, [0x61]);
    panel.write_read(0x20, &[0], &mut chip_id).await.unwrap();
    assert!(matches!(
        standard.read(0x7f, &mut chip_id).await,
        Err(ProxyError::Bus(_))
    ));
    standard.write_read(0x51, &[0], &mut rtc).await.unwrap();
    let stalled = standard.read(0x7e, &mut chip_id).await;
    assert!(matches!(stalled, Err(ProxyError::TransferTimeout)));
    standard.write_read(0x51, &[0], &mut rtc).await.unwrap();
    // Report the I2C interrupt on both cores from the existing IRQ hooks.
    for core in 0..2 {
        let mut report = cpu_load::profile::Snapshot::new();
        cpu_load::profile_snapshot(core, &mut report);
        let irq = report.counter(true, esp_hal::peripherals::Interrupt::I2C_EXT0 as usize);
        console::println!("BUS_OWNER_PROBE irq core={} calls={}", core, irq.calls);
        if core == 0 {
            assert_eq!(irq.calls, 0);
        } else {
            assert!(irq.calls > 0);
        }
    }
    console::println!("BUS_OWNER_PROBE complete queue_expiry=ok rates=100,400 nack_recovery=ok synthetic_deadline_recovery=ok");
}
