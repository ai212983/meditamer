//! Reset-surviving liveness evidence for a serial-silent whole-device stall.
//!
//! This intentionally uses the timer-group's reset action, not an unproven
//! interrupt route. Runtime words use atomics across cores; boot reads the record
//! before either executor starts. A reset, not a recovery flash, is required
//! to preserve the RTC-slow section.

use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration, Timer};
use esp_hal::{
    peripherals::TIMG1,
    time::Duration as HalDuration,
    timer::timg::{MwdtStage, Wdt},
};

const MAGIC: u32 = 0x5354_4c34; // STL4: adds cross-core CPU1 liveness evidence
pub(super) const WORDS: usize = 26;
const MAGIC_WORD: usize = 0;
// The bootloader can overwrite RTC word 0 on reset; this tail marker is the
// authoritative commit, beyond the vulnerable base word.
const MAGIC_TAIL: usize = 25;
const CPU0_TICKS: usize = 1;
const CPU1_TICKS: usize = 2;
const CPU0_PHASE: usize = 3;
const CPU1_PHASE: usize = 4;
const SELF_TEST: usize = 5;
const PC_CURSOR: usize = 6;
const PC_RING: usize = 7;
const PC_SAMPLES: usize = 8;
const CPU1_PC_RING: usize = 15;
const CONNECT_STALL: usize = 23;
const CPU1_STALL: usize = 24;

#[link_section = ".rtc_slow.persistent"]
static RECORD: [AtomicU32; WORDS] = [const { AtomicU32::new(0) }; WORDS];

fn word(index: usize) -> &'static AtomicU32 {
    &RECORD[index]
}

pub(super) fn take() -> Option<[u32; WORDS]> {
    // Called once during single-core boot before either task starts.
    let mut result = [0; WORDS];
    for (index, item) in result.iter_mut().enumerate() {
        *item = word(index).load(Ordering::Relaxed);
    }
    word(MAGIC_WORD).store(0, Ordering::Relaxed);
    word(MAGIC_TAIL).store(0, Ordering::Release);
    (result[MAGIC_TAIL] == MAGIC).then_some(result)
}

pub(super) fn print_record(label: &str, record: [u32; WORDS]) {
    console::println!(
        "{} cpu0_ticks={} cpu1_ticks={} cpu0_phase={} cpu1_phase={} self_test={} pc_cursor={}",
        label,
        record[CPU0_TICKS],
        record[CPU1_TICKS],
        record[CPU0_PHASE],
        record[CPU1_PHASE],
        record[SELF_TEST],
        record[PC_CURSOR]
    );
    console::println!(
        "STALL_PC_RING a=0x{:08x},0x{:08x},0x{:08x},0x{:08x} b=0x{:08x},0x{:08x},0x{:08x},0x{:08x}",
        record[PC_RING],
        record[PC_RING + 1],
        record[PC_RING + 2],
        record[PC_RING + 3],
        record[PC_RING + 4],
        record[PC_RING + 5],
        record[PC_RING + 6],
        record[PC_RING + 7]
    );
    console::println!(
        "STALL_CPU1_PC_RING a=0x{:08x},0x{:08x},0x{:08x},0x{:08x} b=0x{:08x},0x{:08x},0x{:08x},0x{:08x} connect_stall=0x{:08x} cpu1_stall_tick={}",
        record[CPU1_PC_RING], record[CPU1_PC_RING + 1],
        record[CPU1_PC_RING + 2], record[CPU1_PC_RING + 3],
        record[CPU1_PC_RING + 4], record[CPU1_PC_RING + 5],
        record[CPU1_PC_RING + 6], record[CPU1_PC_RING + 7],
        record[CONNECT_STALL], record[CPU1_STALL]
    );
}

pub(super) fn begin(self_test_complete: bool) {
    // Called on CPU0 before CPU1's heartbeat task starts.
    for index in 1..MAGIC_TAIL {
        word(index).store(0, Ordering::Relaxed);
    }
    word(SELF_TEST).store(u32::from(self_test_complete), Ordering::Relaxed);
    word(MAGIC_WORD).store(MAGIC, Ordering::Relaxed);
    word(MAGIC_TAIL).store(MAGIC, Ordering::Release);
}

pub(super) fn cpu0_phase(phase: u32) {
    word(CPU0_PHASE).store(phase, Ordering::Relaxed);
}

pub(super) fn cpu1_phase(phase: u32) {
    word(CPU1_PHASE).store(phase, Ordering::Relaxed);
}

#[embassy_executor::task]
pub(super) async fn cpu1_heartbeat() {
    let mut tick = 0u32;
    loop {
        tick = tick.wrapping_add(1);
        word(CPU1_TICKS).store(tick, Ordering::Release);
        // The ROM enables DPORT PC recording. Sampling from the other
        // core remains possible when CPU0's executor no longer runs.
        let pc = esp_hal::peripherals::DPORT::regs()
            .pro_cpu_record_pdebugpc()
            .read()
            .record_pro_pdebugpc()
            .bits();
        let index = ((tick - 1) as usize) % PC_SAMPLES;
        word(PC_RING + index).store(pc, Ordering::Relaxed);
        word(PC_CURSOR).store(tick, Ordering::Release);
        Timer::after(Duration::from_secs(2)).await;
    }
}

#[embassy_executor::task]
pub(super) async fn cpu0_watchdog(
    mut wdt: Wdt<TIMG1<'static>>,
    self_test_complete: bool,
    previous: Option<[u32; WORDS]>,
) {
    let self_test = option_env!("MEDITAMER_STALL_WDT_SELF_TEST").is_some();
    let timeout_s = if self_test && !self_test_complete {
        30
    } else {
        90
    };
    wdt.set_timeout(MwdtStage::Stage0, HalDuration::from_secs(timeout_s));
    wdt.enable();
    console::println!(
        "STALL_WATCHDOG armed timeout_s={} self_test={} completed={}",
        timeout_s,
        self_test,
        self_test_complete
    );
    let mut tick = 0u32;
    let mut last_cpu1_tick = 0u32;
    let mut unchanged_cpu1_ticks = 0u8;
    #[cfg(all(feature = "context-probe", feature = "asset-upload-http"))]
    let mut last_connect_state = 0u32;
    #[cfg(all(feature = "context-probe", feature = "asset-upload-http"))]
    let mut unchanged_connect_ticks = 0u16;
    loop {
        tick = tick.wrapping_add(1);
        word(CPU0_TICKS).store(tick, Ordering::Relaxed);
        let pc = esp_hal::peripherals::DPORT::regs()
            .app_cpu_record_pdebugpc()
            .read()
            .record_app_pdebugpc()
            .bits();
        word(CPU1_PC_RING + ((tick - 1) as usize % PC_SAMPLES)).store(pc, Ordering::Relaxed);
        if tick == 5 {
            if let Some(record) = previous {
                print_record("STALL_PREVIOUS", record);
            }
        }
        if self_test && !self_test_complete && tick == 4 {
            // Mark the deliberate stop. A subsequent boot must print this
            // retained record and must feed normally instead of looping.
            word(SELF_TEST).store(1, Ordering::Relaxed);
            console::println!("STALL_WATCHDOG self_test_stopped_feeding tick={tick}");
            loop {
                Timer::after(Duration::from_secs(2)).await;
            }
        }
        #[cfg(all(feature = "context-probe", feature = "asset-upload-http"))]
        {
            let state = esp_radio::wifi::connect_probe::current();
            unchanged_connect_ticks = if state != 0 && state == last_connect_state {
                unchanged_connect_ticks.saturating_add(1)
            } else {
                0
            };
            last_connect_state = state;
            let phase = state & 0xff;
            // A synchronous connect entry should return promptly. Event wait
            // can legitimately last up to the 180 s policy maximum; only a
            // longer stationary wait is diagnostic evidence. In either case
            // stop feeding the existing timer-group watchdog so its reset
            // preserves both cores' PC samples and this exact phase.
            if (matches!(phase, 1..=4) && unchanged_connect_ticks >= 10)
                || (phase == esp_radio::wifi::connect_probe::EVENT_WAIT
                    && unchanged_connect_ticks >= 100)
            {
                word(CONNECT_STALL).store(state, Ordering::Release);
                loop {
                    Timer::after(Duration::from_secs(2)).await;
                }
            }
        }
        let cpu1_tick = word(CPU1_TICKS).load(Ordering::Acquire);
        unchanged_cpu1_ticks = if cpu1_tick != 0 && cpu1_tick == last_cpu1_tick {
            unchanged_cpu1_ticks.saturating_add(1)
        } else {
            0
        };
        last_cpu1_tick = cpu1_tick;
        if unchanged_cpu1_ticks >= 15 {
            // CPU0 is alive but CPU1's independent heartbeat has stopped for
            // at least 30 s. Preserve that tick and stop feeding TIMG1 so its
            // hardware reset prints both PC rings on the next boot.
            word(CPU1_STALL).store(cpu1_tick, Ordering::Release);
            loop {
                Timer::after(Duration::from_secs(2)).await;
            }
        }
        wdt.feed();
        Timer::after(Duration::from_secs(2)).await;
    }
}
