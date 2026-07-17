use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 20;
const SCAN_START_ARG3_BYTES: usize = 16;

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    ret: u32,
    pre_obj_ptr: u32,
    pre_obj_word0: u32,
    pre_obj_byte8: u8,
    pre_obj_byte9: u8,
    pre_obj_word12: u32,
    pre_obj_word16: u32,
    pre_obj_word20: u32,
    pre_obj_word24: u32,
    pre_obj_byte28: u8,
    pre_obj_word32: u32,
    pre_obj_word36: u32,
    pre_obj_byte40: u8,
    pre_ptr16_word0: u32,
    pre_ptr16_word4: u32,
    pre_ptr16_word8: u32,
    pre_ptr16_word12: u32,
    pre_cmd_ptr: u32,
    pre_cmd_word0: u32,
    pre_cmd_word4: u32,
    pre_cmd_byte8: u8,
    pre_cmd_byte9: u8,
    pre_cmd_word12: u32,
    pre_cmd_word16: u32,
    pre_cmd_word20: u32,
    pre_cmd_word24: u32,
    pre_cmd_byte28: u8,
    pre_cmd_half32: u16,
    pre_cmd_word36: u32,
    pre_cmd_word40: u32,
    obj_ptr: u32,
    obj_word0: u32,
    obj_word4: u32,
    obj_byte8: u8,
    obj_byte9: u8,
    obj_word12: u32,
    obj_word16: u32,
    obj_word20: u32,
    obj_word24: u32,
    obj_byte28: u8,
    obj_word32: u32,
    obj_word36: u32,
    obj_byte40: u8,
    ptr4_word0: u32,
    ptr4_word4: u32,
    ptr4_word8: u32,
    ptr4_word12: u32,
    ptr16_word0: u32,
    ptr16_word4: u32,
    ptr16_word8: u32,
    ptr16_word12: u32,
    cmd_ptr: u32,
    cmd_word0: u32,
    cmd_word4: u32,
    cmd_byte8: u8,
    cmd_byte9: u8,
    cmd_word12: u32,
    cmd_word16: u32,
    cmd_word20: u32,
    cmd_word24: u32,
    cmd_byte28: u8,
    cmd_half32: u16,
    cmd_word36: u32,
    cmd_word40: u32,
    scan_start_pre_arg3: [u8; SCAN_START_ARG3_BYTES],
    scan_start_post_arg3: [u8; SCAN_START_ARG3_BYTES],
}

impl Snapshot {
    const ZERO: Self = Self {
        fn_id: 0,
        args: [0; 4],
        ret: 0,
        pre_obj_ptr: 0,
        pre_obj_word0: 0,
        pre_obj_byte8: 0,
        pre_obj_byte9: 0,
        pre_obj_word12: 0,
        pre_obj_word16: 0,
        pre_obj_word20: 0,
        pre_obj_word24: 0,
        pre_obj_byte28: 0,
        pre_obj_word32: 0,
        pre_obj_word36: 0,
        pre_obj_byte40: 0,
        pre_ptr16_word0: 0,
        pre_ptr16_word4: 0,
        pre_ptr16_word8: 0,
        pre_ptr16_word12: 0,
        pre_cmd_ptr: 0,
        pre_cmd_word0: 0,
        pre_cmd_word4: 0,
        pre_cmd_byte8: 0,
        pre_cmd_byte9: 0,
        pre_cmd_word12: 0,
        pre_cmd_word16: 0,
        pre_cmd_word20: 0,
        pre_cmd_word24: 0,
        pre_cmd_byte28: 0,
        pre_cmd_half32: 0,
        pre_cmd_word36: 0,
        pre_cmd_word40: 0,
        obj_ptr: 0,
        obj_word0: 0,
        obj_word4: 0,
        obj_byte8: 0,
        obj_byte9: 0,
        obj_word12: 0,
        obj_word16: 0,
        obj_word20: 0,
        obj_word24: 0,
        obj_byte28: 0,
        obj_word32: 0,
        obj_word36: 0,
        obj_byte40: 0,
        ptr4_word0: 0,
        ptr4_word4: 0,
        ptr4_word8: 0,
        ptr4_word12: 0,
        ptr16_word0: 0,
        ptr16_word4: 0,
        ptr16_word8: 0,
        ptr16_word12: 0,
        cmd_ptr: 0,
        cmd_word0: 0,
        cmd_word4: 0,
        cmd_byte8: 0,
        cmd_byte9: 0,
        cmd_word12: 0,
        cmd_word16: 0,
        cmd_word20: 0,
        cmd_word24: 0,
        cmd_byte28: 0,
        cmd_half32: 0,
        cmd_word36: 0,
        cmd_word40: 0,
        scan_start_pre_arg3: [0; SCAN_START_ARG3_BYTES],
        scan_start_post_arg3: [0; SCAN_START_ARG3_BYTES],
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_scan_start(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize)
    -> usize;
    fn __real_scan_start_handler(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_set_scan_id(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_get_scan_id(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_enter_oper_channel_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_wifi_scan_start_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_inter_channel_timeout_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_clear_bss_queue(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ieee80211_sta_scan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

unsafe fn read_u32(ptr: usize, offset: usize) -> u32 {
    ((ptr + offset) as *const u32).read_unaligned()
}

unsafe fn read_u8(ptr: usize, offset: usize) -> u8 {
    *((ptr + offset) as *const u8)
}

unsafe fn read_u16(ptr: usize, offset: usize) -> u16 {
    ((ptr + offset) as *const u16).read_unaligned()
}

fn can_read_data_ptr(ptr: u32) -> bool {
    ptr != 0 && ptr < 0x4000_0000
}

unsafe fn capture_bytes(ptr: usize) -> [u8; SCAN_START_ARG3_BYTES] {
    let mut out = [0u8; SCAN_START_ARG3_BYTES];
    if !(0x3ff0_0000..0x4000_0000).contains(&ptr) {
        return out;
    }
    let mut idx = 0usize;
    while idx < SCAN_START_ARG3_BYTES {
        out[idx] = ((ptr + idx) as *const u8).read_volatile();
        idx += 1;
    }
    out
}

unsafe fn fill_post_obj_fields(snap: &mut Snapshot, ptr: usize) {
    snap.obj_ptr = ptr as u32;
    snap.obj_word0 = read_u32(ptr, 0);
    snap.obj_word4 = read_u32(ptr, 4);
    snap.obj_byte8 = read_u8(ptr, 8);
    snap.obj_byte9 = read_u8(ptr, 9);
    snap.obj_word12 = read_u32(ptr, 12);
    snap.obj_word16 = read_u32(ptr, 16);
    snap.obj_word20 = read_u32(ptr, 20);
    snap.obj_word24 = read_u32(ptr, 24);
    snap.obj_byte28 = read_u8(ptr, 28);
    snap.obj_word32 = read_u32(ptr, 32);
    snap.obj_word36 = read_u32(ptr, 36);
    snap.obj_byte40 = read_u8(ptr, 40);
    if can_read_data_ptr(snap.obj_word4) {
        let ptr = snap.obj_word4 as usize;
        snap.ptr4_word0 = read_u32(ptr, 0);
        snap.ptr4_word4 = read_u32(ptr, 4);
        snap.ptr4_word8 = read_u32(ptr, 8);
        snap.ptr4_word12 = read_u32(ptr, 12);
    }
    if can_read_data_ptr(snap.obj_word16) {
        let ptr = snap.obj_word16 as usize;
        snap.ptr16_word0 = read_u32(ptr, 0);
        snap.ptr16_word4 = read_u32(ptr, 4);
        snap.ptr16_word8 = read_u32(ptr, 8);
        snap.ptr16_word12 = read_u32(ptr, 12);
    }
    let cmd = ptr + 20;
    snap.cmd_ptr = cmd as u32;
    snap.cmd_word0 = read_u32(cmd, 0);
    snap.cmd_word4 = read_u32(cmd, 4);
    snap.cmd_byte8 = read_u8(cmd, 8);
    snap.cmd_byte9 = read_u8(cmd, 9);
    snap.cmd_word12 = read_u32(cmd, 12);
    snap.cmd_word16 = read_u32(cmd, 16);
    snap.cmd_word20 = read_u32(cmd, 20);
    snap.cmd_word24 = read_u32(cmd, 24);
    snap.cmd_byte28 = read_u8(cmd, 28);
    snap.cmd_half32 = read_u16(cmd, 32);
    snap.cmd_word36 = read_u32(cmd, 36);
    snap.cmd_word40 = read_u32(cmd, 40);
}

unsafe fn fill_pre_obj_fields(snap: &mut Snapshot, ptr: usize) {
    snap.pre_obj_ptr = ptr as u32;
    snap.pre_obj_word0 = read_u32(ptr, 0);
    snap.pre_obj_byte8 = read_u8(ptr, 8);
    snap.pre_obj_byte9 = read_u8(ptr, 9);
    snap.pre_obj_word12 = read_u32(ptr, 12);
    snap.pre_obj_word16 = read_u32(ptr, 16);
    snap.pre_obj_word20 = read_u32(ptr, 20);
    snap.pre_obj_word24 = read_u32(ptr, 24);
    snap.pre_obj_byte28 = read_u8(ptr, 28);
    snap.pre_obj_word32 = read_u32(ptr, 32);
    snap.pre_obj_word36 = read_u32(ptr, 36);
    snap.pre_obj_byte40 = read_u8(ptr, 40);
    if can_read_data_ptr(snap.pre_obj_word16) {
        let ptr = snap.pre_obj_word16 as usize;
        snap.pre_ptr16_word0 = read_u32(ptr, 0);
        snap.pre_ptr16_word4 = read_u32(ptr, 4);
        snap.pre_ptr16_word8 = read_u32(ptr, 8);
        snap.pre_ptr16_word12 = read_u32(ptr, 12);
    }
    let cmd = ptr + 20;
    snap.pre_cmd_ptr = cmd as u32;
    snap.pre_cmd_word0 = read_u32(cmd, 0);
    snap.pre_cmd_word4 = read_u32(cmd, 4);
    snap.pre_cmd_byte8 = read_u8(cmd, 8);
    snap.pre_cmd_byte9 = read_u8(cmd, 9);
    snap.pre_cmd_word12 = read_u32(cmd, 12);
    snap.pre_cmd_word16 = read_u32(cmd, 16);
    snap.pre_cmd_word20 = read_u32(cmd, 20);
    snap.pre_cmd_word24 = read_u32(cmd, 24);
    snap.pre_cmd_byte28 = read_u8(cmd, 28);
    snap.pre_cmd_half32 = read_u16(cmd, 32);
    snap.pre_cmd_word36 = read_u32(cmd, 36);
    snap.pre_cmd_word40 = read_u32(cmd, 40);
}

fn store_snapshot(fn_id: u8, args: [usize; 4], ret: usize) {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if slot >= SLOT_COUNT {
        return;
    }
    let mut snap = Snapshot {
        fn_id,
        args: [args[0] as u32, args[1] as u32, args[2] as u32, args[3] as u32],
        ret: ret as u32,
        ..Snapshot::ZERO
    };
    if (fn_id == 5 || fn_id == 6 || fn_id == 9) && args[0] != 0 {
        unsafe {
            fill_post_obj_fields(&mut snap, args[0]);
        }
    }
    unsafe {
        SNAPSHOTS[slot] = snap;
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
    let mut pre = Snapshot::ZERO;
    if fn_id == 1 {
        pre.scan_start_pre_arg3 = unsafe { capture_bytes(a5) };
    }
    if (fn_id == 5 || fn_id == 6 || fn_id == 9) && a2 != 0 {
        unsafe {
            fill_pre_obj_fields(&mut pre, a2);
        }
    }
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    store_snapshot(fn_id, [a2, a3, a4, a5], ret);
    if fn_id == 1 || ((fn_id == 5 || fn_id == 6 || fn_id == 9) && a2 != 0) {
        let slot = NEXT_SLOT.load(Ordering::Relaxed).saturating_sub(1);
        if slot < SLOT_COUNT {
            unsafe {
                if fn_id == 1 {
                    SNAPSHOTS[slot].scan_start_pre_arg3 = pre.scan_start_pre_arg3;
                    SNAPSHOTS[slot].scan_start_post_arg3 = capture_bytes(a5);
                }
                SNAPSHOTS[slot].pre_obj_ptr = pre.pre_obj_ptr;
                SNAPSHOTS[slot].pre_obj_word0 = pre.pre_obj_word0;
                SNAPSHOTS[slot].pre_obj_byte8 = pre.pre_obj_byte8;
                SNAPSHOTS[slot].pre_obj_byte9 = pre.pre_obj_byte9;
                SNAPSHOTS[slot].pre_obj_word12 = pre.pre_obj_word12;
                SNAPSHOTS[slot].pre_obj_word16 = pre.pre_obj_word16;
                SNAPSHOTS[slot].pre_obj_word20 = pre.pre_obj_word20;
                SNAPSHOTS[slot].pre_obj_word24 = pre.pre_obj_word24;
                SNAPSHOTS[slot].pre_obj_byte28 = pre.pre_obj_byte28;
                SNAPSHOTS[slot].pre_obj_word32 = pre.pre_obj_word32;
                SNAPSHOTS[slot].pre_obj_word36 = pre.pre_obj_word36;
                SNAPSHOTS[slot].pre_obj_byte40 = pre.pre_obj_byte40;
                SNAPSHOTS[slot].pre_ptr16_word0 = pre.pre_ptr16_word0;
                SNAPSHOTS[slot].pre_ptr16_word4 = pre.pre_ptr16_word4;
                SNAPSHOTS[slot].pre_ptr16_word8 = pre.pre_ptr16_word8;
                SNAPSHOTS[slot].pre_ptr16_word12 = pre.pre_ptr16_word12;
            }
        }
    }
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_start(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(1, __real_scan_start, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_start_handler(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(2, __real_scan_start_handler, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_scan_id(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(3, __real_scan_set_scan_id, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_get_scan_id(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(4, __real_scan_get_scan_id, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_enter_oper_channel_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(5, __real_scan_enter_oper_channel_process, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wifi_scan_start_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(6, __real_wifi_scan_start_process, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_inter_channel_timeout_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(7, __real_scan_inter_channel_timeout_process, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_clear_bss_queue(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(8, __real_clear_bss_queue, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_sta_scan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(9, __real_ieee80211_sta_scan, a2, a3, a4, a5, a6, a7) }
}

pub(crate) fn reset_scan_process_wrap_diag() {
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
        1 => "scan_start",
        2 => "scan_start_handler",
        3 => "scan_set_scan_id",
        4 => "scan_get_scan_id",
        5 => "scan_enter_oper_channel_process",
        6 => "wifi_scan_start_process",
        7 => "scan_inter_channel_timeout_process",
        8 => "clear_bss_queue",
        9 => "ieee80211_sta_scan",
        _ => "unknown",
    }
}

pub(crate) fn print_scan_process_wrap_diag(label: &str, phase: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "legacy_nostd_wifi_control: scan_process_wrap_diag label={} phase={} count={}",
        label, phase, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "legacy_nostd_wifi_control: scan_process_wrap_diag_entry label={} phase={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x}",
            label,
            phase,
            idx,
            fn_label(snap.fn_id),
            snap.ret,
            snap.args[0],
            snap.args[1],
            snap.args[2],
            snap.args[3],
        );
        if snap.fn_id == 1 {
            println!(
                "legacy_nostd_wifi_control: scan_process_wrap_scan_start_arg3 label={} phase={} idx={} pre={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                label,
                phase,
                idx,
                snap.scan_start_pre_arg3[0], snap.scan_start_pre_arg3[1], snap.scan_start_pre_arg3[2], snap.scan_start_pre_arg3[3],
                snap.scan_start_pre_arg3[4], snap.scan_start_pre_arg3[5], snap.scan_start_pre_arg3[6], snap.scan_start_pre_arg3[7],
                snap.scan_start_pre_arg3[8], snap.scan_start_pre_arg3[9], snap.scan_start_pre_arg3[10], snap.scan_start_pre_arg3[11],
                snap.scan_start_pre_arg3[12], snap.scan_start_pre_arg3[13], snap.scan_start_pre_arg3[14], snap.scan_start_pre_arg3[15],
                snap.scan_start_post_arg3[0], snap.scan_start_post_arg3[1], snap.scan_start_post_arg3[2], snap.scan_start_post_arg3[3],
                snap.scan_start_post_arg3[4], snap.scan_start_post_arg3[5], snap.scan_start_post_arg3[6], snap.scan_start_post_arg3[7],
                snap.scan_start_post_arg3[8], snap.scan_start_post_arg3[9], snap.scan_start_post_arg3[10], snap.scan_start_post_arg3[11],
                snap.scan_start_post_arg3[12], snap.scan_start_post_arg3[13], snap.scan_start_post_arg3[14], snap.scan_start_post_arg3[15],
            );
        }
        if (snap.fn_id == 6 || snap.fn_id == 9) && snap.obj_ptr != 0 {
            println!(
                "legacy_nostd_wifi_control: scan_process_wrap_obj label={} phase={} idx={} fn={} pre_ptr=0x{:08x} pre_word0=0x{:08x} pre_byte8=0x{:02x} pre_byte9=0x{:02x} pre_word12=0x{:08x} pre_word16=0x{:08x} pre_word20=0x{:08x} pre_word24=0x{:08x} pre_byte28=0x{:02x} pre_word32=0x{:08x} pre_word36=0x{:08x} pre_byte40=0x{:02x} pre_ptr16=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] pre_cmd_ptr=0x{:08x} pre_cmd=[w0=0x{:08x},w4=0x{:08x},b8=0x{:02x},b9=0x{:02x},w12=0x{:08x},w16=0x{:08x},w20=0x{:08x},w24=0x{:08x},b28=0x{:02x},h32=0x{:04x},w36=0x{:08x},w40=0x{:08x}] ptr=0x{:08x} word0=0x{:08x} word4=0x{:08x} byte8=0x{:02x} byte9=0x{:02x} word12=0x{:08x} word16=0x{:08x} word20=0x{:08x} word24=0x{:08x} byte28=0x{:02x} word32=0x{:08x} word36=0x{:08x} byte40=0x{:02x} ptr4=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] ptr16=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] cmd_ptr=0x{:08x} cmd=[w0=0x{:08x},w4=0x{:08x},b8=0x{:02x},b9=0x{:02x},w12=0x{:08x},w16=0x{:08x},w20=0x{:08x},w24=0x{:08x},b28=0x{:02x},h32=0x{:04x},w36=0x{:08x},w40=0x{:08x}]",
                label,
                phase,
                idx,
                fn_label(snap.fn_id),
                snap.pre_obj_ptr,
                snap.pre_obj_word0,
                snap.pre_obj_byte8,
                snap.pre_obj_byte9,
                snap.pre_obj_word12,
                snap.pre_obj_word16,
                snap.pre_obj_word20,
                snap.pre_obj_word24,
                snap.pre_obj_byte28,
                snap.pre_obj_word32,
                snap.pre_obj_word36,
                snap.pre_obj_byte40,
                snap.pre_ptr16_word0,
                snap.pre_ptr16_word4,
                snap.pre_ptr16_word8,
                snap.pre_ptr16_word12,
                snap.pre_cmd_ptr,
                snap.pre_cmd_word0,
                snap.pre_cmd_word4,
                snap.pre_cmd_byte8,
                snap.pre_cmd_byte9,
                snap.pre_cmd_word12,
                snap.pre_cmd_word16,
                snap.pre_cmd_word20,
                snap.pre_cmd_word24,
                snap.pre_cmd_byte28,
                snap.pre_cmd_half32,
                snap.pre_cmd_word36,
                snap.pre_cmd_word40,
                snap.obj_ptr,
                snap.obj_word0,
                snap.obj_word4,
                snap.obj_byte8,
                snap.obj_byte9,
                snap.obj_word12,
                snap.obj_word16,
                snap.obj_word20,
                snap.obj_word24,
                snap.obj_byte28,
                snap.obj_word32,
                snap.obj_word36,
                snap.obj_byte40,
                snap.ptr4_word0,
                snap.ptr4_word4,
                snap.ptr4_word8,
                snap.ptr4_word12,
                snap.ptr16_word0,
                snap.ptr16_word4,
                snap.ptr16_word8,
                snap.ptr16_word12,
                snap.cmd_ptr,
                snap.cmd_word0,
                snap.cmd_word4,
                snap.cmd_byte8,
                snap.cmd_byte9,
                snap.cmd_word12,
                snap.cmd_word16,
                snap.cmd_word20,
                snap.cmd_word24,
                snap.cmd_byte28,
                snap.cmd_half32,
                snap.cmd_word36,
                snap.cmd_word40,
            );
        }
    }
}
