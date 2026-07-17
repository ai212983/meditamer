use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;
const WORD_COUNT: usize = 4;

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    ret: u32,
    ret_words: [u32; WORD_COUNT],
}

impl Snapshot {
    const ZERO: Self = Self {
        fn_id: 0,
        args: [0; 4],
        ret: 0,
        ret_words: [0; WORD_COUNT],
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_cnx_bss_alloc(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_cnx_update_bss_more(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

fn capture_ret_words(ptr: usize) -> [u32; WORD_COUNT] {
    if !(0x3ff0_0000..0x4000_0000).contains(&ptr) {
        return [0; WORD_COUNT];
    }
    let mut out = [0u32; WORD_COUNT];
    let mut idx = 0usize;
    while idx < WORD_COUNT {
        out[idx] = unsafe { ((ptr + idx * 4) as *const u32).read_volatile() };
        idx += 1;
    }
    out
}

fn store_snapshot(fn_id: u8, args: [usize; 4], ret: usize, ret_words: [u32; WORD_COUNT]) {
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
            ret_words,
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
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    store_snapshot(fn_id, [a2, a3, a4, a5], ret, capture_ret_words(ret));
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_cnx_bss_alloc(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(1, __real_cnx_bss_alloc, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_cnx_update_bss_more(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(2, __real_cnx_update_bss_more, a2, a3, a4, a5, a6, a7) }
}

pub(crate) fn reset_bss_wrap_diag() {
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
        1 => "cnx_bss_alloc",
        2 => "cnx_update_bss_more",
        _ => "unknown",
    }
}

pub(crate) fn print_bss_wrap_diag(label: &str, phase: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "legacy_nostd_wifi_control: bss_wrap_diag label={} phase={} count={}",
        label, phase, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "legacy_nostd_wifi_control: bss_wrap_diag_entry label={} phase={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret_words={:08x}:{:08x}:{:08x}:{:08x}",
            label,
            phase,
            idx,
            fn_label(snap.fn_id),
            snap.ret,
            snap.args[0],
            snap.args[1],
            snap.args[2],
            snap.args[3],
            snap.ret_words[0],
            snap.ret_words[1],
            snap.ret_words[2],
            snap.ret_words[3],
        );
    }
}
