//! Bounded validation of per-core idle/full/half-load accounting.
#![no_std]
#![no_main]
use core::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::{clock::CpuClock, system::Stack};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();
static PHASE: AtomicU32 = AtomicU32::new(0);
static IRQ_TIMER: critical_section::Mutex<
    RefCell<Option<esp_hal::timer::PeriodicTimer<'static, esp_hal::Blocking>>>,
> = critical_section::Mutex::new(RefCell::new(None));

#[esp_hal::handler(priority = esp_hal::interrupt::Priority::Priority2)]
fn irq_work() {
    critical_section::with(|cs| {
        if let Some(timer) = IRQ_TIMER.borrow_ref_mut(cs).as_mut() {
            timer.clear_interrupt();
        }
    });
    if PHASE.load(Ordering::Relaxed) == 5 {
        let start = esp_hal::time::Instant::now();
        while start.elapsed().as_micros() < 1_000 {
            core::hint::spin_loop();
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));
    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    let timg1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
    let mut timer = esp_hal::timer::PeriodicTimer::new(timg1.timer0);
    timer.set_interrupt_handler(irq_work);
    timer
        .start(esp_hal::time::Duration::from_millis(5))
        .unwrap();
    timer.listen();
    critical_section::with(|cs| IRQ_TIMER.borrow_ref_mut(cs).replace(timer));
    static STACK: StaticCell<Stack<8192>> = StaticCell::new();
    esp_rtos::start_second_core(
        peripherals.CPU_CTRL,
        peripherals.FROM_CPU_INTR1,
        STACK.init(Stack::new()),
        || {
            static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
            EXECUTOR
                .init(esp_rtos::embassy::Executor::new())
                .run(|spawner| {
                    spawner.spawn(worker(1).unwrap());
                })
        },
    );
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR
        .init(esp_rtos::embassy::Executor::new())
        .run(|spawner| {
            spawner.spawn(worker(0).unwrap());
            spawner.spawn(report().unwrap());
        })
}

#[embassy_executor::task(pool_size = 2)]
async fn worker(core: u32) {
    loop {
        let phase = PHASE.load(Ordering::Relaxed);
        let busy_us = match phase {
            1 if core == 0 => 10_000,
            2 if core == 1 => 10_000,
            3 => 5_000,
            4 => 10_000,
            _ => 0,
        };
        if busy_us != 0 {
            let start = esp_hal::time::Instant::now();
            while start.elapsed().as_micros() < busy_us {
                core::hint::spin_loop();
            }
        }
        if busy_us < 10_000 {
            Timer::after(Duration::from_micros(10_000 - busy_us)).await;
        } else {
            embassy_futures::yield_now().await;
        }
    }
}

#[embassy_executor::task]
async fn report() {
    for phase in 0..=5 {
        PHASE.store(phase, Ordering::Relaxed);
        Timer::after(Duration::from_millis(100)).await;
        let _ = cpu_load::sample();
        for window in 0..2 {
            Timer::after(Duration::from_secs(5)).await;
            let readings = cpu_load::sample();
            for (core, reading) in readings.into_iter().enumerate() {
                if let Some(reading) = reading {
                    console::println!("CPU_LOAD_PROBE phase={} window={} core={} elapsed_us={} busy_us={} irq_us={} percent={}",
                        phase, window, core, reading.elapsed_us, reading.busy_us, reading.interrupt_us, reading.percent());
                }
            }
        }
    }
    PHASE.store(0, Ordering::Relaxed);
    console::println!("CPU_LOAD_PROBE complete");
}
