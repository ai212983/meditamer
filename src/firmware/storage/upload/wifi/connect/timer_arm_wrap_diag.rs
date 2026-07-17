use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use esp_println::println;

const SLOT_COUNT: usize = 8;

static COUNT: AtomicU32 = AtomicU32::new(0);
static RECENT_ORDINALS: [AtomicU32; SLOT_COUNT] = [const { AtomicU32::new(0) }; SLOT_COUNT];
static RECENT_TIMER_PTRS: [AtomicUsize; SLOT_COUNT] = [const { AtomicUsize::new(0) }; SLOT_COUNT];
static RECENT_CALLBACK_PTRS: [AtomicUsize; SLOT_COUNT] =
    [const { AtomicUsize::new(0) }; SLOT_COUNT];
static RECENT_ARG_PTRS: [AtomicUsize; SLOT_COUNT] = [const { AtomicUsize::new(0) }; SLOT_COUNT];
static RECENT_CALLER_PTRS: [AtomicUsize; SLOT_COUNT] = [const { AtomicUsize::new(0) }; SLOT_COUNT];
static RECENT_TIMEOUT_US: [AtomicU32; SLOT_COUNT] = [const { AtomicU32::new(0) }; SLOT_COUNT];
static RECENT_PERIODIC: [AtomicU32; SLOT_COUNT] = [const { AtomicU32::new(0) }; SLOT_COUNT];

unsafe extern "Rust" {
    fn __real_esp_rtos_timer_arm(timer: NonNull<()>, timeout: u64, periodic: bool);
}

#[cfg(target_arch = "xtensa")]
fn current_caller_ptr() -> usize {
    let caller_ptr: usize;
    unsafe {
        core::arch::asm!("mov {0}, a0", out(reg) caller_ptr);
    }
    caller_ptr
}

#[cfg(not(target_arch = "xtensa"))]
fn current_caller_ptr() -> usize {
    0
}

fn record(
    timer_ptr: usize,
    callback_ptr: usize,
    arg_ptr: usize,
    caller_ptr: usize,
    timeout_us: u64,
    periodic: bool,
) {
    let ordinal = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let idx = (ordinal as usize) % SLOT_COUNT;
    RECENT_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    RECENT_TIMER_PTRS[idx].store(timer_ptr, Ordering::Relaxed);
    RECENT_CALLBACK_PTRS[idx].store(callback_ptr, Ordering::Relaxed);
    RECENT_ARG_PTRS[idx].store(arg_ptr, Ordering::Relaxed);
    RECENT_CALLER_PTRS[idx].store(caller_ptr, Ordering::Relaxed);
    RECENT_TIMEOUT_US[idx].store(timeout_us as u32, Ordering::Relaxed);
    RECENT_PERIODIC[idx].store(periodic as u32, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
unsafe extern "Rust" fn __wrap_esp_rtos_timer_arm(
    timer: NonNull<()>,
    timeout: u64,
    periodic: bool,
) {
    let timer_ptr = timer.as_ptr() as usize;
    let live = esp_radio::diagnostic_timer_live_diag(timer_ptr);
    record(
        timer_ptr,
        live.callback_ptr,
        live.callback_arg_ptr,
        current_caller_ptr(),
        timeout,
        periodic,
    );
    unsafe { __real_esp_rtos_timer_arm(timer, timeout, periodic) }
}

pub(super) fn reset_timer_arm_wrap_diag() {
    COUNT.store(0, Ordering::Relaxed);
    for idx in 0..SLOT_COUNT {
        RECENT_ORDINALS[idx].store(0, Ordering::Relaxed);
        RECENT_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        RECENT_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        RECENT_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        RECENT_CALLER_PTRS[idx].store(0, Ordering::Relaxed);
        RECENT_TIMEOUT_US[idx].store(0, Ordering::Relaxed);
        RECENT_PERIODIC[idx].store(0, Ordering::Relaxed);
    }
}

pub(super) fn log_timer_arm_wrap_diag(stage: &str) {
    println!(
        "upload_http: boot_scan_only_diag timer_arm_wrap_diag after={} count={}",
        stage,
        COUNT.load(Ordering::Relaxed)
    );
    for idx in 0..SLOT_COUNT {
        let ordinal = RECENT_ORDINALS[idx].load(Ordering::Relaxed);
        if ordinal == 0 {
            continue;
        }
        println!(
            "upload_http: boot_scan_only_diag timer_arm_wrap_recent after={} idx={} ordinal={} timer_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x} caller_ptr=0x{:x} timeout_us={} periodic={}",
            stage,
            idx,
            ordinal,
            RECENT_TIMER_PTRS[idx].load(Ordering::Relaxed),
            RECENT_CALLBACK_PTRS[idx].load(Ordering::Relaxed),
            RECENT_ARG_PTRS[idx].load(Ordering::Relaxed),
            RECENT_CALLER_PTRS[idx].load(Ordering::Relaxed),
            RECENT_TIMEOUT_US[idx].load(Ordering::Relaxed),
            RECENT_PERIODIC[idx].load(Ordering::Relaxed),
        );
    }
}
