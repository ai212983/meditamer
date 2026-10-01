//! Standalone native SDMMC qualification; see the SD slice plan for prerequisites.
//! No product tasks, filesystem, radio, allocator or PSRAM. Read-only unless both
//! MEDINOTE_SD_SCRATCH_START and MEDINOTE_SD_SCRATCH_SECTORS designate scratch media.
#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

#[cfg(any(feature = "shared-ble-runtime", feature = "crash-screen"))]
compile_error!("Build the SD probe with --no-default-features --features sd-transport-probe only");

#[path = "../sd_probe/mod.rs"]
mod sd_probe;

use aligned::{Aligned, A4};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, sdmmc::SdHostController, timer::timg::TimerGroup};
use static_cell::{ConstStaticCell, StaticCell};

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_hal::delay::Delay::new().delay_millis(800);
    console::println!(
        "SD_PROBE state=boot board=waveshare-rlcd42 chip=esp32s3 hal=1.2.0 sdio=0.5.1"
    );
    let config = match sd_probe::policy::Config::parse(
        option_env!("MEDINOTE_SD_SCRATCH_START"),
        option_env!("MEDINOTE_SD_SCRATCH_SECTORS"),
    ) {
        Ok(config) => config,
        Err(error) => {
            console::println!("SD_PROBE state=done result=error config={:?}", error);
            sd_probe::halt();
        }
    };
    let fault = match sd_probe::policy::Fault::parse(option_env!("MEDINOTE_SD_FAULT"), config) {
        Ok(fault) => fault,
        Err(error) => {
            console::println!("SD_PROBE state=done result=error config={}", error);
            sd_probe::halt();
        }
    };
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0);
    static HOST: StaticCell<SdHostController<'static>> = StaticCell::new();
    let host = HOST.init(SdHostController::new(p.SDHOST, Default::default()).expect("sdhost"));
    let slot = host
        .slot::<1>(Default::default())
        .expect("slot1")
        .with_clk(p.GPIO38)
        .with_cmd(p.GPIO21)
        .with_data0(p.GPIO39)
        .into_async();
    // 1024 bytes, 4-byte aligned internal SRAM. HAL supplies its own descriptor
    // ring; no extra GDMA channel or PSRAM bounce workspace is allocated here.
    static BLOCKS: ConstStaticCell<[Aligned<A4, [u8; 512]>; 2]> =
        ConstStaticCell::new([Aligned([0; 512]), Aligned([0; 512])]);
    let blocks = BLOCKS.take();
    console::println!("SD_PROBE state=resources buffer_bytes=1024 buffer_addr={:#x} heap_bytes=0 psram_bytes=0 slot=1 width=1 irq=SDIO_HOST", blocks.as_ptr() as usize);
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR
        .init(esp_rtos::embassy::Executor::new())
        .run(|spawner| {
            spawner.spawn(sd_probe::run(slot, blocks, config, fault).unwrap());
        });
}
