#![forbid(unsafe_code)]
//! Explicit Sleep/Deep Sleep screens, built through the checked LVGL
//! adapter.
//!
//! Neither is a catalogue-navigable surface -- there is no `SurfaceRole` or
//! shell instance for "asleep" -- so, unlike [`super::home`]/
//! [`super::launcher`]/[`super::hourglass`], these are loaded and torn down
//! directly by the target around its own sleep sequencing rather than
//! through `UiCoordinator`'s transaction path. Home's own screen instance is
//! untouched throughout (deep sleep powers the chip off; regular sleep's
//! wake path reloads Home's still-live root instead of rebuilding it).

use render::lvgl_adapter::{
    Align, Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

pub struct PowerScreen {
    root: Widget,
}

impl PowerScreen {
    /// Loads this screen as LVGL's active screen. Returns whether LVGL
    /// reports it active afterward.
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }

    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root }),
        }
    }
}

pub fn create_sleep(token: &UiAccessToken) -> Option<PowerScreen> {
    create(token, c"SLEEP", c"CPU PAUSED")
}

pub fn create_deep_sleep(token: &UiAccessToken) -> Option<PowerScreen> {
    create(token, c"DEEP SLEEP", c"CPU OFF")
}

fn create(
    token: &UiAccessToken,
    heading_text: &'static core::ffi::CStr,
    state_text: &'static core::ffi::CStr,
) -> Option<PowerScreen> {
    let screen = Widget::screen(token).ok()?;
    if build(&screen, token, heading_text, state_text) {
        Some(PowerScreen { root: screen })
    } else {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        None
    }
}

fn build(
    screen: &Widget,
    token: &UiAccessToken,
    heading_text: &'static core::ffi::CStr,
    state_text: &'static core::ffi::CStr,
) -> bool {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();

    let Ok(card) = screen.child(token, WidgetKind::Container) else {
        return false;
    };
    let built = card.set_size(token, 340, 210).is_ok()
        && card.align(token, Align::Center, 0, 0).is_ok()
        && card.set_border_width(token, 5, StyleState::Default).is_ok()
        && card
            .set_border_color(token, black, StyleState::Default)
            .is_ok()
        && card.set_bg_color(token, black, StyleState::Default).is_ok()
        && card.set_radius(token, 0, StyleState::Default).is_ok();
    if !built {
        return false;
    }

    let Ok(heading) = card.child(token, WidgetKind::Label) else {
        return false;
    };
    let built = heading.set_text(token, heading_text).is_ok()
        && heading
            .set_text_font(token, Font::Size24, StyleState::Default)
            .is_ok()
        && heading
            .set_text_color(token, white, StyleState::Default)
            .is_ok()
        && heading.align(token, Align::Center, 0, -52).is_ok();
    if !built {
        return false;
    }

    let Ok(state) = card.child(token, WidgetKind::Label) else {
        return false;
    };
    let built = state.set_text(token, state_text).is_ok()
        && state
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && state
            .set_text_color(token, white, StyleState::Default)
            .is_ok()
        && state.align(token, Align::Center, 0, -5).is_ok();
    if !built {
        return false;
    }

    let Ok(wake) = card.child(token, WidgetKind::Label) else {
        return false;
    };
    wake.set_text(token, c"Press KEY to wake").is_ok()
        && wake
            .set_text_font(token, Font::Size14, StyleState::Default)
            .is_ok()
        && wake
            .set_text_color(token, white, StyleState::Default)
            .is_ok()
        && wake.align(token, Align::Center, 0, 48).is_ok()
}
