#![forbid(unsafe_code)]

//! Modal clock overlay shown above the still-live Ambient Home surface.

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
use shell::types::{CompositionIntent, OverlayInput, OverlayInstance, OwnedCompositionIntent};

use super::{clock_font, clock_model};

const SURFACE_SIZE: i32 = 600;

pub(in crate::firmware::ui) struct ClockOverlay {
    instance: OverlayInstance,
    root: Widget,
    time_label: Widget,
    callbacks: CallbackLease,
    shown_at_ms: Option<u64>,
    displayed_minute: Option<u16>,
}

impl ClockOverlay {
    pub(super) fn create(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
        launcher: shell::types::SurfaceRef,
    ) -> Result<Self, super::base_overlays::OverlayEnterError> {
        use super::base_overlays::OverlayEnterError;

        if instance.input != OverlayInput::Modal {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Modal {
            navigation: Some(shell::types::OwnedNavIntent {
                source: instance.token,
                intent: shell::types::NavIntent::OpenLauncher(launcher),
            }),
            dismiss: OwnedCompositionIntent {
                source: instance.token,
                intent: CompositionIntent::DismissActiveModal,
            },
        })
        .map_err(|_| OverlayEnterError::CallbackRoute)?;
        let Ok(root) = Widget::on_system_layer(ui_token, WidgetKind::Container) else {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        };
        if root.set_hidden(ui_token, true).is_err() || root.remove_style_all(ui_token).is_err() {
            cleanup_failed_construction(root, ui_token);
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        let Some(time_label) = build_overlay(&root, ui_token, &callbacks) else {
            cleanup_failed_construction(root, ui_token);
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        };
        Ok(Self {
            instance,
            root,
            time_label,
            callbacks,
            shown_at_ms: None,
            displayed_minute: None,
        })
    }

    pub(super) fn instance(&self) -> OverlayInstance {
        self.instance
    }

    pub(super) fn root(&self) -> &Widget {
        &self.root
    }

    pub(super) fn callbacks(&self) -> &CallbackLease {
        &self.callbacks
    }

    pub(super) fn update_time(
        &mut self,
        ui_token: &UiAccessToken,
        local_epoch_seconds: u32,
        now_ms: u64,
    ) -> Result<bool, ()> {
        let minute = clock_model::displayed_minute(local_epoch_seconds);
        let changed = self.displayed_minute != Some(minute);
        if changed {
            let mut buffer = [0; 6];
            let text = clock_model::format_time_hm(&mut buffer, local_epoch_seconds).ok_or(())?;
            self.time_label.set_text(ui_token, text).map_err(|_| ())?;
            self.time_label.center(ui_token).map_err(|_| ())?;
            self.displayed_minute = Some(minute);
        }
        self.shown_at_ms = Some(now_ms);
        Ok(changed)
    }

    pub(super) fn timeout_deadline_ms(&self) -> Option<u64> {
        self.shown_at_ms
            .map(|at| at.saturating_add(clock_model::DISPLAY_DURATION_MS))
    }

    pub(super) fn timeout_elapsed(&self, now_ms: u64) -> bool {
        self.shown_at_ms
            .is_some_and(|shown_at_ms| clock_model::timeout_elapsed(shown_at_ms, now_ms))
    }

    pub(super) fn destroy_root(self, ui_token: &UiAccessToken) -> Result<CallbackLease, Self> {
        let Self {
            instance,
            root,
            time_label,
            callbacks,
            shown_at_ms,
            displayed_minute,
        } = self;
        match root.delete(ui_token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(callbacks),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                instance,
                root,
                time_label,
                callbacks,
                shown_at_ms,
                displayed_minute,
            }),
        }
    }
}

fn cleanup_failed_construction(root: Widget, ui_token: &UiAccessToken) {
    if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
        render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
    }
}

fn build_overlay(
    root: &Widget,
    ui_token: &UiAccessToken,
    callbacks: &CallbackLease,
) -> Option<Widget> {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = root.set_size(ui_token, SURFACE_SIZE, SURFACE_SIZE).is_ok()
        && root.set_pos(ui_token, 0, 0).is_ok()
        && root
            .set_bg_color(ui_token, white, StyleState::Default)
            .is_ok()
        && root.set_bg_opa(ui_token, 255, StyleState::Default).is_ok()
        && root
            .set_text_color(ui_token, black, StyleState::Default)
            .is_ok()
        && root.set_clickable(ui_token, true).is_ok()
        && intent_bridge::bind_ambient_tap(root, ui_token).is_ok();
    if !built {
        return None;
    }

    let time_label = root.child(ui_token, WidgetKind::Label).ok()?;
    let built = time_label.set_text(ui_token, c"").is_ok()
        && time_label
            .set_text_font(
                ui_token,
                Font::custom(clock_font::font()),
                StyleState::Default,
            )
            .is_ok()
        && time_label
            .set_text_color(ui_token, black, StyleState::Default)
            .is_ok()
        && time_label.center(ui_token).is_ok()
        && time_label.set_non_interactive(ui_token).is_ok();
    if !built
        || !create_button(
            root,
            ui_token,
            callbacks,
            100,
            c"Launcher",
            RouteCallback::Navigation { index: 0 },
        )
        || !create_button(
            root,
            ui_token,
            callbacks,
            320,
            c"Back",
            RouteCallback::DismissModal,
        )
    {
        return None;
    }
    Some(time_label)
}

fn create_button(
    parent: &Widget,
    ui_token: &UiAccessToken,
    callbacks: &CallbackLease,
    x: i32,
    text: &core::ffi::CStr,
    action: RouteCallback,
) -> bool {
    let Ok(button) = parent.child(ui_token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(ui_token).is_ok()
        && button.set_size(ui_token, 180, 64).is_ok()
        && button.set_pos(ui_token, x, 500).is_ok()
        && button.set_ext_click_area(ui_token, 16).is_ok()
        && button
            .set_bg_color(ui_token, white, StyleState::Default)
            .is_ok()
        && button
            .set_bg_opa(ui_token, 255, StyleState::Default)
            .is_ok()
        && button
            .set_text_color(ui_token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_color(ui_token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_width(ui_token, 3, StyleState::Default)
            .is_ok()
        && button.set_radius(ui_token, 8, StyleState::Default).is_ok()
        && button
            .set_bg_color(ui_token, black, StyleState::Pressed)
            .is_ok()
        && button
            .set_text_color(ui_token, white, StyleState::Pressed)
            .is_ok()
        && intent_bridge::bind_click(&button, ui_token, action, callbacks).is_ok();
    if !built {
        return false;
    }
    let Ok(label) = button.child(ui_token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(ui_token, text).is_ok()
        && label
            .set_text_font(ui_token, Font::Size18, StyleState::Default)
            .is_ok()
        && label.center(ui_token).is_ok()
}
