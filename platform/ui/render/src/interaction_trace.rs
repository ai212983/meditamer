//! Optional synchronous observation hook; never retains widget pointers.
use core::cell::Cell;
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
static HOOK: Mutex<CriticalSectionRawMutex, Cell<Option<fn(u8, u32, u32)>>> =
    Mutex::new(Cell::new(None));
pub fn install(hook: fn(u8, u32, u32)) {
    HOOK.lock(|slot| slot.set(Some(hook)));
}
pub fn emit(stage: u8, target: u32, detail: u32) {
    let hook = HOOK.lock(|slot| slot.get());
    if let Some(hook) = hook {
        hook(stage, target, detail);
    }
}

static FAILURES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn hook_failed() {
    FAILURES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}
pub fn failures() -> u32 {
    FAILURES.load(core::sync::atomic::Ordering::Relaxed)
}
