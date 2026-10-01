//! Boot-only allocation rejection through the actual, unpolled SD runner.
use super::{allocate_fat_engine, allocate_runner, BufferAllocError, SdProbeDriver};
use core::sync::atomic::{AtomicU32, Ordering};

static RUNNER_POLLS: AtomicU32 = AtomicU32::new(0);

pub(super) fn record_runner_poll() {
    RUNNER_POLLS.fetch_add(1, Ordering::Relaxed);
}

fn external_free() -> usize {
    esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::External.into())
}

fn check_clean(probe: &SdProbeDriver, before: usize, stage: &str) {
    let after = external_free();
    assert_eq!(
        before, after,
        "SD allocation fixture leaked external storage"
    );
    assert_eq!(RUNNER_POLLS.load(Ordering::Relaxed), 0);
    assert!(!probe.is_initialized());
    console::println!(
        "SD_ALLOC_FIXTURE stage={} outcome=passed external_before={} external_after={} runner_polls=0 probe_initialized=false",
        stage, before, after
    );
}

#[inline(never)]
pub(super) fn verify(probe: &mut SdProbeDriver) {
    assert!(!probe.is_initialized());
    let before = external_free();
    crate::firmware::psram::reject_next_external_allocation();
    assert!(matches!(
        allocate_fat_engine(),
        Err(BufferAllocError::OutOfMemory)
    ));
    check_clean(probe, before, "fat_rejected");

    let before = external_free();
    let fat = allocate_fat_engine().expect("SD fixture FAT allocation");
    assert!(external_free() < before);
    crate::firmware::psram::reject_next_external_allocation();
    assert!(matches!(
        allocate_runner(probe, fat),
        Err(BufferAllocError::OutOfMemory)
    ));
    // The failed future owned the real external FAT allocation. Its drop must
    // free that allocation and end its probe borrow before this point.
    check_clean(probe, before, "runner_rejected");

    let before = external_free();
    let fat = allocate_fat_engine().expect("SD fixture recovery FAT allocation");
    let runner =
        allocate_runner(probe, fat).unwrap_or_else(|_| panic!("SD fixture runner allocation"));
    assert!(external_free() < before);
    drop(runner);
    check_clean(probe, before, "unpolled_runner_dropped");
    // The ordinary task now allocates and polls its runner. No reset/halt is
    // intercepted: only the fallible factories are exercised above.
}
