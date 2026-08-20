//! Host coverage for the Inkplate L8-to-panel blit.
//!
//! `src/platform/inkplate/panel_blit.rs` is board code inside the firmware
//! crate, which sets `[lib] test = false`, so its inline tests cannot run
//! there. This shim re-hosts it, supplying the panel geometry it reads from its
//! parent module. It goes away when `boards/inkplate-tempera` becomes a crate
//! that can test itself (ADR-0015).

pub const E_INK_WIDTH: usize = 600;
pub const E_INK_HEIGHT: usize = 600;
pub const FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 8;

#[path = "../../../src/platform/inkplate/panel_blit.rs"]
mod panel_blit;
