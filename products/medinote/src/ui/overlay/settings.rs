#![forbid(unsafe_code)]
//! Settings overlay presented over a live Home screen.
//!
//! Medinote has no touchscreen, so `UISETTINGS` and `UICLOSE` invoke this
//! non-interactive panel programmatically. It lives on LVGL's system layer.

use lightvgl_sys as lv;
use render::lvgl_adapter::{StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind};
use shell::types::{OverlayBand, OverlayInput, OverlayInstance, OverlayLifetime};

pub struct Settings {
    root: Widget,
}

impl Settings {
    #[cfg(test)]
    pub(crate) fn is_hidden(&self, token: &UiAccessToken) -> bool {
        self.root.is_hidden(token).unwrap_or(false)
    }

    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root }),
        }
    }

    /// Reveals the panel. `create` stages it hidden, so pixels only change
    /// once the coordinator has actually committed this instance as live.
    pub fn show(&self, token: &UiAccessToken) {
        let _ = self.root.set_hidden(token, false);
    }

    /// Hides the panel without destroying it -- the coordinator's disable
    /// step before a teardown or capture change.
    pub fn hide(&self, token: &UiAccessToken) {
        let _ = self.root.set_hidden(token, true);
    }
}

/// Builds the panel over whatever Home has already drawn onto its own
/// (untouched, still-live) root. A partially built overlay is torn down
/// here rather than leaked; the caller sees only the `None` outcome.
pub fn create(token: &UiAccessToken, instance: OverlayInstance) -> Option<Settings> {
    if instance.band != OverlayBand::BaseSystem
        || instance.input != OverlayInput::Modal
        || instance.lifetime != OverlayLifetime::Transient
    {
        return None;
    }

    let root = Widget::on_system_layer(token, WidgetKind::Container).ok()?;
    // Stage hidden: this instance is not yet the coordinator's committed
    // live overlay, so it must not be visible even for one LVGL tick.
    // `show()` reveals it once composition actually commits.
    if root.set_hidden(token, true).is_err() || !build(&root, token) {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    }
    Some(Settings { root })
}

fn build(root: &Widget, token: &UiAccessToken) -> bool {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = root.remove_style_all(token).is_ok()
        && root.set_size(token, 260, 140).is_ok()
        && root.center(token).is_ok()
        && root.set_bg_color(token, white, StyleState::Default).is_ok()
        && root.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && root
            .set_border_color(token, black, StyleState::Default)
            .is_ok()
        && root.set_border_width(token, 3, StyleState::Default).is_ok()
        && root.set_radius(token, 8, StyleState::Default).is_ok();
    if !built {
        return false;
    }

    create_label(root, token, c"Settings", 16, 14, black)
        && create_label(
            root,
            token,
            c"Home stays live behind this panel.",
            12,
            52,
            black,
        )
        && create_label(root, token, c"UICLOSE to dismiss.", 12, 76, black)
}

fn create_label(
    parent: &Widget,
    token: &UiAccessToken,
    text: &'static core::ffi::CStr,
    x: i32,
    y: i32,
    color: lv::lv_color_t,
) -> bool {
    let Ok(label) = parent.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, text).is_ok()
        && label
            .set_text_font(
                token,
                render::lvgl_adapter::Font::Size14,
                StyleState::Default,
            )
            .is_ok()
        && label
            .set_text_color(token, color, StyleState::Default)
            .is_ok()
        && label.set_pos(token, x, y).is_ok()
}
