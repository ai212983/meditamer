static STACK_HEADROOM_MIN_BYTES: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);
static TOUCH_CORE_STACK_GUARD: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static TOUCH_CORE_STACK_TOP: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static TOUCH_CORE_STACK_HEADROOM_MIN_BYTES: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);

#[cfg(target_arch = "xtensa")]
#[inline(always)]
fn current_sp() -> usize {
    esp_hal::xtensa_lx::get_stack_pointer() as usize
}

#[cfg(not(target_arch = "xtensa"))]
#[inline(always)]
fn current_sp() -> usize {
    0
}

pub(crate) fn log_stack_headroom(tag: &str) {
    if !diag_enabled(DIAG_DOMAIN_SD)
        && !diag_enabled(DIAG_DOMAIN_HTTP)
        && !diag_enabled(DIAG_DOMAIN_NET)
    {
        return;
    }

    #[cfg(target_arch = "xtensa")]
    {
        unsafe extern "C" {
            static _stack_end_cpu0: u32;
            static _stack_start_cpu0: u32;
            static __stack_chk_guard: u32;
        }

        let sp = current_sp();
        let stack_end = core::ptr::addr_of!(_stack_end_cpu0) as usize;
        let stack_start = core::ptr::addr_of!(_stack_start_cpu0) as usize;
        let guard = core::ptr::addr_of!(__stack_chk_guard) as usize;

        let total_bytes = stack_start.saturating_sub(stack_end);
        let used_bytes = stack_start.saturating_sub(sp);
        let headroom_bytes = sp.saturating_sub(guard);
        let headroom_u32 = headroom_bytes.min(u32::MAX as usize) as u32;

        let mut min_headroom =
            STACK_HEADROOM_MIN_BYTES.load(core::sync::atomic::Ordering::Relaxed);
        while headroom_u32 < min_headroom {
            match STACK_HEADROOM_MIN_BYTES.compare_exchange_weak(
                min_headroom,
                headroom_u32,
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(next) => min_headroom = next,
            }
        }

        if headroom_bytes <= 4 * 1024 {
            esp_println::println!(
                "stack_diag: tag={} sp=0x{:08x} guard=0x{:08x} headroom={} used={} total={}",
                tag,
                sp as u32,
                guard as u32,
                headroom_bytes,
                used_bytes,
                total_bytes
            );
        }
    }

    #[cfg(not(target_arch = "xtensa"))]
    let _ = tag;

    #[cfg(target_arch = "xtensa")]
    let _ = tag;
}

pub(crate) fn configure_touch_core_stack(guard: usize, top: usize) {
    TOUCH_CORE_STACK_GUARD.store(guard, core::sync::atomic::Ordering::Release);
    TOUCH_CORE_STACK_TOP.store(top, core::sync::atomic::Ordering::Release);
}

pub(crate) fn record_touch_core_stack_headroom() {
    #[cfg(target_arch = "xtensa")]
    {
        let guard = TOUCH_CORE_STACK_GUARD.load(core::sync::atomic::Ordering::Acquire);
        let top = TOUCH_CORE_STACK_TOP.load(core::sync::atomic::Ordering::Acquire);
        if guard == 0 || top == 0 {
            return;
        }

        let sp = current_sp();
        let headroom = sp.saturating_sub(guard).min(u32::MAX as usize) as u32;
        let mut minimum = TOUCH_CORE_STACK_HEADROOM_MIN_BYTES
            .load(core::sync::atomic::Ordering::Relaxed);
        while headroom < minimum {
            match TOUCH_CORE_STACK_HEADROOM_MIN_BYTES.compare_exchange_weak(
                minimum,
                headroom,
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(next) => minimum = next,
            }
        }

        if headroom <= 512 {
            esp_println::println!(
                "touch_core_stack_diag: sp=0x{:08x} guard=0x{:08x} headroom={} used={} total={}",
                sp as u32,
                guard as u32,
                headroom,
                top.saturating_sub(sp),
                top.saturating_sub(guard),
            );
        }
    }
}

pub(crate) fn minimum_stack_headroom_bytes() -> u32 {
    let value = STACK_HEADROOM_MIN_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    if value == u32::MAX { 0 } else { value }
}

pub(crate) fn minimum_touch_core_stack_headroom_bytes() -> u32 {
    let value =
        TOUCH_CORE_STACK_HEADROOM_MIN_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    if value == u32::MAX { 0 } else { value }
}
