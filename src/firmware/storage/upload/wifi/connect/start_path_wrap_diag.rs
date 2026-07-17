use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 12;

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    ret: u32,
    pre_timer_setfn: u32,
    post_timer_setfn: u32,
    pre_timer_arm: u32,
    post_timer_arm: u32,
    pre_sta_ptr: u32,
    post_sta_ptr: u32,
    pre_chm_ptr: u32,
    post_chm_ptr: u32,
    pre_home_chan: u8,
    post_home_chan: u8,
    pre_current_chan: u8,
    post_current_chan: u8,
    pre_op_chan: u8,
    post_op_chan: u8,
    pre_word114: u32,
    post_word114: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        fn_id: 0,
        args: [0; 4],
        ret: 0,
        pre_timer_setfn: 0,
        post_timer_setfn: 0,
        pre_timer_arm: 0,
        post_timer_arm: 0,
        pre_sta_ptr: 0,
        post_sta_ptr: 0,
        pre_chm_ptr: 0,
        post_chm_ptr: 0,
        pre_home_chan: 0,
        post_home_chan: 0,
        pre_current_chan: 0,
        post_current_chan: 0,
        pre_op_chan: 0,
        post_op_chan: 0,
        pre_word114: 0,
        post_word114: 0,
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    static mut g_ic: u8;
    static mut g_chm: u8;
    static mut g_scan: u8;

    fn __real__do_wifi_start(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_wifi_hw_start(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_chm_init(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
}

fn read_u8(base: usize, offset: usize) -> u8 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u8).read_volatile() }
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

fn capture_snapshot(fn_id: u8, args: [usize; 4], ret: usize, pre: bool) -> Snapshot {
    let timer_diag = esp_radio::diagnostic_timer_compat_diag();
    let g_ic_ptr = unsafe { core::ptr::addr_of!(g_ic) as usize };
    let g_chm_slot_ptr = unsafe { core::ptr::addr_of!(g_chm) as usize };
    let g_scan_slot_ptr = unsafe { core::ptr::addr_of!(g_scan) as usize };
    let sta_ptr = read_ptr(g_ic_ptr, 0x10) as u32;
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0) as usize;
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0) as usize;
    let mut snap = Snapshot::ZERO;
    snap.fn_id = fn_id;
    snap.args = [
        args[0] as u32,
        args[1] as u32,
        args[2] as u32,
        args[3] as u32,
    ];
    snap.ret = ret as u32;
    if pre {
        snap.pre_timer_setfn = timer_diag.setfn_count;
        snap.pre_timer_arm = timer_diag.arm_count;
        snap.pre_sta_ptr = sta_ptr;
        snap.pre_chm_ptr = chm_ptr as u32;
        snap.pre_home_chan = read_u8(chm_ptr, 0x18);
        snap.pre_current_chan = read_u8(chm_ptr, 0x1a);
        snap.pre_op_chan = read_u8(chm_ptr, 0x04);
        snap.pre_word114 = read_u32(scan_ptr, 0x114);
    } else {
        snap.post_timer_setfn = timer_diag.setfn_count;
        snap.post_timer_arm = timer_diag.arm_count;
        snap.post_sta_ptr = sta_ptr;
        snap.post_chm_ptr = chm_ptr as u32;
        snap.post_home_chan = read_u8(chm_ptr, 0x18);
        snap.post_current_chan = read_u8(chm_ptr, 0x1a);
        snap.post_op_chan = read_u8(chm_ptr, 0x04);
        snap.post_word114 = read_u32(scan_ptr, 0x114);
    }
    snap
}

fn merge(pre: Snapshot, post: Snapshot) -> Snapshot {
    Snapshot {
        fn_id: pre.fn_id,
        args: pre.args,
        ret: post.ret,
        pre_timer_setfn: pre.pre_timer_setfn,
        post_timer_setfn: post.post_timer_setfn,
        pre_timer_arm: pre.pre_timer_arm,
        post_timer_arm: post.post_timer_arm,
        pre_sta_ptr: pre.pre_sta_ptr,
        post_sta_ptr: post.post_sta_ptr,
        pre_chm_ptr: pre.pre_chm_ptr,
        post_chm_ptr: post.post_chm_ptr,
        pre_home_chan: pre.pre_home_chan,
        post_home_chan: post.post_home_chan,
        pre_current_chan: pre.pre_current_chan,
        post_current_chan: post.post_current_chan,
        pre_op_chan: pre.pre_op_chan,
        post_op_chan: post.post_op_chan,
        pre_word114: pre.pre_word114,
        post_word114: post.post_word114,
    }
}

fn record(snapshot: Snapshot) {
    let ordinal = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    let idx = ordinal % SLOT_COUNT;
    unsafe {
        SNAPSHOTS[idx] = snapshot;
    }
}

pub(super) fn reset_start_path_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        SNAPSHOTS = [Snapshot::ZERO; SLOT_COUNT];
    }
}

pub(super) fn log_start_path_wrap_diag(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed);
    println!(
        "upload_http: boot_scan_only_diag start_path_wrap_diag after={} count={}",
        stage, count
    );
    let limit = core::cmp::min(count, SLOT_COUNT);
    for idx in 0..limit {
        let slot = unsafe { SNAPSHOTS[idx] };
        if slot.fn_id == 0 {
            continue;
        }
        let name = match slot.fn_id {
            1 => "_do_wifi_start",
            2 => "wifi_hw_start",
            3 => "chm_init",
            _ => "unknown",
        };
        println!(
            "upload_http: boot_scan_only_diag start_path_wrap_diag_entry after={} idx={} fn={} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret=0x{:08x} pre_timer_setfn={} post_timer_setfn={} pre_timer_arm={} post_timer_arm={} pre_sta_ptr=0x{:08x} post_sta_ptr=0x{:08x} pre_chm_ptr=0x{:08x} post_chm_ptr=0x{:08x} pre_home_chan=0x{:02x} post_home_chan=0x{:02x} pre_current_chan=0x{:02x} post_current_chan=0x{:02x} pre_op_chan=0x{:02x} post_op_chan=0x{:02x} pre_word114=0x{:08x} post_word114=0x{:08x}",
            stage,
            idx,
            name,
            slot.args[0],
            slot.args[1],
            slot.args[2],
            slot.args[3],
            slot.ret,
            slot.pre_timer_setfn,
            slot.post_timer_setfn,
            slot.pre_timer_arm,
            slot.post_timer_arm,
            slot.pre_sta_ptr,
            slot.post_sta_ptr,
            slot.pre_chm_ptr,
            slot.post_chm_ptr,
            slot.pre_home_chan,
            slot.post_home_chan,
            slot.pre_current_chan,
            slot.post_current_chan,
            slot.pre_op_chan,
            slot.post_op_chan,
            slot.pre_word114,
            slot.post_word114,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap__do_wifi_start(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre = capture_snapshot(1, [a2, a3, a4, a5], 0, true);
    let ret = unsafe { __real__do_wifi_start(a2, a3, a4, a5, a6, a7) };
    let post = capture_snapshot(1, [a2, a3, a4, a5], ret, false);
    record(merge(pre, post));
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wifi_hw_start(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre = capture_snapshot(2, [a2, a3, a4, a5], 0, true);
    let ret = unsafe { __real_wifi_hw_start(a2, a3, a4, a5, a6, a7) };
    let post = capture_snapshot(2, [a2, a3, a4, a5], ret, false);
    record(merge(pre, post));
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_chm_init(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre = capture_snapshot(3, [a2, a3, a4, a5], 0, true);
    let ret = unsafe { __real_chm_init(a2, a3, a4, a5, a6, a7) };
    let post = capture_snapshot(3, [a2, a3, a4, a5], ret, false);
    record(merge(pre, post));
    ret
}
