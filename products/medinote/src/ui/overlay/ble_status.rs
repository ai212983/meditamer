#![forbid(unsafe_code)]
//! Passive LVGL projection of Medinote's base-owned sticky CheerTok status.
//!
//! The target's runtime owns this object and every update; the BLE task
//! only publishes a copyable bounded state through
//! `targets/medinote-waveshare/src/cheertok.rs`. Built through the checked
//! LVGL adapter, on `lv_layer_sys()` -- see [`super::settings`]'s doc for
//! why.

use render::lvgl_adapter::{
    Align, Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
use shell::types::{OverlayBand, OverlayInput, OverlayInstance, OverlayLifetime};

/// The three states the target's CheerTok BLE session can report. Owned
/// here, beside the widget that renders it, rather than in the target's own
/// session module -- the target still owns the actual BLE session and
/// decides which state applies; this only names what it can report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BleStatus {
    Active,
    Ready,
    Stopped,
}

pub struct BleStatusChip {
    root: Widget,
    label: Widget,
    state: Option<BleStatus>,
}

impl BleStatusChip {
    #[cfg(test)]
    pub(crate) fn is_hidden(&self, token: &UiAccessToken) -> bool {
        self.root.is_hidden(token).unwrap_or(false)
    }

    /// Update the chip only when the bounded BLE state changes.
    ///
    /// Returns `true` when LVGL now has an invalidated micro-region the
    /// target's runtime should service.
    pub fn update(&mut self, token: &UiAccessToken, state: BleStatus) -> bool {
        if self.state == Some(state) {
            return false;
        }

        let (text, background, foreground) = match state {
            BleStatus::Active => (
                c"BT",
                render::lvgl_adapter::white(),
                render::lvgl_adapter::black(),
            ),
            BleStatus::Ready => (
                c"BT",
                render::lvgl_adapter::black(),
                render::lvgl_adapter::white(),
            ),
            BleStatus::Stopped => (
                c"BT !",
                render::lvgl_adapter::white(),
                render::lvgl_adapter::black(),
            ),
        };
        let _ = self.label.set_text(token, text);
        let _ = self
            .root
            .set_bg_color(token, background, StyleState::Default);
        let _ = self.root.set_bg_opa(token, 255, StyleState::Default);
        let _ = self
            .label
            .set_text_color(token, foreground, StyleState::Default);
        let _ = self.label.center(token);
        let _ = self.root.invalidate(token);
        self.state = Some(state);
        true
    }

    /// Deletes the widget through the adapter's checked-handle contract.
    /// LVGL reporting the object still valid after deletion hands the whole
    /// widget back so the caller can retry, matching `DestroyFailure::Live`
    /// one level up.
    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root, ..self }),
        }
    }

    /// Reveals the chip. `create` stages it hidden, so pixels only change
    /// once the coordinator has actually committed this instance as live.
    pub fn show(&self, token: &UiAccessToken) {
        let _ = self.root.set_hidden(token, false);
    }

    /// Hides the chip without destroying it -- the coordinator's disable
    /// step before a teardown or capture change.
    pub fn hide(&self, token: &UiAccessToken) {
        let _ = self.root.set_hidden(token, true);
    }
}

/// Creates one passive resident projection on the system layer. A
/// partially built widget is torn down here rather than leaked; the caller
/// sees only the `None` outcome.
pub fn create(
    token: &UiAccessToken,
    instance: OverlayInstance,
    initial: BleStatus,
) -> Option<BleStatusChip> {
    if instance.band != OverlayBand::BaseSystem
        || instance.input != OverlayInput::Passive
        || instance.lifetime != OverlayLifetime::Sticky
    {
        return None;
    }

    let root = Widget::on_system_layer(token, WidgetKind::Container).ok()?;
    // Stage hidden: this instance is not yet the coordinator's committed
    // live overlay, so it must not be visible even for one LVGL tick.
    // `show()` reveals it once composition actually commits.
    let built = root.set_hidden(token, true).is_ok()
        && root.remove_style_all(token).is_ok()
        && root.set_size(token, 54, 24).is_ok()
        && root.align(token, Align::TopRight, -8, 6).is_ok()
        && root
            .set_border_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && root.set_border_width(token, 2, StyleState::Default).is_ok()
        && root.set_radius(token, 6, StyleState::Default).is_ok()
        && root.set_non_interactive(token).is_ok();
    if !built {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    }

    let Ok(label) = root.child(token, WidgetKind::Label) else {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    };
    let built = label
        .set_text_font(token, Font::Size14, StyleState::Default)
        .is_ok()
        && label.set_non_interactive(token).is_ok()
        && label.center(token).is_ok();
    if !built {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    }

    let mut chip = BleStatusChip {
        root,
        label,
        state: None,
    };
    chip.update(token, initial);
    Some(chip)
}
