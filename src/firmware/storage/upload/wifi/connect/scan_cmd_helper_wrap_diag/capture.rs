use super::*;

unsafe extern "C" {
    static mut g_ic: u8;
    static mut g_chm: u8;
    static mut app_scan_params: u8;
}

fn force_chm_arg2() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SCAN_CMD_HELPER_WRAP_FORCE_CHM_ARG2"),
        Some("1")
    ) || matches!(
        option_env!("WIFI_SCAN_CMD_HELPER_WRAP_FORCE_CHM_ARG2"),
        Some("1")
    )
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

unsafe fn read_ptr(ptr: usize, offset: usize) -> usize {
    ((ptr + offset) as *const u32).read_unaligned() as usize
}

unsafe fn maybe_rewrite_scan_build_chan_list_arg2(arg2: usize) -> usize {
    if !force_chm_arg2() {
        return arg2;
    }
    let g_ic_slot_ptr = core::ptr::addr_of!(g_ic) as usize;
    let sta_ptr = read_ptr(g_ic_slot_ptr, 0x10);
    if arg2 != sta_ptr {
        return arg2;
    }
    let g_chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0);
    if chm_ptr == 0 {
        return arg2;
    }
    chm_ptr + 0x50
}

fn store_snapshot(fn_id: u8, args: [usize; 4], call_arg2: usize, ret: usize) {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if slot >= SLOT_COUNT {
        return;
    }
    unsafe {
        SNAPSHOTS[slot] = Snapshot {
            fn_id,
            args: [
                args[0] as u32,
                args[1] as u32,
                args[2] as u32,
                args[3] as u32,
            ],
            call_arg2: call_arg2 as u32,
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

pub(super) unsafe fn wrap_call(
    fn_id: u8,
    real: unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let call_arg2 = if fn_id == 3 {
        unsafe { maybe_rewrite_scan_build_chan_list_arg2(a4) }
    } else {
        a4
    };
    let pre_app_scan_params = capture_app_scan_params();
    let pre_arg2 = capture_ptr_bytes(call_arg2);
    let pre_arg3 = capture_ptr_bytes(a5);
    let ret = unsafe { real(a2, a3, call_arg2, a5, a6, a7) };
    let post_app_scan_params = capture_app_scan_params();
    let post_arg2 = capture_ptr_bytes(call_arg2);
    let post_arg3 = capture_ptr_bytes(a5);
    store_snapshot(fn_id, [a2, a3, a4, a5], call_arg2, ret);
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
