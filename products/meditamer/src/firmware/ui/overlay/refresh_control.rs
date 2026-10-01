#![forbid(unsafe_code)]
//! Sticky refresh-control overlay, built on the LVGL safety adapter. Kept
//! separate from `base_overlays` since it is its own reusable widget, not
//! part of the base-overlay dispatch enum's own construction code.

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
use shell::types::{OverlayInput, OverlayInstance, OwnedRefreshIntent, RefreshIntent};

use super::base_overlays::OverlayEnterError;

pub(in crate::firmware::ui) struct RefreshControl {
    instance: OverlayInstance,
    root: Widget,
    callbacks: intent_bridge::CallbackLease,
}

impl RefreshControl {
    pub(in crate::firmware::ui) fn create(
        token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Interactive {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Refresh {
            request: OwnedRefreshIntent {
                source: instance.token,
                intent: RefreshIntent::FullRepaint,
            },
        })
        .map_err(|_| OverlayEnterError::CallbackRoute)?;

        let Ok(root) = Widget::on_system_layer(token, WidgetKind::Button) else {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        };
        if !build(&root, token, &callbacks) {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        if intent_bridge::enable(&callbacks).is_err() {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::CallbackRoute);
        }
        Ok(Self {
            instance,
            root,
            callbacks,
        })
    }

    pub(in crate::firmware::ui) fn instance(&self) -> OverlayInstance {
        self.instance
    }

    pub(in crate::firmware::ui) fn callbacks(&self) -> &intent_bridge::CallbackLease {
        &self.callbacks
    }

    /// The checked handle itself, for `ActiveOverlay`'s cross-kind show/hide
    /// glue in `base_overlays.rs`.
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    /// Deletes the overlay's root through the adapter's checked-handle
    /// contract, so `ActiveOverlay::destroy` frees this slice's registry
    /// entries instead of leaking them through a raw `lv_obj_delete`. On
    /// success, hands back the callback lease for the caller to release --
    /// matching the generic path, which separates the two steps. LVGL
    /// reporting the object still valid after deletion hands the whole
    /// overlay back so the caller can retry, mirroring `DestroyFailure::
    /// Live` one level up in `ActiveOverlay::destroy`.
    pub(in crate::firmware::ui) fn destroy_root(
        self,
        token: &UiAccessToken,
    ) -> Result<intent_bridge::CallbackLease, Self> {
        let Self {
            instance,
            root,
            callbacks,
        } = self;
        match root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(callbacks),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                instance,
                root,
                callbacks,
            }),
        }
    }
}

fn build(root: &Widget, token: &UiAccessToken, lease: &CallbackLease) -> bool {
    root.set_hidden(token, true).is_ok()
        && root.remove_style_all(token).is_ok()
        && root.set_size(token, 112, 38).is_ok()
        && root.set_pos(token, 474, 12).is_ok()
        && root
            .set_bg_color(token, render::lvgl_adapter::white(), StyleState::Default)
            .is_ok()
        && root.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && root
            .set_border_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && root.set_border_width(token, 2, StyleState::Default).is_ok()
        && root.set_radius(token, 6, StyleState::Default).is_ok()
        && intent_bridge::bind_click(root, token, RouteCallback::FullRepaint, lease).is_ok()
        && create_label(root, token, c"STICKY", 22, 8)
}

fn create_label(
    parent: &Widget,
    token: &UiAccessToken,
    text: &'static core::ffi::CStr,
    x: i32,
    y: i32,
) -> bool {
    let Ok(label) = parent.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, text).is_ok()
        && label
            .set_text_font(token, Font::Size14, StyleState::Default)
            .is_ok()
        && label.set_pos(token, x, y).is_ok()
}
