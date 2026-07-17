use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;
const PTR_ARG_COUNT: usize = 3;
const FIELD_COUNT: usize = 10;
const OFFSETS: [usize; FIELD_COUNT] = [0x06, 0x24, 0x3c, 0x5d, 0x5e, 0x5f, 0x7c, 0x80, 0x88, 0x92];

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    ret: u32,
    pre: [[u8; FIELD_COUNT]; PTR_ARG_COUNT],
    post: [[u8; FIELD_COUNT]; PTR_ARG_COUNT],
}

impl Snapshot {
    const ZERO: Self = Self {
        fn_id: 0,
        args: [0; 4],
        ret: 0,
        pre: [[0; FIELD_COUNT]; PTR_ARG_COUNT],
        post: [[0; FIELD_COUNT]; PTR_ARG_COUNT],
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_scan_profile_check(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_parse_beacon(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

fn capture_fields(ptr: usize) -> [u8; FIELD_COUNT] {
    if !(0x3ff0_0000..0x4000_0000).contains(&ptr) {
        return [0; FIELD_COUNT];
    }
    let mut out = [0u8; FIELD_COUNT];
    let mut idx = 0usize;
    while idx < FIELD_COUNT {
        out[idx] = unsafe { ((ptr + OFFSETS[idx]) as *const u8).read_volatile() };
        idx += 1;
    }
    out
}

fn store_snapshot(
    fn_id: u8,
    args: [usize; 4],
    ret: usize,
    pre: [[u8; FIELD_COUNT]; PTR_ARG_COUNT],
    post: [[u8; FIELD_COUNT]; PTR_ARG_COUNT],
) {
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
            ret: ret as u32,
            pre,
            post,
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
    let ptrs = [a2, a3, a4];
    let pre = [
        capture_fields(ptrs[0]),
        capture_fields(ptrs[1]),
        capture_fields(ptrs[2]),
    ];
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    let post = [
        capture_fields(ptrs[0]),
        capture_fields(ptrs[1]),
        capture_fields(ptrs[2]),
    ];
    store_snapshot(fn_id, [a2, a3, a4, a5], ret, pre, post);
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_profile_check(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(1, __real_scan_profile_check, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_parse_beacon(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(2, __real_scan_parse_beacon, a2, a3, a4, a5, a6, a7) }
}

pub(super) fn reset_profile_wrap_diag() {
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
        1 => "scan_profile_check",
        2 => "scan_parse_beacon",
        _ => "unknown",
    }
}

pub(super) fn log_profile_wrap_diag(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "upload_http: boot_scan_only_diag profile_wrap_diag after={} count={}",
        stage, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "upload_http: boot_scan_only_diag profile_wrap_diag_entry after={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x}",
            stage, idx, fn_label(snap.fn_id), snap.ret, snap.args[0], snap.args[1], snap.args[2], snap.args[3]
        );
        for ptr_idx in 0..PTR_ARG_COUNT {
            let pre = snap.pre[ptr_idx];
            let post = snap.post[ptr_idx];
            println!(
                "upload_http: boot_scan_only_diag profile_wrap_diag_fields after={} idx={} fn={} ptr_idx={} pre={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                stage,
                idx,
                fn_label(snap.fn_id),
                ptr_idx,
                pre[0], pre[1], pre[2], pre[3], pre[4], pre[5], pre[6], pre[7], pre[8], pre[9],
                post[0], post[1], post[2], post[3], post[4], post[5], post[6], post[7], post[8], post[9],
            );
        }
    }
}
