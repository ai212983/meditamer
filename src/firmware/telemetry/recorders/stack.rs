static STACK_HEADROOM_MIN_BYTES: core::sync::atomic::AtomicU32 =
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

        let new_low = headroom_u32 < min_headroom;
        if new_low || headroom_bytes <= 4 * 1024 {
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
}
