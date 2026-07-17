use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 24;
const PTR_BYTES: usize = 8;
const APP_SCAN_PARAMS_BYTES: usize = 16;

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    ret: u32,
    pre_app_scan_params: [u8; APP_SCAN_PARAMS_BYTES],
    post_app_scan_params: [u8; APP_SCAN_PARAMS_BYTES],
    pre_arg2: [u8; PTR_BYTES],
    post_arg2: [u8; PTR_BYTES],
    pre_arg3: [u8; PTR_BYTES],
    post_arg3: [u8; PTR_BYTES],
}

impl Snapshot {
    const ZERO: Self = Self {
        fn_id: 0,
        args: [0; 4],
        ret: 0,
        pre_app_scan_params: [0; APP_SCAN_PARAMS_BYTES],
        post_app_scan_params: [0; APP_SCAN_PARAMS_BYTES],
        pre_arg2: [0; PTR_BYTES],
        post_arg2: [0; PTR_BYTES],
        pre_arg3: [0; PTR_BYTES],
        post_arg3: [0; PTR_BYTES],
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    static mut app_scan_params: u8;
    fn __real_scan_hidden_ssid(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_set_current_scan_times(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_build_chan_list(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_set_desChan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ieee80211_regdomain_chan_in_range(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ieee80211_regdomain_min_chan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ieee80211_regdomain_max_chan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

fn capture_ptr_bytes(ptr: usize) -> [u8; PTR_BYTES] {
    if !(0x3ff0_0000..0x4000_0000).contains(&ptr) {
        return [0; PTR_BYTES];
    }
    let mut out = [0u8; PTR_BYTES];
    let mut idx = 0usize;
    while idx < PTR_BYTES {
        out[idx] = unsafe { ((ptr + idx) as *const u8).read_volatile() };
        idx += 1;
    }
    out
}

fn capture_app_scan_params() -> [u8; APP_SCAN_PARAMS_BYTES] {
    let ptr = core::ptr::addr_of!(app_scan_params) as usize;
    let mut out = [0u8; APP_SCAN_PARAMS_BYTES];
    let mut idx = 0usize;
    while idx < APP_SCAN_PARAMS_BYTES {
        out[idx] = unsafe { ((ptr + idx) as *const u8).read_volatile() };
        idx += 1;
    }
    out
}

fn store_snapshot(fn_id: u8, args: [usize; 4], ret: usize) {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if slot >= SLOT_COUNT {
        return;
    }
    unsafe {
        SNAPSHOTS[slot] = Snapshot {
            fn_id,
            args: [args[0] as u32, args[1] as u32, args[2] as u32, args[3] as u32],
            ret: ret as u32,
            pre_app_scan_params: [0; APP_SCAN_PARAMS_BYTES],
            post_app_scan_params: [0; APP_SCAN_PARAMS_BYTES],
            pre_arg2: [0; PTR_BYTES],
            post_arg2: [0; PTR_BYTES],
            pre_arg3: [0; PTR_BYTES],
            post_arg3: [0; PTR_BYTES],
        };
    }
}

unsafe fn wrap_call(
    fn_id: u8,
    real: unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_app_scan_params = capture_app_scan_params();
    let pre_arg2 = capture_ptr_bytes(a4);
    let pre_arg3 = capture_ptr_bytes(a5);
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    let post_app_scan_params = capture_app_scan_params();
    let post_arg2 = capture_ptr_bytes(a4);
    let post_arg3 = capture_ptr_bytes(a5);
    store_snapshot(fn_id, [a2, a3, a4, a5], ret);
    let slot = NEXT_SLOT.load(Ordering::Relaxed).saturating_sub(1);
    if slot < SLOT_COUNT {
        unsafe {
            SNAPSHOTS[slot].pre_app_scan_params = pre_app_scan_params;
            SNAPSHOTS[slot].post_app_scan_params = post_app_scan_params;
            SNAPSHOTS[slot].pre_arg2 = pre_arg2;
            SNAPSHOTS[slot].post_arg2 = post_arg2;
            SNAPSHOTS[slot].pre_arg3 = pre_arg3;
            SNAPSHOTS[slot].post_arg3 = post_arg3;
        }
    }
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_hidden_ssid(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(1, __real_scan_hidden_ssid, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_current_scan_times(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(2, __real_scan_set_current_scan_times, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_build_chan_list(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(3, __real_scan_build_chan_list, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_desChan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(4, __real_scan_set_desChan, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_chan_in_range(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(5, __real_ieee80211_regdomain_chan_in_range, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_min_chan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(6, __real_ieee80211_regdomain_min_chan, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_max_chan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(7, __real_ieee80211_regdomain_max_chan, a2, a3, a4, a5, a6, a7) }
}

pub(crate) fn reset_scan_cmd_helper_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        let mut idx = 0usize;
        while idx < SLOT_COUNT {
            SNAPSHOTS[idx] = Snapshot::ZERO;
            idx += 1;
        }
    }
}

fn fn_label(fn_id: u8) -> &'static str {
    match fn_id {
        1 => "scan_hidden_ssid",
        2 => "scan_set_current_scan_times",
        3 => "scan_build_chan_list",
        4 => "scan_set_desChan",
        5 => "ieee80211_regdomain_chan_in_range",
        6 => "ieee80211_regdomain_min_chan",
        7 => "ieee80211_regdomain_max_chan",
        _ => "unknown",
    }
}

pub(crate) fn print_scan_cmd_helper_wrap_diag(label: &str, phase: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "legacy_nostd_wifi_control: scan_cmd_helper_wrap_diag label={} phase={} count={}",
        label, phase, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "legacy_nostd_wifi_control: scan_cmd_helper_wrap_diag_entry label={} phase={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} pre_app_scan_params={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_app_scan_params={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} pre_arg2={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_arg2={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} pre_arg3={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_arg3={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            label,
            phase,
            idx,
            fn_label(snap.fn_id),
            snap.ret,
            snap.args[0],
            snap.args[1],
            snap.args[2],
            snap.args[3],
            snap.pre_app_scan_params[0], snap.pre_app_scan_params[1], snap.pre_app_scan_params[2], snap.pre_app_scan_params[3],
            snap.pre_app_scan_params[4], snap.pre_app_scan_params[5], snap.pre_app_scan_params[6], snap.pre_app_scan_params[7],
            snap.pre_app_scan_params[8], snap.pre_app_scan_params[9], snap.pre_app_scan_params[10], snap.pre_app_scan_params[11],
            snap.pre_app_scan_params[12], snap.pre_app_scan_params[13], snap.pre_app_scan_params[14], snap.pre_app_scan_params[15],
            snap.post_app_scan_params[0], snap.post_app_scan_params[1], snap.post_app_scan_params[2], snap.post_app_scan_params[3],
            snap.post_app_scan_params[4], snap.post_app_scan_params[5], snap.post_app_scan_params[6], snap.post_app_scan_params[7],
            snap.post_app_scan_params[8], snap.post_app_scan_params[9], snap.post_app_scan_params[10], snap.post_app_scan_params[11],
            snap.post_app_scan_params[12], snap.post_app_scan_params[13], snap.post_app_scan_params[14], snap.post_app_scan_params[15],
            snap.pre_arg2[0], snap.pre_arg2[1], snap.pre_arg2[2], snap.pre_arg2[3],
            snap.pre_arg2[4], snap.pre_arg2[5], snap.pre_arg2[6], snap.pre_arg2[7],
            snap.post_arg2[0], snap.post_arg2[1], snap.post_arg2[2], snap.post_arg2[3],
            snap.post_arg2[4], snap.post_arg2[5], snap.post_arg2[6], snap.post_arg2[7],
            snap.pre_arg3[0], snap.pre_arg3[1], snap.pre_arg3[2], snap.pre_arg3[3],
            snap.pre_arg3[4], snap.pre_arg3[5], snap.pre_arg3[6], snap.pre_arg3[7],
            snap.post_arg3[0], snap.post_arg3[1], snap.post_arg3[2], snap.post_arg3[3],
            snap.post_arg3[4], snap.post_arg3[5], snap.post_arg3[6], snap.post_arg3[7],
        );
    }
}
