use core::sync::atomic::{AtomicU32, Ordering};

pub(super) fn saturating_add_u32(counter: &AtomicU32, value: u32) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

pub(super) fn update_max_u32(max_counter: &AtomicU32, value: u32) {
    let mut current = max_counter.load(Ordering::Relaxed);
    while value > current {
        match max_counter.compare_exchange_weak(
            current,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}
