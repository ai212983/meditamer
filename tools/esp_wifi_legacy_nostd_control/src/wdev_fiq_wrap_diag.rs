use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;

#[derive(Copy, Clone)]
struct Snapshot {
    arg0: u32,
    ret: u32,
    pre_mac_isr: u32,
    post_mac_isr: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        arg0: 0,
        ret: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_wDev_ProcessFiq(arg: usize) -> usize;
}

pub(crate) fn reset_wdev_fiq_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        core::ptr::write(
            SNAPSHOTS.as_ptr() as *mut [Snapshot; SLOT_COUNT],
            [Snapshot::ZERO; SLOT_COUNT],
        );
    }
}

pub(crate) fn print_wdev_fiq_wrap_diag(label: &str, phase: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "legacy_nostd_wifi_control: wdev_fiq_wrap_diag label={} phase={} count={}",
        label, phase, count
    );
    for idx in 0..count {
        let snap = unsafe { core::ptr::read(SNAPSHOTS.as_ptr().add(idx)) };
        println!(
            "legacy_nostd_wifi_control: wdev_fiq_wrap_diag_entry label={} phase={} idx={} arg0=0x{:08x} ret=0x{:08x} pre_mac_isr={} post_mac_isr={}",
            label, phase, idx, snap.arg0, snap.ret, snap.pre_mac_isr, snap.post_mac_isr
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wDev_ProcessFiq(arg: usize) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let ret = unsafe { __real_wDev_ProcessFiq(arg) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let ordinal = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if ordinal < SLOT_COUNT {
        unsafe {
            core::ptr::write(
                SNAPSHOTS.as_ptr().add(ordinal) as *mut Snapshot,
                Snapshot {
                    arg0: arg as u32,
                    ret: ret as u32,
                    pre_mac_isr,
                    post_mac_isr,
                },
            );
        }
    }
    ret
}
