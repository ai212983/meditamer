//! Bounded CPU1 diagnostic records; producers never wait for the UART or its owner.
use core::{
    fmt::{Arguments, Write},
    sync::atomic::{AtomicBool, Ordering},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};

static ENABLED: AtomicBool = AtomicBool::new(false);
// Four startup/control records, up to 256 bytes including newline. Overflow is
// lossy and counted by the console; there is no heap allocation or capacity growth.
pub static RECORDS: Channel<CriticalSectionRawMutex, heapless::String<256>, 4> = Channel::new();
pub(super) static WRITER: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredDrop {
    Oversize,
    Overflow,
}

/// Defer CPU1 logs only after arranging a CPU0 drain task. Boot/probe users opt in.
pub fn enable_deferred_logs() {
    ENABLED.store(true, Ordering::Release);
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

pub fn enqueue(args: Arguments<'_>, newline: bool) -> Result<(), DeferredDrop> {
    let mut record = heapless::String::new();
    if record.write_fmt(args).is_err() || (newline && record.push('\n').is_err()) {
        crate::counters::record_deferred_oversize();
        return Err(DeferredDrop::Oversize);
    }
    if RECORDS.try_send(record).is_err() {
        crate::counters::record_deferred_overflow();
        return Err(DeferredDrop::Overflow);
    }
    Ok(())
}

/// Sole background drain. Responses and diagnostics share the same TX mutex;
/// a record stays contiguous even while its bounded transmission yields.
pub async fn drain_logs() -> ! {
    loop {
        let record = RECORDS.receive().await;
        crate::write_response(record.as_bytes()).await;
        embassy_futures::yield_now().await;
    }
}
