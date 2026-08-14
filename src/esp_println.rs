//! Interrupt-enabled UART0 ownership around the upstream ROM writer.

use core::{
    fmt::{Arguments, Write},
    hint::spin_loop,
    sync::atomic::{AtomicU32, AtomicU8, Ordering},
};

const OWNER_NONE: u8 = 0;

static TX_OWNER: AtomicU8 = AtomicU8::new(OWNER_NONE);
static RESPONSE_PENDING_CORE: AtomicU8 = AtomicU8::new(OWNER_NONE);
static DROPPED_WRITES: AtomicU32 = AtomicU32::new(0);

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

struct ResponseReservation;

impl ResponseReservation {
    fn write_bytes(&mut self, bytes: &[u8]) {
        esp_println_upstream::Printer::write_bytes(bytes);
    }
}

impl Drop for ResponseReservation {
    fn drop(&mut self) {
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
        if pending != OWNER_NONE {
            spin_loop();
            continue;
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

fn drop_log_write() {
    DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
}

#[doc(hidden)]
pub(crate) fn try_print(args: Arguments<'_>, newline: bool) {
    let Some(mut reservation) = reserve_log() else {
        drop_log_write();
        return;
    };
    let _ = reservation.write_fmt(args);
    if newline {
        reservation.write_bytes(b"\n");
    }
}

/// Gives a correlated serial response priority over lossy diagnostics.
pub(crate) async fn write_response(bytes: &[u8]) {
    // SerialUart has one mutable owner, so only one response reservation can
    // exist. Busy-waiting leaves interrupts enabled and avoids scheduling a
    // same-core diagnostic between priority publication and ownership.
    let current = current_owner();
    RESPONSE_PENDING_CORE.store(current, Ordering::Release);
    let _pending = PendingResponse;
    let mut reservation = loop {
        if TX_OWNER
            .compare_exchange(OWNER_NONE, current, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            break ResponseReservation;
        }
        spin_loop();
    };
    reservation.write_bytes(bytes);
}

pub(crate) fn dropped_write_count() -> u32 {
    DROPPED_WRITES.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! esp_uart_println {
    () => {{
        $crate::uart_println::try_print(::core::format_args!(""), true);
    }};
    ($($arg:tt)*) => {{
        $crate::uart_println::try_print(::core::format_args!($($arg)*), true);
    }};
}

#[macro_export]
macro_rules! esp_uart_print {
    ($($arg:tt)*) => {{
        $crate::uart_println::try_print(::core::format_args!($($arg)*), false);
    }};
}
