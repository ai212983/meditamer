//! Guru Meditation crash screen: draws a short, readable message on the
//! panel from inside the panic handler itself, no reboot.
//!
//! This is Tier A only (see the assistant-session discussion this shipped
//! from -- no persisted design doc yet): an ordinary task-context `panic!()`
//! gets a screen instead of silently going dark; the harder case (a fault
//! originating inside interrupt dispatch, the one that actually took
//! Medinote down once -- `docs/archive/architecture/
//! 0014-single-production-sd-recovery-updater.md`) is not attempted here.
//! Reset semantics are unchanged: after drawing, esp-backtrace's normal
//! `abort()` still parks in an interrupts-disabled `loop {}`, same as
//! without this module. `src/bin/panic_screen_probe.rs` hardware-proved the
//! underlying approach (peripheral-steal + fresh driver + blocking SPI,
//! no LVGL/heap/embassy) on 2026-09-03.
//!
//! Hooked through `esp-backtrace`'s `custom-pre-backtrace` extension point,
//! which `vendor/esp-backtrace-0.20.0-custom-pre-backtrace-info` is patched (see that
//! crate's `MEDITAMER_PATCH.md`, "Second delta") to call with `&PanicInfo`
//! -- upstream's hook takes no arguments, which is not enough to read the
//! panic message or location.
//!
//! Gated behind the `crash-screen` Cargo feature (see `Cargo.toml`'s
//! comment on that feature for why it isn't in `default`).

mod font;
mod render;

use core::fmt::Write as _;

use esp_hal::peripherals::Peripherals;

use waveshare_rlcd42::panel;

use font::FONT_LINE_HEIGHT;
use render::{build_panel, draw_text, draw_text_wrapped, MARGIN, SCALE};

// ---------------------------------------------------------------------
// Message capture: no heap, no `alloc`, bounded, never fails outright even
// if the panic message is longer than fits -- it just stops copying, which
// `core::fmt::Write` machinery treats as "keep going" (`Ok`), not an error
// that would drop everything already gathered. `heapless::String`'s own
// `Write` impl rejects an over-long `write_str` chunk wholesale (see
// `heapless::string::StringInner::push_str`), which risks losing an entire
// single-chunk `Display` message rather than truncating it -- this small
// buffer avoids that by design.
// ---------------------------------------------------------------------

struct FixedBuf<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> FixedBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    fn as_str(&self) -> &str {
        // SAFETY / correctness: `write_str` below only ever copies a prefix
        // that lands on a `char` boundary, so `buf[..len]` is always valid
        // UTF-8; `unwrap_or` is just defence in depth, not expected to fire.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for FixedBuf<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let remaining = self.buf.len() - self.len;
        let mut take = remaining.min(s.len());
        while take > 0 && !s.is_char_boundary(take) {
            take -= 1;
        }
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(()) // always Ok: truncating silently is the point, not an error.
    }
}

/// esp-backtrace's `custom-pre-backtrace` hook (patched to carry
/// `&PanicInfo` -- see the module doc above). Runs once, before the normal
/// panic banner prints and before the final `loop {}`.
#[no_mangle]
pub extern "Rust" fn custom_pre_backtrace(info: &core::panic::PanicInfo) {
    console::println!("CRASH_SCREEN stage=steal");

    // SAFETY: re-acquiring handles the running app already owns. Whatever
    // task/interrupt panicked is not running concurrently with this handler
    // on this core, so there is no live transaction on SPI2/these GPIOs to
    // collide with -- see `panic_screen_probe.rs`'s module doc for the
    // fuller reasoning this shares.
    let peripherals = unsafe { Peripherals::steal() };

    static mut CRASH_FRAMEBUFFER: [u8; panel::FRAMEBUFFER_BYTES] = [0; panel::FRAMEBUFFER_BYTES];
    // SAFETY: written and read only here, once, on the single core that
    // reaches this handler.
    let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(CRASH_FRAMEBUFFER) };

    let mut display = build_panel(
        peripherals.SPI2,
        peripherals.GPIO11,
        peripherals.GPIO12,
        peripherals.GPIO5,
        peripherals.GPIO40,
        peripherals.GPIO41,
        framebuffer,
    );
    display.init();
    display.set_frame_rates(panel::HighPowerRate::Full, panel::LowPowerRate::Hz0_25);
    console::println!("CRASH_SCREEN stage=panel_ready");

    let mut message_bytes = [0u8; 80];
    let mut message = FixedBuf::new(&mut message_bytes);
    let _ = write!(message, "{}", info.message());

    let mut location_bytes = [0u8; 64];
    let mut location = FixedBuf::new(&mut location_bytes);
    match info.location() {
        Some(loc) => {
            let _ = write!(location, "at {}:{}", loc.file(), loc.line());
        }
        None => {
            let _ = write!(location, "location unknown");
        }
    }

    let line_height = FONT_LINE_HEIGHT * SCALE;
    let heading_y = MARGIN + 4;
    let rule_y = heading_y + line_height;
    let message_y = rule_y + 6 + line_height;
    // Leave room for at least one location line below whatever the message
    // wraps to -- if the message itself is long enough to eat that room,
    // `draw_text_wrapped` stops rather than colliding with it.
    let message_max_y = panel::HEIGHT as i32 - MARGIN - line_height;

    draw_text(&mut display, MARGIN, heading_y, "GURU MEDITATION");
    for x in MARGIN..(panel::WIDTH as i32 - MARGIN) {
        display.set_pixel(x as usize, (rule_y - line_height / 2) as usize, true);
    }
    let location_y = draw_text_wrapped(
        &mut display,
        MARGIN,
        message_y,
        line_height,
        message_max_y,
        message.as_str(),
    );
    draw_text_wrapped(
        &mut display,
        MARGIN,
        location_y,
        line_height,
        panel::HEIGHT as i32 - MARGIN,
        location.as_str(),
    );
    console::println!("CRASH_SCREEN stage=drawn");

    display.flush();
    console::println!("CRASH_SCREEN stage=flush_done result=success");
}
