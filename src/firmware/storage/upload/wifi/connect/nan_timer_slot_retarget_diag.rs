use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use esp_println::println;
use esp_wifi_sys::include::ets_timer;

fn nan_timer_slot_retarget_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_NAN_TIMER_SLOT_RETARGET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn nan_timer_slot_retarget_arg_filter() -> Option<usize> {
    match option_env!("MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_ARG_FILTER_DIAG")
        .or(option_env!("WIFI_NAN_TIMER_SLOT_RETARGET_ARG_FILTER_DIAG"))
    {
        Some("0") => Some(0),
        Some("1") => Some(1),
        _ => None,
    }
}

const IEEE_TIMER_PROCESS_OFFSET: usize = 0xa0;

static INVOKE_COUNT: AtomicU32 = AtomicU32::new(0);
static MATCHED_COUNT: AtomicU32 = AtomicU32::new(0);
static RETARGETED_COUNT: AtomicU32 = AtomicU32::new(0);
static DUPLICATE_TIMER_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_FROM_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_TO_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_ETS_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static SLOT0_ETS_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static SLOT0_TIMER_HANDLE_PTR: AtomicUsize = AtomicUsize::new(0);
static SLOT1_ETS_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static SLOT1_TIMER_HANDLE_PTR: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn ieee80211_timer_process(arg: *mut esp_wifi_sys::c_types::c_void);
    #[link_name = "ieee80211_timer_process"]
    fn ieee80211_timer_process_scan_step(
        kind: u32,
        reason: u32,
        arg: *mut esp_wifi_sys::c_types::c_void,
    );
}

fn nan_timer_slot_retarget_trampoline_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

unsafe extern "C" fn ieee80211_timer_process_scan_step_trampoline(
    arg: *mut esp_wifi_sys::c_types::c_void,
) {
    unsafe {
        ieee80211_timer_process_scan_step(7, 8, arg);
    }
}

fn ieee80211_timer_process_callback_ptr() -> usize {
    if nan_timer_slot_retarget_trampoline_enabled() {
        ieee80211_timer_process_scan_step_trampoline as usize
    } else {
        ieee80211_timer_process as usize + IEEE_TIMER_PROCESS_OFFSET
    }
}

fn infer_source_callback_ptr() -> usize {
    let diag = esp_radio::diagnostic_timer_compat_diag();
    let mut idx = 0usize;
    while idx < diag.recent_setfn_ordinals.len() {
        let ordinal = diag.recent_setfn_ordinals[idx];
        let callback_ptr = diag.recent_setfn_callback_ptrs[idx];
        let arg_ptr = diag.recent_setfn_arg_ptrs[idx];
        if ordinal == 0 || callback_ptr == 0 || !(arg_ptr == 0 || arg_ptr == 1) {
            idx += 1;
            continue;
        }

        let mut saw_arg0 = false;
        let mut saw_arg1 = false;
        let mut other = 0usize;
        while other < diag.recent_setfn_ordinals.len() {
            if diag.recent_setfn_callback_ptrs[other] == callback_ptr {
                let other_arg = diag.recent_setfn_arg_ptrs[other];
                if other_arg == 0 {
                    saw_arg0 = true;
                } else if other_arg == 1 {
                    saw_arg1 = true;
                }
            }
            other += 1;
        }
        if saw_arg0 && saw_arg1 {
            return callback_ptr;
        }
        idx += 1;
    }
    0
}

fn refresh_inferred_timer_slots() {
    let diag = esp_radio::diagnostic_timer_compat_diag();
    let source_callback_ptr = infer_source_callback_ptr();
    let mut slot0_ets = 0usize;
    let mut slot0_handle = 0usize;
    let mut slot1_ets = 0usize;
    let mut slot1_handle = 0usize;
    let mut idx = 0usize;
    while idx < diag.recent_setfn_ordinals.len() {
        if diag.recent_setfn_ordinals[idx] != 0
            && diag.recent_setfn_callback_ptrs[idx] == source_callback_ptr
        {
            match diag.recent_setfn_arg_ptrs[idx] {
                0 if slot0_ets == 0 => {
                    slot0_ets = diag.recent_setfn_ets_timer_ptrs[idx];
                    slot0_handle = diag.recent_setfn_timer_handle_ptrs[idx];
                }
                1 if slot1_ets == 0 => {
                    slot1_ets = diag.recent_setfn_ets_timer_ptrs[idx];
                    slot1_handle = diag.recent_setfn_timer_handle_ptrs[idx];
                }
                _ => {}
            }
        }
        idx += 1;
    }
    SLOT0_ETS_TIMER_PTR.store(slot0_ets, Ordering::Relaxed);
    SLOT0_TIMER_HANDLE_PTR.store(slot0_handle, Ordering::Relaxed);
    SLOT1_ETS_TIMER_PTR.store(slot1_ets, Ordering::Relaxed);
    SLOT1_TIMER_HANDLE_PTR.store(slot1_handle, Ordering::Relaxed);
}

pub(super) fn reset_nan_timer_slot_retarget_diag() {
    INVOKE_COUNT.store(0, Ordering::Relaxed);
    MATCHED_COUNT.store(0, Ordering::Relaxed);
    RETARGETED_COUNT.store(0, Ordering::Relaxed);
    DUPLICATE_TIMER_COUNT.store(0, Ordering::Relaxed);
    LAST_FROM_CALLBACK_PTR.store(0, Ordering::Relaxed);
    LAST_TO_CALLBACK_PTR.store(0, Ordering::Relaxed);
    LAST_ETS_TIMER_PTR.store(0, Ordering::Relaxed);
    LAST_ARG_PTR.store(0, Ordering::Relaxed);
    SLOT0_ETS_TIMER_PTR.store(0, Ordering::Relaxed);
    SLOT0_TIMER_HANDLE_PTR.store(0, Ordering::Relaxed);
    SLOT1_ETS_TIMER_PTR.store(0, Ordering::Relaxed);
    SLOT1_TIMER_HANDLE_PTR.store(0, Ordering::Relaxed);
}

pub(super) fn maybe_apply_nan_timer_slot_retarget_diag() {
    refresh_inferred_timer_slots();
    if !nan_timer_slot_retarget_enabled() {
        return;
    }
    INVOKE_COUNT.fetch_add(1, Ordering::Relaxed);
    let from_callback_ptr = infer_source_callback_ptr();
    if from_callback_ptr == 0 {
        LAST_TO_CALLBACK_PTR.store(ieee80211_timer_process_callback_ptr(), Ordering::Relaxed);
        return;
    }
    let to_callback_ptr = ieee80211_timer_process_callback_ptr();
    let diag = unsafe {
        esp_radio::diagnostic_retarget_timer_callbacks_with_arg_filter(
            from_callback_ptr,
            to_callback_ptr,
            nan_timer_slot_retarget_arg_filter(),
        )
    };
    MATCHED_COUNT.store(diag.matched_count, Ordering::Relaxed);
    RETARGETED_COUNT.store(diag.retargeted_count, Ordering::Relaxed);
    DUPLICATE_TIMER_COUNT.store(diag.duplicate_timer_count, Ordering::Relaxed);
    LAST_FROM_CALLBACK_PTR.store(diag.last_from_callback_ptr, Ordering::Relaxed);
    LAST_TO_CALLBACK_PTR.store(diag.last_to_callback_ptr, Ordering::Relaxed);
    LAST_ETS_TIMER_PTR.store(diag.last_ets_timer_ptr, Ordering::Relaxed);
    LAST_ARG_PTR.store(diag.last_arg_ptr, Ordering::Relaxed);
    refresh_inferred_timer_slots();
}

fn log_timer_slot(stage: &str, slot: usize, ets_timer_ptr: usize, timer_handle_ptr: usize) {
    if timer_handle_ptr == 0 {
        println!(
            "upload_http: boot_scan_only_diag nan_timer_slot_live after={} slot={} ets_timer_ptr=0x{:x} timer_handle_ptr=0x0 callback_ptr=0x0 arg_ptr=0x0 active=0 started_us=0 next_due_us=0 period_us=0 periodic=0",
            stage, slot, ets_timer_ptr
        );
        return;
    }
    let ets_timer = unsafe { &*(ets_timer_ptr as *const ets_timer) };
    let live_timer_handle_ptr = ets_timer.priv_ as usize;
    let live = esp_radio::diagnostic_timer_live_diag(live_timer_handle_ptr);
    println!(
        "upload_http: boot_scan_only_diag nan_timer_slot_live after={} slot={} ets_timer_ptr=0x{:x} timer_handle_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x} active={} started_us={} next_due_us={} period_us={} periodic={}",
        stage,
        slot,
        ets_timer_ptr,
        live_timer_handle_ptr,
        live.callback_ptr,
        live.callback_arg_ptr,
        live.is_active as u32,
        live.started_us,
        live.next_due_us,
        live.period_us,
        live.periodic as u32,
    );
}

pub(super) fn log_nan_timer_slot_retarget_diag(stage: &str) {
    println!(
        "upload_http: boot_scan_only_diag nan_timer_slot_retarget_diag after={} enabled={} arg_filter={} invoke_count={} matched_count={} retargeted_count={} duplicate_timer_count={} from_callback_ptr=0x{:x} to_callback_ptr=0x{:x} last_ets_timer_ptr=0x{:x} last_arg_ptr=0x{:x}",
        stage,
        nan_timer_slot_retarget_enabled(),
        match nan_timer_slot_retarget_arg_filter() {
            Some(0) => 0i32,
            Some(1) => 1i32,
            _ => -1i32,
        },
        INVOKE_COUNT.load(Ordering::Relaxed),
        MATCHED_COUNT.load(Ordering::Relaxed),
        RETARGETED_COUNT.load(Ordering::Relaxed),
        DUPLICATE_TIMER_COUNT.load(Ordering::Relaxed),
        LAST_FROM_CALLBACK_PTR.load(Ordering::Relaxed),
        LAST_TO_CALLBACK_PTR.load(Ordering::Relaxed),
        LAST_ETS_TIMER_PTR.load(Ordering::Relaxed),
        LAST_ARG_PTR.load(Ordering::Relaxed),
    );
    log_timer_slot(
        stage,
        0,
        SLOT0_ETS_TIMER_PTR.load(Ordering::Relaxed),
        SLOT0_TIMER_HANDLE_PTR.load(Ordering::Relaxed),
    );
    log_timer_slot(
        stage,
        1,
        SLOT1_ETS_TIMER_PTR.load(Ordering::Relaxed),
        SLOT1_TIMER_HANDLE_PTR.load(Ordering::Relaxed),
    );

    let arm_diag = esp_radio::diagnostic_timer_arm_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_live_arm_diag after={} count={}",
        stage, arm_diag.count
    );
    let mut idx = 0usize;
    while idx < arm_diag.recent_ordinals.len() {
        let ordinal = arm_diag.recent_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_live_arm_recent after={} idx={} ordinal={} timer_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x} caller_ptr=0x{:x} timeout_us={} periodic={}",
                stage,
                idx,
                ordinal,
                arm_diag.recent_timer_ptrs[idx],
                arm_diag.recent_callback_ptrs[idx],
                arm_diag.recent_arg_ptrs[idx],
                arm_diag.recent_caller_ptrs[idx],
                arm_diag.recent_timeout_us[idx],
                arm_diag.recent_periodic[idx] as u32,
            );
        }
        idx += 1;
    }
}
