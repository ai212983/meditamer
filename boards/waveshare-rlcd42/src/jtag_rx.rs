//! A minimal, direct-register reader for the USB-Serial-JTAG RX FIFO --
//! enough to receive one `SET_EPOCH <digits>\n` line from a host script, and
//! nothing else.
//!
//! `esp-println`'s own writer talks to this hardware via raw MMIO, bypassing
//! `esp_hal`'s peripheral-ownership system entirely -- it never calls
//! `PeripheralClockControl::enable`, so as far as that refcount is concerned
//! nobody has claimed `USB_DEVICE` yet. Constructing `esp_hal`'s own
//! `UsbSerialJtag` driver would look like the first claim and run the reset
//! its own source comment warns against: "Do NOT reset the peripheral. Doing
//! so will result in a broken USB JTAG connection." Reading the same
//! registers `esp-println` already polls, the same way -- raw pointers, no
//! ownership, no reset -- sidesteps that reset path entirely rather than
//! risking it.
//!
//! Register layout is not guessed: it comes from the `esp32s3` PAC
//! (`usb_device::ep1` / `ep1_conf`, SVD-derived from Espressif's own
//! peripheral description). `EP1` at `0x6003_8000` is the exact register
//! `esp-println` writes through for TX -- on this hardware the same address
//! is bidirectional, and reading it pops a byte from the RX FIFO instead.
//! `EP1_CONF` at `0x6003_8004`, bit 2 (`SERIAL_OUT_EP_DATA_AVAIL` in the
//! PAC), is set exactly when a byte is waiting. Bit 1 of that same register
//! is what `esp-println` already reads for "TX FIFO full" -- a different
//! bit, so polling this one alongside it is non-destructive.

const EP1: *const u32 = 0x6003_8000 as *const u32;
const EP1_CONF: *const u32 = 0x6003_8004 as *const u32;
const SERIAL_OUT_EP_DATA_AVAIL: u32 = 1 << 2;

fn byte_ready() -> bool {
    unsafe { EP1_CONF.read_volatile() & SERIAL_OUT_EP_DATA_AVAIL != 0 }
}

fn read_byte() -> u8 {
    unsafe { (EP1.read_volatile() & 0xff) as u8 }
}

/// Poll for a `SET_EPOCH <digits>` line, up to `timeout`. Ticks on an
/// embassy timer between polls so the executor keeps making progress rather
/// than spinning the CPU -- this only ever runs once, in the rare case the
/// RTC needs provisioning, so a few milliseconds of poll latency is
/// irrelevant against the seconds of round-trip time it is replacing.
///
/// Returns `None` on timeout, on a line that doesn't parse, or if the line
/// exceeds the fixed buffer -- any of which falls back to the caller's own
/// compiled-in reference time rather than blocking forever on a host that
/// may not be attached at all (a battery deployment has no host to answer).
pub async fn read_epoch_line(timeout: embassy_time::Duration) -> Option<u32> {
    const PREFIX: &[u8] = b"SET_EPOCH ";
    const MAX_LINE: usize = 20;

    let mut line = [0u8; MAX_LINE];
    let mut at = 0usize;
    let deadline = embassy_time::Instant::now() + timeout;

    loop {
        if embassy_time::Instant::now() >= deadline {
            return None;
        }

        if !byte_ready() {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(5)).await;
            continue;
        }

        let byte = read_byte();
        if byte == b'\n' || byte == b'\r' {
            if at == 0 {
                // A lone CR or LF before any content -- likely the line
                // ending of whatever the host's terminal sent before the
                // real reply. Keep waiting rather than treat it as empty.
                continue;
            }
            break;
        }

        if at >= line.len() {
            return None;
        }
        line[at] = byte;
        at += 1;
    }

    let received = &line[..at];
    let digits = received.strip_prefix(PREFIX)?;
    core::str::from_utf8(digits).ok()?.parse().ok()
}
