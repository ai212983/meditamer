//! Waveshare ESP32-S3 sleep entry and wake-source configuration.
//!
//! This remains target-local because GPIO wake capabilities and panel
//! retention differ by board. A future Inkplate target should provide its own
//! sibling module; a shared contract can be extracted once both concrete
//! implementations exist.
//!
//! esp-hal 1.2.0 replaced the 1.1.x sleep API (`Rtc::sleep_deep`/
//! `sleep_light` taking a slice of `&dyn WakeSource`) with one where each
//! driver that owns a wakeup source arms it directly, and a separate
//! [`LowPower`] handle (over the `LPWR` peripheral, not `RTC_TIMER`) reads
//! back the resulting wakeup-enable mask and performs the sleep transition
//! itself (esp-rs/esp-hal#6060). Concretely:
//! - the timer source is armed with [`LowPower::set_wakeup_deadline`], an
//!   absolute [`Instant`], instead of constructing a `TimerWakeupSource`;
//! - a GPIO source is armed by making the pin listen for the trigger level
//!   (`Input::listen`) and, for a path that survives digital-peripheral
//!   power-down, requesting the low-power path via
//!   [`Input::apply_wakeup_config`] -- there is no more separate
//!   `Ext0WakeupSource`;
//! - `sleep_deep`/`sleep_light` now take an explicit [`RtcSleepConfig`]
//!   rather than assuming one internally; `RtcSleepConfig::deep()` and
//!   `RtcSleepConfig::default()` are exactly what the old `Rtc::sleep_deep`/
//!   `sleep_light` used internally, so passing those reproduces the same
//!   configuration.

use esp_hal::gpio::{Event, Input, InputConfig, Pull, WakeupConfig};
use esp_hal::peripherals::GPIO18;
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use esp_hal::time::{Duration, Instant};

/// Temporary device-validation escape hatch. Deep Sleep resets after this
/// interval; Sleep can wake earlier from an active-low KEY press.
const VALIDATION_WAKE_TIMEOUT_S: u64 = 15;

/// Drain the final diagnostic packet before deep sleep removes USB power.
/// `esp-println` only submits it with WR_DONE. The S3 EP1_CONF contract keeps
/// SERIAL_IN_EP_DATA_FREE low after submission until the host reads the FIFO.
/// Like ESP-IDF's USB Serial JTAG fsync, cap the wait at 50ms so an unplugged
/// console never prevents sleep. This observes completion, not a fixed delay.
fn drain_console_before_deep_sleep() {
    let usb = esp_hal::peripherals::USB_DEVICE::regs();
    usb.ep1_conf().modify(|_, w| w.wr_done().set_bit());
    let deadline = Instant::now() + Duration::from_millis(50);
    while usb
        .ep1_conf()
        .read()
        .serial_in_ep_data_free()
        .bit_is_clear()
    {
        if Instant::now() >= deadline {
            return;
        }
        core::hint::spin_loop();
    }
}

/// Enter full ESP32-S3 Deep Sleep.
///
/// The ST7305 card has already been flushed by the UI, but the current board
/// has no retained hardware bias on its CS and RESET lines. The panel therefore
/// blanks while the digital pad domain is off. Deep Sleep returns through reset
/// to Home when the validation timer fires.
pub fn enter_deep_sleep(low_power: &mut LowPower<'static>) -> ! {
    console::println!(
        "DEEP_SLEEP_ENTER wake=timer-only duration_s={} destination=home",
        VALIDATION_WAKE_TIMEOUT_S
    );
    console::println!("DISPLAY_SLEEP_RETENTION deep_hold=off");
    drain_console_before_deep_sleep();
    #[cfg(feature = "wifi-storage")]
    crate::network_retention::arm_if_enabled(crate::net_host::desired_for_sleep());
    low_power.set_wakeup_deadline(Instant::now() + Duration::from_secs(VALIDATION_WAKE_TIMEOUT_S));
    low_power.sleep_deep(RtcSleepConfig::deep())
}

/// Enter ESP32-S3 light sleep, exposed to the user simply as `Sleep`.
///
/// The active display state remains visible. GPIO18 KEY wakes the existing
/// runtime in place; the timer is only a validation failsafe.
pub fn enter_sleep(low_power: &mut LowPower<'static>, key_pin: &mut GPIO18<'static>) {
    console::println!(
        "SLEEP_ENTER wake=GPIO18-low failsafe_s={} destination=home",
        VALIDATION_WAKE_TIMEOUT_S
    );

    // A pull-up holds the pad high (against the low level that wakes the
    // chip) so it does not wake immediately on a floating/idle KEY; the
    // pin's own interrupt trigger (`Event::LowLevel`, set via `listen`) is
    // now what a GPIO wakeup source is, replacing the old
    // `Ext0WakeupSource::new(key_pin.reborrow(), WakeupLevel::Low)`. The
    // low-power path keeps this pin listening while sleep powers the
    // digital GPIO peripheral down, matching what EXT0 gave the old API.
    let mut key = Input::new(
        key_pin.reborrow(),
        InputConfig::default().with_pull(Pull::Up),
    );
    key.listen(Event::LowLevel);
    key.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("GPIO18 has no low-power wakeup path");

    low_power.set_wakeup_deadline(Instant::now() + Duration::from_secs(VALIDATION_WAKE_TIMEOUT_S));
    console::println!("DISPLAY_SLEEP_RETENTION active_gpio=unchanged sleep_pad_override=off");
    let entered = Instant::now();
    low_power.sleep_light(RtcSleepConfig::default());
    console::println!(
        "SLEEP_RESUME wake={:?} elapsed_us={}",
        esp_hal::rtc_cntl::wakeup_cause(),
        entered.elapsed().as_micros()
    );
}
