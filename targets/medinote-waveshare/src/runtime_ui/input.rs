//! Target-owned KEY/BOOT sampling, modal-capture gating, and event delivery.
//!
//! Deliberately does not own the raw `key_pin`/`boot_pin` peripherals or the
//! `Button` values built from them: the sleep sequence needs the
//! bare pin directly (`sleep::enter_sleep(&mut rtc, &mut key_pin)`) and
//! reconstructs a fresh `Button`/`CaptureGate` pair as part of waking, so
//! `mod.rs` keeps that ownership and only lends `&Button` references in here
//! for sampling.

use medinote::input::{ButtonEvent, ButtonEventKind, ButtonEvents, CaptureGate};
use shell::types::SurfaceInstanceToken;
use waveshare_rlcd42::buttons::Button;

pub(crate) fn log_button_event(event: ButtonEvent) {
    console::println!(
        "BUTTON_EVENT button={} kind={} at_ms={} held_ms={}",
        event.button.as_str(),
        event.kind.as_str(),
        event.at_ms,
        event.held_ms,
    );
}

fn sample_button(gate: &mut CaptureGate, button: &Button<'_>, now_ms: u64) -> ButtonEvents {
    gate.sample(button.is_pressed(), now_ms)
}

/// One tick's worth of KEY/BOOT recognition, ready for the loop to branch on.
pub(crate) struct ButtonTick {
    pub(crate) key_pressed: bool,
    pub(crate) key_pressed_at_us: Option<u64>,
    /// A completed short tap on KEY (release before the long-press
    /// threshold, and not the second half of a double-click) -- what the
    /// Launcher uses to cycle its own selection once more than one app is
    /// launchable. `medinote::input::key_action`'s doc explains why the
    /// Launcher needs this distinction and `Ambient`/`AppRoot` do not.
    pub(crate) key_clicked: bool,
    /// KEY held past the long-press threshold -- what the Launcher uses to
    /// open whichever entry is currently selected.
    pub(crate) key_long_pressed: bool,
    pub(crate) boot_pressed: bool,
}

/// Borrowed GPIO and recognition state needed for one UI-loop sample.
pub(crate) struct ButtonSample<'state, 'button> {
    pub(crate) key_gate: &'state mut CaptureGate,
    pub(crate) boot_gate: &'state mut CaptureGate,
    pub(crate) key_button: &'state Button<'button>,
    pub(crate) boot_button: &'state Button<'button>,
    pub(crate) surface_instance: SurfaceInstanceToken,
    pub(crate) modal_active: bool,
    pub(crate) now_ms: u64,
    pub(crate) now_us: u64,
}

/// Applies the exact surface-instance ownership boundary and modal capture to
/// both gates, then samples both buttons and reports the accepted edges.
///
/// Capture is set *before* sampling, through each `CaptureGate`, not just
/// checked after on the resulting `pressed` flags: gating only the result
/// would leave the recognizer's own state machine running underneath a live
/// modal, so a hold or a pending double-click that began while captured
/// input was blocked could still resolve once capture returns -- "a new
/// surface must not inherit an old hold or click." `CaptureGate::
/// set_captured` cancels any in-flight interaction immediately on capture
/// loss instead.
pub(crate) fn sample_tick(sample: ButtonSample<'_, '_>) -> ButtonTick {
    for event in sample
        .key_gate
        .set_surface_instance(sample.surface_instance, sample.now_ms)
    {
        log_button_event(event);
    }
    for event in sample
        .boot_gate
        .set_surface_instance(sample.surface_instance, sample.now_ms)
    {
        log_button_event(event);
    }
    for event in sample
        .key_gate
        .set_captured(!sample.modal_active, sample.now_ms)
    {
        log_button_event(event);
    }
    for event in sample
        .boot_gate
        .set_captured(!sample.modal_active, sample.now_ms)
    {
        log_button_event(event);
    }
    let key_events = sample_button(sample.key_gate, sample.key_button, sample.now_ms);
    let boot_events = sample_button(sample.boot_gate, sample.boot_button, sample.now_ms);
    let key_pressed = key_events
        .iter()
        .any(|event| event.kind == ButtonEventKind::Pressed);
    let key_pressed_at_us = key_pressed.then_some(sample.now_us);
    let key_clicked = key_events
        .iter()
        .any(|event| event.kind == ButtonEventKind::Click);
    let key_long_pressed = key_events
        .iter()
        .any(|event| event.kind == ButtonEventKind::LongPress);
    let boot_pressed = boot_events
        .iter()
        .any(|event| event.kind == ButtonEventKind::Pressed);
    for event in key_events.iter().chain(boot_events.iter()) {
        log_button_event(*event);
    }
    ButtonTick {
        key_pressed,
        key_pressed_at_us,
        key_clicked,
        key_long_pressed,
        boot_pressed,
    }
}
