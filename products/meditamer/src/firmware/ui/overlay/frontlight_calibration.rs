#![forbid(unsafe_code)]

//! Runtime-only frontlight calibration modal.

use core::ffi::CStr;

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, TextAlign, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
use shell::types::{OverlayInput, OverlayInstance};

use super::base_overlays::OverlayEnterError;

pub(in crate::firmware::ui) const ACTION_SLIDER: usize = 0;
pub(in crate::firmware::ui) const ACTION_ON: usize = 2;
pub(in crate::firmware::ui) const ACTION_OFF: usize = 3;
pub(in crate::firmware::ui) const ACTION_CANCEL: usize = 4;
pub(in crate::firmware::ui) const FRONTLIGHT_SLIDER_MIN: i32 = 0;
pub(in crate::firmware::ui) const FRONTLIGHT_SLIDER_MAX: i32 = 63;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrontlightCalibrationEffect {
    Preview(u8),
    Commit(u8),
    Cancel,
}

pub(in crate::firmware::ui) struct FrontlightCalibration {
    instance: OverlayInstance,
    root: Widget,
    level_label: Widget,
    callbacks: CallbackLease,
    level: u8,
}

impl FrontlightCalibration {
    pub(super) fn create(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
        initial_level: u8,
    ) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Modal {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Action {
            source: instance.token,
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
        let Some(level_label) = build_overlay(&root, ui_token, &callbacks, initial_level) else {
            cleanup_failed_construction(root, ui_token);
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        };
        Ok(Self {
            instance,
            root,
            level_label,
            callbacks,
            level: initial_level.min(63),
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

    pub(super) fn apply_action(&mut self, index: usize) -> Option<FrontlightCalibrationEffect> {
        match index {
            ACTION_ON => Some(FrontlightCalibrationEffect::Commit(self.level)),
            ACTION_OFF => Some(FrontlightCalibrationEffect::Commit(0)),
            ACTION_CANCEL => Some(FrontlightCalibrationEffect::Cancel),
            _ => None,
        }
    }

    pub(super) fn apply_value(
        &mut self,
        ui_token: &UiAccessToken,
        action: intent_bridge::IndexedValueAction,
    ) -> Option<FrontlightCalibrationEffect> {
        if action.index != ACTION_SLIDER {
            return None;
        }
        let level = clamp_slider_level(action.value);
        self.update_level(ui_token, level)
            .then_some(FrontlightCalibrationEffect::Preview(level))
    }

    fn update_level(&mut self, ui_token: &UiAccessToken, level: u8) -> bool {
        if self.level == level {
            return true;
        }
        let mut buffer = [0; 3];
        let text = format_level(&mut buffer, level);
        if self.level_label.set_text(ui_token, text).is_err() {
            return false;
        }
        self.level = level;
        true
    }

    pub(super) fn destroy_root(self, ui_token: &UiAccessToken) -> Result<CallbackLease, Self> {
        let Self {
            instance,
            root,
            level_label,
            callbacks,
            level,
        } = self;
        match root.delete(ui_token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(callbacks),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                instance,
                root,
                level_label,
                callbacks,
                level,
            }),
        }
    }
}

fn clamp_slider_level(value: i32) -> u8 {
    value
        .clamp(FRONTLIGHT_SLIDER_MIN, FRONTLIGHT_SLIDER_MAX)
        .try_into()
        .unwrap_or(0)
}

fn format_level<'a>(buffer: &'a mut [u8; 3], level: u8) -> &'a CStr {
    let level = level.min(63);
    if level >= 10 {
        buffer[0] = b'0' + level / 10;
        buffer[1] = b'0' + level % 10;
        buffer[2] = 0;
    } else {
        buffer[0] = b'0' + level;
        buffer[1] = 0;
        buffer[2] = 0;
    }
    CStr::from_bytes_until_nul(buffer).expect("frontlight level is nul-terminated")
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
    initial_level: u8,
) -> Option<Widget> {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = root.set_size(ui_token, 500, 420).is_ok()
        && root.set_pos(ui_token, 50, 90).is_ok()
        && root
            .set_bg_color(ui_token, white, StyleState::Default)
            .is_ok()
        && root.set_bg_opa(ui_token, 255, StyleState::Default).is_ok()
        && root
            .set_border_color(ui_token, black, StyleState::Default)
            .is_ok()
        && root
            .set_border_width(ui_token, 4, StyleState::Default)
            .is_ok()
        && root.set_radius(ui_token, 10, StyleState::Default).is_ok();
    if !built {
        return None;
    }

    let title = root.child(ui_token, WidgetKind::Label).ok()?;
    let built = title.set_text(ui_token, c"Default backlight").is_ok()
        && title
            .set_text_font(ui_token, Font::Size24, StyleState::Default)
            .is_ok()
        && title.set_pos(ui_token, 133, 24).is_ok();
    if !built {
        return None;
    }

    let level_label = root.child(ui_token, WidgetKind::Label).ok()?;
    let mut buffer = [0; 3];
    let level_text = format_level(&mut buffer, initial_level);
    let built = level_label.set_text(ui_token, level_text).is_ok()
        && level_label.set_size(ui_token, 160, 76).is_ok()
        && level_label.set_pos(ui_token, 170, 78).is_ok()
        && level_label
            .set_text_font(ui_token, Font::Size32, StyleState::Default)
            .is_ok()
        && level_label
            .set_text_align(ui_token, TextAlign::Center, StyleState::Default)
            .is_ok()
        && level_label
            .set_bg_color(ui_token, black, StyleState::Default)
            .is_ok()
        && level_label
            .set_bg_opa(ui_token, 255, StyleState::Default)
            .is_ok()
        && level_label
            .set_text_color(ui_token, white, StyleState::Default)
            .is_ok()
        && level_label
            .set_radius(ui_token, 8, StyleState::Default)
            .is_ok();
    if !built
        || !create_slider(root, ui_token, callbacks, initial_level)
        || !create_button(
            root,
            ui_token,
            callbacks,
            ButtonSpec {
                x: 28,
                text: c"On",
                action: ACTION_ON,
            },
        )
        || !create_button(
            root,
            ui_token,
            callbacks,
            ButtonSpec {
                x: 185,
                text: c"Off",
                action: ACTION_OFF,
            },
        )
        || !create_button(
            root,
            ui_token,
            callbacks,
            ButtonSpec {
                x: 342,
                text: c"Cancel",
                action: ACTION_CANCEL,
            },
        )
    {
        return None;
    }
    Some(level_label)
}

fn create_slider(
    parent: &Widget,
    ui_token: &UiAccessToken,
    callbacks: &CallbackLease,
    initial_level: u8,
) -> bool {
    let Ok(slider) = parent.child(ui_token, WidgetKind::Slider) else {
        return false;
    };
    let level = i32::from(initial_level.min(63));
    slider.set_size(ui_token, 420, 80).is_ok()
        && slider.set_pos(ui_token, 40, 168).is_ok()
        && slider
            .set_slider_range(ui_token, FRONTLIGHT_SLIDER_MIN, FRONTLIGHT_SLIDER_MAX)
            .is_ok()
        && slider.set_slider_value(ui_token, level).is_ok()
        && slider.set_ext_click_area(ui_token, 16).is_ok()
        && intent_bridge::bind_click(
            &slider,
            ui_token,
            RouteCallback::Value {
                index: ACTION_SLIDER,
            },
            callbacks,
        )
        .is_ok()
}

struct ButtonSpec<'a> {
    x: i32,
    text: &'a CStr,
    action: usize,
}

fn create_button(
    parent: &Widget,
    ui_token: &UiAccessToken,
    callbacks: &CallbackLease,
    spec: ButtonSpec<'_>,
) -> bool {
    let Ok(button) = parent.child(ui_token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(ui_token).is_ok()
        && button.set_size(ui_token, 130, 64).is_ok()
        && button.set_pos(ui_token, spec.x, 310).is_ok()
        && button.set_ext_click_area(ui_token, 10).is_ok()
        && button
            .set_bg_color(ui_token, white, StyleState::Default)
            .is_ok()
        && button
            .set_bg_opa(ui_token, 255, StyleState::Default)
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
        && intent_bridge::bind_click(
            &button,
            ui_token,
            RouteCallback::Action { index: spec.action },
            callbacks,
        )
        .is_ok();
    if !built {
        return false;
    }
    let Ok(label) = button.child(ui_token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(ui_token, spec.text).is_ok()
        && label
            .set_text_font(ui_token, Font::Size18, StyleState::Default)
            .is_ok()
        && label.center(ui_token).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_level_clamps_to_backlight_range() {
        assert_eq!(clamp_slider_level(i32::MIN), 0);
        assert_eq!(clamp_slider_level(-1), 0);
        assert_eq!(clamp_slider_level(0), 0);
        assert_eq!(clamp_slider_level(8), 8);
        assert_eq!(clamp_slider_level(63), 63);
        assert_eq!(clamp_slider_level(64), 63);
        assert_eq!(clamp_slider_level(i32::MAX), 63);
    }

    #[test]
    fn slider_route_index_does_not_collide_with_commit_buttons() {
        assert_ne!(ACTION_SLIDER, ACTION_ON);
        assert_ne!(ACTION_SLIDER, ACTION_OFF);
        assert_ne!(ACTION_SLIDER, ACTION_CANCEL);
        assert_eq!((FRONTLIGHT_SLIDER_MIN, FRONTLIGHT_SLIDER_MAX), (0, 63));
    }
}
