use esp_println::println;
use esp_wifi_sys::{c_types, include::ets_timer};

unsafe extern "C" {
    static mut g_chm: u8;
}

#[repr(C)]
#[derive(Copy, Clone)]
struct TimerCallback {
    f: unsafe extern "C" fn(*mut c_types::c_void),
    args: *mut c_types::c_void,
}

#[repr(C)]
struct LegacyTimer {
    ets_timer: *mut ets_timer,
    started: u64,
    timeout: u64,
    active: bool,
    periodic: bool,
    callback: TimerCallback,
    next: *mut LegacyTimer,
}

fn read_u32(base: usize, offset: usize) -> u32 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn read_ptr(base: usize, offset: usize) -> usize {
    read_u32(base, offset) as usize
}

fn g_chm_ptr() -> usize {
    let g_chm_slot_ptr = unsafe { core::ptr::addr_of!(g_chm) as usize };
    read_ptr(g_chm_slot_ptr, 0)
}

fn log_slot(label: &str, phase: &str, slot: usize, ets_timer_ptr: usize) {
    let timer_handle_ptr = if ets_timer_ptr == 0 {
        0
    } else {
        let ets = unsafe { &*(ets_timer_ptr as *const ets_timer) };
        ets.priv_ as usize
    };

    if timer_handle_ptr == 0 {
        println!(
            "legacy_nostd_wifi_control: chm_timer_slot_live label={} phase={} slot={} ets_timer_ptr=0x{:08x} timer_handle_ptr=0x0 callback_ptr=0x0 arg_ptr=0x0 active=0 started_us=0 next_due_us=0 period_us=0 periodic=0",
            label, phase, slot, ets_timer_ptr
        );
        return;
    }

    let timer = unsafe { &*(timer_handle_ptr as *const LegacyTimer) };
    let started_us = timer.started;
    let period_us = timer.timeout;
    let next_due_us = started_us.saturating_add(period_us);
    println!(
        "legacy_nostd_wifi_control: chm_timer_slot_live label={} phase={} slot={} ets_timer_ptr=0x{:08x} timer_handle_ptr=0x{:08x} callback_ptr=0x{:08x} arg_ptr=0x{:08x} active={} started_us={} next_due_us={} period_us={} periodic={}",
        label,
        phase,
        slot,
        ets_timer_ptr,
        timer_handle_ptr,
        timer.callback.f as usize,
        timer.callback.args as usize,
        timer.active as u32,
        started_us,
        next_due_us,
        period_us,
        timer.periodic as u32,
    );
}

pub(crate) fn print_chm_timer_slot_live(label: &str, phase: &str) {
    let chm_ptr = g_chm_ptr();
    println!(
        "legacy_nostd_wifi_control: chm_timer_slot_diag label={} phase={} chm_ptr=0x{:08x}",
        label, phase, chm_ptr
    );
    log_slot(label, phase, 0, chm_ptr.saturating_add(36));
    log_slot(label, phase, 1, chm_ptr.saturating_add(56));
}
