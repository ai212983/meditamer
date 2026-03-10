use portable_atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub(crate) static LEGACY_SETFN_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_EXEC_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_PROCESS_DUE_CALL_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_PROCESS_DUE_HIT_COUNT: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_LAST_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LEGACY_LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LEGACY_LAST_ARM_US: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_LAST_ARM_REPEAT: AtomicBool = AtomicBool::new(false);
pub(crate) static LEGACY_LAST_NOW_US: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_LAST_STARTED_US: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_LAST_TIMEOUT_US: AtomicU32 = AtomicU32::new(0);
pub(crate) static LEGACY_LAST_NEXT_DUE_US: AtomicU32 = AtomicU32::new(u32::MAX);

#[derive(Clone, Copy)]
pub(crate) struct LegacyTimerDiag {
    pub setfn_count: u32,
    pub arm_count: u32,
    pub exec_count: u32,
    pub process_due_call_count: u32,
    pub process_due_hit_count: u32,
    pub last_callback_ptr: usize,
    pub last_arg_ptr: usize,
    pub last_arm_us: u32,
    pub last_arm_repeat: bool,
    pub last_now_us: u32,
    pub last_started_us: u32,
    pub last_timeout_us: u32,
    pub last_next_due_us: u32,
}

pub(crate) fn reset_diag() {
    LEGACY_SETFN_COUNT.store(0, Ordering::Relaxed);
    LEGACY_ARM_COUNT.store(0, Ordering::Relaxed);
    LEGACY_EXEC_COUNT.store(0, Ordering::Relaxed);
    LEGACY_PROCESS_DUE_CALL_COUNT.store(0, Ordering::Relaxed);
    LEGACY_PROCESS_DUE_HIT_COUNT.store(0, Ordering::Relaxed);
    LEGACY_LAST_CALLBACK_PTR.store(0, Ordering::Relaxed);
    LEGACY_LAST_ARG_PTR.store(0, Ordering::Relaxed);
    LEGACY_LAST_ARM_US.store(0, Ordering::Relaxed);
    LEGACY_LAST_ARM_REPEAT.store(false, Ordering::Relaxed);
    LEGACY_LAST_NOW_US.store(0, Ordering::Relaxed);
    LEGACY_LAST_STARTED_US.store(0, Ordering::Relaxed);
    LEGACY_LAST_TIMEOUT_US.store(0, Ordering::Relaxed);
    LEGACY_LAST_NEXT_DUE_US.store(u32::MAX, Ordering::Relaxed);
}

pub(crate) fn diag() -> LegacyTimerDiag {
    LegacyTimerDiag {
        setfn_count: LEGACY_SETFN_COUNT.load(Ordering::Relaxed),
        arm_count: LEGACY_ARM_COUNT.load(Ordering::Relaxed),
        exec_count: LEGACY_EXEC_COUNT.load(Ordering::Relaxed),
        process_due_call_count: LEGACY_PROCESS_DUE_CALL_COUNT.load(Ordering::Relaxed),
        process_due_hit_count: LEGACY_PROCESS_DUE_HIT_COUNT.load(Ordering::Relaxed),
        last_callback_ptr: LEGACY_LAST_CALLBACK_PTR.load(Ordering::Relaxed),
        last_arg_ptr: LEGACY_LAST_ARG_PTR.load(Ordering::Relaxed),
        last_arm_us: LEGACY_LAST_ARM_US.load(Ordering::Relaxed),
        last_arm_repeat: LEGACY_LAST_ARM_REPEAT.load(Ordering::Relaxed),
        last_now_us: LEGACY_LAST_NOW_US.load(Ordering::Relaxed),
        last_started_us: LEGACY_LAST_STARTED_US.load(Ordering::Relaxed),
        last_timeout_us: LEGACY_LAST_TIMEOUT_US.load(Ordering::Relaxed),
        last_next_due_us: LEGACY_LAST_NEXT_DUE_US.load(Ordering::Relaxed),
    }
}
