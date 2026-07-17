use portable_atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub(crate) const LEGACY_TIMER_RING_CAP: usize = 6;

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
pub(crate) static LEGACY_RECENT_SETFN_ORDINALS: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_SETFN_ETS_TIMER_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_SETFN_CALLBACK_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_SETFN_ARG_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_SETFN_CALLER_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_ORDINALS: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_CALLBACK_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_ARG_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_OP_CHANS: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_SCAN_WORD00: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_EXEC_SCAN_WORD114: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_ORDINALS: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_FOUND: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_EXECUTED: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_CALLBACK_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_ARG_PTRS: [AtomicUsize; LEGACY_TIMER_RING_CAP] =
    [const { AtomicUsize::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_OP_CHANS: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_SCAN_WORD00: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];
pub(crate) static LEGACY_RECENT_DUE_SCAN_WORD114: [AtomicU32; LEGACY_TIMER_RING_CAP] =
    [const { AtomicU32::new(0) }; LEGACY_TIMER_RING_CAP];

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
    pub recent_setfn_ordinals: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_setfn_ets_timer_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_setfn_callback_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_setfn_arg_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_setfn_caller_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_exec_ordinals: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_exec_callback_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_exec_arg_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_exec_op_chans: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_exec_scan_word00: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_exec_scan_word114: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_ordinals: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_found: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_executed: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_callback_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_due_arg_ptrs: [usize; LEGACY_TIMER_RING_CAP],
    pub recent_due_op_chans: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_scan_word00: [u32; LEGACY_TIMER_RING_CAP],
    pub recent_due_scan_word114: [u32; LEGACY_TIMER_RING_CAP],
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
    for idx in 0..LEGACY_TIMER_RING_CAP {
        LEGACY_RECENT_SETFN_ORDINALS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_SETFN_ETS_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_SETFN_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_SETFN_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_SETFN_CALLER_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_ORDINALS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_OP_CHANS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_SCAN_WORD00[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_SCAN_WORD114[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_ORDINALS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_FOUND[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_EXECUTED[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_OP_CHANS[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_SCAN_WORD00[idx].store(0, Ordering::Relaxed);
        LEGACY_RECENT_DUE_SCAN_WORD114[idx].store(0, Ordering::Relaxed);
    }
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
        recent_setfn_ordinals: core::array::from_fn(|idx| {
            LEGACY_RECENT_SETFN_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_ets_timer_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_SETFN_ETS_TIMER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_callback_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_SETFN_CALLBACK_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_arg_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_SETFN_ARG_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_caller_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_SETFN_CALLER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_exec_ordinals: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_exec_callback_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_CALLBACK_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_exec_arg_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_ARG_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_exec_op_chans: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_OP_CHANS[idx].load(Ordering::Relaxed)
        }),
        recent_exec_scan_word00: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_SCAN_WORD00[idx].load(Ordering::Relaxed)
        }),
        recent_exec_scan_word114: core::array::from_fn(|idx| {
            LEGACY_RECENT_EXEC_SCAN_WORD114[idx].load(Ordering::Relaxed)
        }),
        recent_due_ordinals: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_due_found: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_FOUND[idx].load(Ordering::Relaxed)
        }),
        recent_due_executed: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_EXECUTED[idx].load(Ordering::Relaxed)
        }),
        recent_due_callback_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_CALLBACK_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_due_arg_ptrs: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_ARG_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_due_op_chans: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_OP_CHANS[idx].load(Ordering::Relaxed)
        }),
        recent_due_scan_word00: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_SCAN_WORD00[idx].load(Ordering::Relaxed)
        }),
        recent_due_scan_word114: core::array::from_fn(|idx| {
            LEGACY_RECENT_DUE_SCAN_WORD114[idx].load(Ordering::Relaxed)
        }),
    }
}
