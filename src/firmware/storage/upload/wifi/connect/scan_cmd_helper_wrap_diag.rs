use core::sync::atomic::{AtomicUsize, Ordering};

#[path = "scan_cmd_helper_wrap_diag/capture.rs"]
mod capture;
#[path = "scan_cmd_helper_wrap_diag/logging.rs"]
mod logging;
#[path = "scan_cmd_helper_wrap_diag/wrappers.rs"]
mod wrappers;

const SLOT_COUNT: usize = 24;
const PTR_BYTES: usize = 8;
const APP_SCAN_PARAMS_BYTES: usize = 16;

#[derive(Copy, Clone)]
struct Snapshot {
    fn_id: u8,
    args: [u32; 4],
    call_arg2: u32,
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
        call_arg2: 0,
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

pub(super) fn reset_scan_cmd_helper_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        let mut idx = 0usize;
        while idx < SLOT_COUNT {
            SNAPSHOTS[idx] = Snapshot::ZERO;
            idx += 1;
        }
    }
}

pub(super) fn log_scan_cmd_helper_wrap_diag(stage: &str) {
    logging::log(stage);
}
