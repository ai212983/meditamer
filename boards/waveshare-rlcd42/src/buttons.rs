//! Active-low board buttons.
//!
//! Waveshare's board pin assignment names GPIO18 as the active-low `KEY`
//! input. GPIO0 is the separate `BOOT` button and must remain available for
//! the ROM download path.

use esp_hal::gpio::{Input, InputConfig, Pull};

pub const KEY_GPIO_DESCRIPTION: &str = "GPIO18 (KEY), active-low";
pub const BOOT_GPIO_DESCRIPTION: &str = "GPIO0 (BOOT), active-low";

pub fn button_input_config() -> InputConfig {
    InputConfig::default().with_pull(Pull::Up)
}

/// Thin hardware wrapper around one active-low GPIO input.
///
/// Debounce and interaction recognition intentionally live in the target's
/// `input` module. That keeps this board crate responsible only for GPIO
/// ownership and lets the recognizer use monotonic time rather than a
/// board-specific polling cadence.
pub struct Button<'d> {
    input: Input<'d>,
}

impl<'d> Button<'d> {
    pub fn new(input: Input<'d>) -> Button<'d> {
        Button { input }
    }

    /// Returns the raw active-low physical state. `true` means pressed.
    pub fn is_pressed(&self) -> bool {
        self.input.is_low()
    }

    /// Release the GPIO input so the target can temporarily reconfigure the
    /// same pin as an RTC wake source.
    pub fn into_inner(self) -> Input<'d> {
        self.input
    }
}
