#![no_std]

#[path = "../../../../platform/diagnostics/console/src/counters.rs"]
pub mod counters;

#[path = "../../../../platform/diagnostics/console/src/deferred.rs"]
pub mod deferred;

pub use counters::{drop_counts, dropped_write_count, DropCounts};

// Host stand-in for the UART writer. Production queue, formatting, and
// counters run unmodified; only the transmitted bytes are dropped.
pub async fn write_response(bytes: &[u8]) {
    let _guard = deferred::WRITER.lock().await;
    let _ = bytes;
}
