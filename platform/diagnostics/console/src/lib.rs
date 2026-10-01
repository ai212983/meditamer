//! Interrupt-enabled UART0 ownership around the upstream ROM writer.
//!
//! Extracted from the `meditamer` crate by ADR-0015 step 1. It previously lived
//! at `src/esp_println.rs` and was reached through
//! `extern crate self as esp_println;`, so that `esp_println::println!` resolved
//! to this implementation rather than to the upstream crate. Exactly one crate
//! in a build can play that trick, which blocked every shared-crate extraction
//! the ADR depends on. Call sites now say `console::println!`.

#![no_std]

use core::{
    fmt::{Arguments, Write},
    hint::spin_loop,
    sync::atomic::{AtomicU8, Ordering},
};

mod counters;
#[cfg(feature = "deferred-logs")]
mod deferred;
#[cfg(feature = "deferred-logs")]
pub use deferred::{drain_logs, enable_deferred_logs};

pub use counters::{drop_counts, dropped_write_count, DropCounts};

const OWNER_NONE: u8 = 0;

static TX_OWNER: AtomicU8 = AtomicU8::new(OWNER_NONE);
static RESPONSE_PENDING_CORE: AtomicU8 = AtomicU8::new(OWNER_NONE);

struct TxReservation;

impl TxReservation {
    fn write_bytes(&mut self, bytes: &[u8]) {
        esp_println_upstream::Printer::write_bytes(bytes);
    }
}

impl Write for TxReservation {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        self.write_bytes(value.as_bytes());
        Ok(())
    }
}

impl Drop for TxReservation {
    fn drop(&mut self) {
        TX_OWNER.store(OWNER_NONE, Ordering::Release);
    }
}

struct ResponseReservation {
    complete: bool,
}

impl ResponseReservation {
    fn write_bytes(&mut self, bytes: &[u8]) {
        esp_println_upstream::Printer::write_bytes(bytes);
    }
}

impl Drop for ResponseReservation {
    fn drop(&mut self) {
        // Cancellation may leave a partial record, but cannot join it to the
        // next writer's record. Normal completion adds no bytes.
        if !self.complete {
            self.write_bytes(b"\r\n");
        }
        TX_OWNER.store(OWNER_NONE, Ordering::Release);
    }
}

struct PendingResponse;

impl Drop for PendingResponse {
    fn drop(&mut self) {
        RESPONSE_PENDING_CORE.store(OWNER_NONE, Ordering::Release);
    }
}

fn current_owner() -> u8 {
    esp_hal::system::Cpu::current() as u8 + 1
}

fn reserve_log() -> Option<TxReservation> {
    let current = current_owner();
    loop {
        let pending = RESPONSE_PENDING_CORE.load(Ordering::Acquire);
        let owner = TX_OWNER.load(Ordering::Acquire);
        // Only same-core ISR/reentrant logging can observe an owner from the
        // same core: task writers never await while holding the reservation.
        if pending == current || owner == current {
            return None;
        }
        if pending != OWNER_NONE || owner != OWNER_NONE {
            return None;
        }
        match TX_OWNER.compare_exchange(OWNER_NONE, current, Ordering::Acquire, Ordering::Relaxed) {
            Ok(_) => {
                let pending = RESPONSE_PENDING_CORE.load(Ordering::Acquire);
                if pending == OWNER_NONE {
                    return Some(TxReservation);
                }
                TX_OWNER.store(OWNER_NONE, Ordering::Release);
                if pending == current {
                    return None;
                }
            }
            Err(owner) if owner == current => return None,
            Err(_) => {}
        }
        spin_loop();
    }
}

#[doc(hidden)]
pub fn try_print(args: Arguments<'_>, newline: bool) {
    #[cfg(feature = "deferred-logs")]
    if deferred::enabled() && current_owner() == 2 {
        let _ = deferred::enqueue(args, newline);
        return;
    }
    let Some(mut reservation) = reserve_log() else {
        #[cfg(feature = "deferred-logs")]
        if deferred::enabled() {
            // A response can yield on CPU0 while holding UART ownership. Keep
            // short diagnostics in the same bounded queue used by CPU1;
            // enqueue records any capacity/format failure exactly once.
            let _ = deferred::enqueue(args, newline);
            return;
        }
        counters::record_contention();
        return;
    };
    let _ = reservation.write_fmt(args);
    if newline {
        reservation.write_bytes(b"\n");
    }
}

/// Gives a correlated serial response priority over lossy diagnostics.
pub async fn write_response(bytes: &[u8]) {
    // Runtime response and diagnostic drains serialize through the async
    // writer mutex. The atomic reservation also excludes synchronous boot logs;
    // those never await and release it before another same-core task can run.
    #[cfg(feature = "deferred-logs")]
    let _writer = deferred::WRITER.lock().await;
    let current = current_owner();
    RESPONSE_PENDING_CORE.store(current, Ordering::Release);
    let _pending = PendingResponse;
    let mut reservation = loop {
        if TX_OWNER
            .compare_exchange(OWNER_NONE, current, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            break ResponseReservation { complete: false };
        }
        spin_loop();
    };
    #[cfg(feature = "deferred-logs")]
    {
        // At 115200 baud a full 32-byte chunk takes under 3 ms. Preserve the
        // reservation across yields so other tasks run without record interleaving.
        let mut chunks = bytes.chunks(32).peekable();
        while let Some(chunk) = chunks.next() {
            reservation.write_bytes(chunk);
            if chunks.peek().is_some() {
                embassy_futures::yield_now().await;
            }
        }
    }
    #[cfg(not(feature = "deferred-logs"))]
    reservation.write_bytes(bytes);
    reservation.complete = true;
    #[cfg(feature = "deferred-logs")]
    {
        // Releasing the mutex wakes another writer but does not reserve it for
        // that waiter. Yield after the complete record so a bulk producer cannot
        // immediately reacquire it for every line of a metrics dump.
        drop(reservation);
        drop(_pending);
        drop(_writer);
        embassy_futures::yield_now().await;
    }
}

/// Nonblocking diagnostic line. With a runtime drain enabled, busy UART writes
/// use its bounded queue; capacity and format failures are counted. Without a
/// drain, contention drops the line; see [`dropped_write_count`].
#[macro_export]
macro_rules! println {
    () => {{
        $crate::try_print(::core::format_args!(""), true);
    }};
    ($($arg:tt)*) => {{
        $crate::try_print(::core::format_args!($($arg)*), true);
    }};
}
