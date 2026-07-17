use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;
use esp_wifi_sys::include::ets_timer;

const RING: usize = 12;

#[derive(Copy, Clone)]
struct SetfnEntry {
    ordinal: u32,
    timer_ptr: u32,
    timer_handle_ptr: u32,
    callback_ptr: u32,
    arg_ptr: u32,
    caller_ptr: u32,
}

impl SetfnEntry {
    const ZERO: Self = Self {
        ordinal: 0,
        timer_ptr: 0,
        timer_handle_ptr: 0,
        callback_ptr: 0,
        arg_ptr: 0,
        caller_ptr: 0,
    };
}

#[derive(Copy, Clone)]
struct ArmEntry {
    ordinal: u32,
    timer_ptr: u32,
    timer_handle_ptr: u32,
    callback_ptr: u32,
    arg_ptr: u32,
    caller_ptr: u32,
    timeout_us: u32,
    repeat: bool,
    kind: u8,
}

impl ArmEntry {
    const ZERO: Self = Self {
        ordinal: 0,
        timer_ptr: 0,
        timer_handle_ptr: 0,
        callback_ptr: 0,
        arg_ptr: 0,
        caller_ptr: 0,
        timeout_us: 0,
        repeat: false,
        kind: 0,
    };
}

static SETFN_NEXT: AtomicUsize = AtomicUsize::new(0);
static ARM_NEXT: AtomicUsize = AtomicUsize::new(0);

static mut SETFN: [SetfnEntry; RING] = [SetfnEntry::ZERO; RING];
static mut ARM: [ArmEntry; RING] = [ArmEntry::ZERO; RING];

#[derive(Copy, Clone)]
pub struct TimerWrapCounts {
    pub setfn_count: u32,
    pub arm_count: u32,
}

unsafe extern "C" {
    fn __real_ets_timer_setfn(
        ptimer: *mut esp_wifi_sys::c_types::c_void,
        pfunction: *mut esp_wifi_sys::c_types::c_void,
        parg: *mut esp_wifi_sys::c_types::c_void,
    );
    fn __real_ets_timer_arm(
        ptimer: *mut esp_wifi_sys::c_types::c_void,
        timeout_ms: u32,
        repeat: bool,
    );
    fn __real_ets_timer_arm_us(
        ptimer: *mut esp_wifi_sys::c_types::c_void,
        timeout_us: u32,
        repeat: bool,
    );
    fn __real_ets_timer_disarm(ptimer: *mut esp_wifi_sys::c_types::c_void);
}

#[cfg(target_arch = "xtensa")]
fn current_caller_ptr() -> usize {
    let caller: usize;
    unsafe { core::arch::asm!("mov {0}, a0", out(reg) caller); }
    caller
}

#[cfg(not(target_arch = "xtensa"))]
fn current_caller_ptr() -> usize {
    0
}

unsafe fn timer_handle_ptr(ptimer: *mut esp_wifi_sys::c_types::c_void) -> usize {
    if ptimer.is_null() {
        return 0;
    }
    unsafe { (*(ptimer as *mut ets_timer)).priv_ as usize }
}

fn last_callback_for_timer(timer_ptr: usize, timer_handle_ptr: usize) -> (usize, usize) {
    let next = SETFN_NEXT.load(Ordering::Relaxed);
    let mut seen = 0usize;
    while seen < RING {
        let idx = next.wrapping_sub(1 + seen) % RING;
        let entry = unsafe { SETFN[idx] };
        if entry.ordinal != 0
            && (entry.timer_handle_ptr as usize == timer_handle_ptr
                || entry.timer_ptr as usize == timer_ptr)
        {
            return (entry.callback_ptr as usize, entry.arg_ptr as usize);
        }
        seen += 1;
    }
    (0, 0)
}

pub fn reset_timer_wrap_diag() {
    SETFN_NEXT.store(0, Ordering::Relaxed);
    ARM_NEXT.store(0, Ordering::Relaxed);
    unsafe {
        SETFN = [SetfnEntry::ZERO; RING];
        ARM = [ArmEntry::ZERO; RING];
    }
}

pub fn snapshot_timer_wrap_counts() -> TimerWrapCounts {
    TimerWrapCounts {
        setfn_count: SETFN_NEXT.load(Ordering::Relaxed) as u32,
        arm_count: ARM_NEXT.load(Ordering::Relaxed) as u32,
    }
}

pub fn print_timer_wrap_diag(label: &str, phase: &str) {
    let setfn_count = SETFN_NEXT.load(Ordering::Relaxed);
    let arm_count = ARM_NEXT.load(Ordering::Relaxed);
    println!(
        "legacy_nostd_wifi_control: timer_wrap_diag label={} phase={} setfn_count={} arm_count={}",
        label, phase, setfn_count, arm_count
    );

    let mut idx = 0usize;
    while idx < RING {
        let entry = unsafe { SETFN[idx] };
        if entry.ordinal != 0 {
            println!(
                "legacy_nostd_wifi_control: timer_wrap_setfn label={} phase={} idx={} ordinal={} timer_ptr=0x{:08x} timer_handle_ptr=0x{:08x} callback_ptr=0x{:08x} arg_ptr=0x{:08x} caller_ptr=0x{:08x}",
                label, phase, idx, entry.ordinal, entry.timer_ptr, entry.timer_handle_ptr, entry.callback_ptr, entry.arg_ptr, entry.caller_ptr
            );
        }
        idx += 1;
    }

    let mut idx = 0usize;
    while idx < RING {
        let entry = unsafe { ARM[idx] };
        if entry.ordinal != 0 {
            let kind = match entry.kind {
                1 => "arm_ms",
                2 => "arm_us",
                3 => "disarm",
                _ => "unknown",
            };
            println!(
                "legacy_nostd_wifi_control: timer_wrap_arm label={} phase={} idx={} ordinal={} kind={} timer_ptr=0x{:08x} timer_handle_ptr=0x{:08x} callback_ptr=0x{:08x} arg_ptr=0x{:08x} caller_ptr=0x{:08x} timeout_us={} repeat={}",
                label,
                phase,
                idx,
                entry.ordinal,
                kind,
                entry.timer_ptr,
                entry.timer_handle_ptr,
                entry.callback_ptr,
                entry.arg_ptr,
                entry.caller_ptr,
                entry.timeout_us,
                entry.repeat
            );
        }
        idx += 1;
    }
}

unsafe fn record_setfn(
    ptimer: *mut esp_wifi_sys::c_types::c_void,
    pfunction: *mut esp_wifi_sys::c_types::c_void,
    parg: *mut esp_wifi_sys::c_types::c_void,
) {
    let ordinal = SETFN_NEXT.fetch_add(1, Ordering::Relaxed) as u32;
    let idx = ordinal as usize % RING;
    unsafe {
        SETFN[idx] = SetfnEntry {
            ordinal: ordinal + 1,
            timer_ptr: ptimer as usize as u32,
            timer_handle_ptr: timer_handle_ptr(ptimer) as u32,
            callback_ptr: pfunction as usize as u32,
            arg_ptr: parg as usize as u32,
            caller_ptr: current_caller_ptr() as u32,
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ets_timer_setfn(
    ptimer: *mut esp_wifi_sys::c_types::c_void,
    pfunction: *mut esp_wifi_sys::c_types::c_void,
    parg: *mut esp_wifi_sys::c_types::c_void,
) {
    unsafe {
        __real_ets_timer_setfn(ptimer, pfunction, parg);
        record_setfn(ptimer, pfunction, parg);
    }
}

unsafe fn record_arm(
    kind: u8,
    ptimer: *mut esp_wifi_sys::c_types::c_void,
    timeout_us: u32,
    repeat: bool,
) {
    let ordinal = ARM_NEXT.fetch_add(1, Ordering::Relaxed) as u32;
    let idx = ordinal as usize % RING;
    let timer_ptr = ptimer as usize;
    let timer_handle_ptr = unsafe { timer_handle_ptr(ptimer) };
    let (callback_ptr, arg_ptr) = last_callback_for_timer(timer_ptr, timer_handle_ptr);
    unsafe {
        ARM[idx] = ArmEntry {
            ordinal: ordinal + 1,
            timer_ptr: timer_ptr as u32,
            timer_handle_ptr: timer_handle_ptr as u32,
            callback_ptr: callback_ptr as u32,
            arg_ptr: arg_ptr as u32,
            caller_ptr: current_caller_ptr() as u32,
            timeout_us,
            repeat,
            kind,
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ets_timer_arm(
    ptimer: *mut esp_wifi_sys::c_types::c_void,
    timeout_ms: u32,
    repeat: bool,
) {
    unsafe {
        record_arm(1, ptimer, timeout_ms.saturating_mul(1000), repeat);
        __real_ets_timer_arm(ptimer, timeout_ms, repeat);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ets_timer_arm_us(
    ptimer: *mut esp_wifi_sys::c_types::c_void,
    timeout_us: u32,
    repeat: bool,
) {
    unsafe {
        record_arm(2, ptimer, timeout_us, repeat);
        __real_ets_timer_arm_us(ptimer, timeout_us, repeat);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ets_timer_disarm(ptimer: *mut esp_wifi_sys::c_types::c_void) {
    unsafe {
        record_arm(3, ptimer, 0, false);
        __real_ets_timer_disarm(ptimer);
    }
}
