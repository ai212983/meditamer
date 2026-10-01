#![forbid(unsafe_code)]
//! Multi-touch gesture diagnostics screen, built on the LVGL safety adapter.

use core::fmt::Write;

use heapless::String;
use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

use crate::firmware::ui::lvgl::io::{
    LvglGestureDirection, LvglGestureEvent, LvglGestureKind, LvglGestureState,
};
use crate::firmware::ui::widget::carousel;

const RESULT_TEXT_CAPACITY: usize = 192;

pub(in crate::firmware::ui) struct GestureTestScreen {
    root: Widget,
    result_label: Widget,
    gesture_count: u32,
}

impl GestureTestScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        let Self {
            root,
            result_label,
            gesture_count,
        } = self;
        match root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                root,
                result_label,
                gesture_count,
            }),
        }
    }

    pub(in crate::firmware::ui) fn show_gesture(
        &mut self,
        token: &UiAccessToken,
        event: LvglGestureEvent,
        active: bool,
    ) -> bool {
        if event.state != LvglGestureState::Ended || !active {
            return false;
        }

        self.gesture_count = self.gesture_count.saturating_add(1);
        let sequence = self.gesture_count;
        let mut text = String::<RESULT_TEXT_CAPACITY>::new();
        match event.kind {
            LvglGestureKind::Pinch { scale } => {
                let motion = if scale >= 1.0 { "out" } else { "in" };
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nPinch {motion}\nScale: {scale:.3}\nLVGL state: ended"
                );
            }
            LvglGestureKind::Rotation { radians } => {
                let degrees = radians * 57.295_78;
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nRotation\nRadians: {radians:.3}\nDegrees: {degrees:.1}\nLVGL state: ended"
                );
            }
            LvglGestureKind::TwoFingerSwipe {
                direction,
                distance_px,
            } => {
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nTwo-finger swipe\nDirection: {}\nDistance: {distance_px:.1} px\nLVGL state: ended",
                    direction_label(direction)
                );
            }
        }
        if text.push('\0').is_err() {
            return false;
        }
        let Ok(text) = core::ffi::CStr::from_bytes_with_nul(text.as_bytes()) else {
            return false;
        };
        self.result_label.set_text(token, text).is_ok()
    }
}

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    lease: &CallbackLease,
) -> Option<GestureTestScreen> {
    let screen = Widget::screen(token).ok()?;
    let Some(result_label) = build_screen(&screen, token, lease) else {
        discard_incomplete_screen(screen, token);
        return None;
    };

    Some(GestureTestScreen {
        root: screen,
        result_label,
        gesture_count: 0,
    })
}

fn build_screen(screen: &Widget, token: &UiAccessToken, lease: &CallbackLease) -> Option<Widget> {
    if !style_screen(screen, token)
        || !create_positioned_label(
            screen,
            token,
            c"Multi-gesture test",
            Font::Size24,
            182,
            42,
        )
        || !create_positioned_label(
            screen,
            token,
            c"Use two fingers to pinch, rotate, or swipe.\nThe result appears after both fingers are released.",
            Font::Size18,
            42,
            126,
        )
    {
        return None;
    }

    let result_panel = create_result_panel(screen, token)?;
    let result_label = create_result_label(&result_panel, token)?;
    if !create_overlay_demo_button(&result_panel, token, lease)
        || !carousel::add_navigation(
            screen,
            token,
            c"3 / 3",
            intent_bridge::HOME_NAVIGATION_INDEX,
            intent_bridge::HOME_NAVIGATION_INDEX,
            lease,
        )
    {
        return None;
    }
    Some(result_label)
}

fn style_screen(screen: &Widget, token: &UiAccessToken) -> bool {
    screen
        .set_bg_color(token, render::lvgl_adapter::white(), StyleState::Default)
        .is_ok()
        && screen.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && screen
            .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
}

fn create_positioned_label(
    parent: &Widget,
    token: &UiAccessToken,
    text: &'static core::ffi::CStr,
    font: Font,
    x: i32,
    y: i32,
) -> bool {
    let Ok(label) = parent.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, text).is_ok()
        && label
            .set_text_font(token, font, StyleState::Default)
            .is_ok()
        && label.set_pos(token, x, y).is_ok()
}

fn create_result_panel(parent: &Widget, token: &UiAccessToken) -> Option<Widget> {
    let panel = parent.child(token, WidgetKind::Container).ok()?;
    let built = panel.set_size(token, 516, 290).is_ok()
        && panel.set_pos(token, 42, 210).is_ok()
        && panel
            .set_bg_color(token, render::lvgl_adapter::white(), StyleState::Default)
            .is_ok()
        && panel.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && panel
            .set_border_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && panel
            .set_border_width(token, 3, StyleState::Default)
            .is_ok()
        && panel.set_radius(token, 8, StyleState::Default).is_ok();
    built.then_some(panel)
}

fn create_result_label(parent: &Widget, token: &UiAccessToken) -> Option<Widget> {
    let label = parent.child(token, WidgetKind::Label).ok()?;
    let built = label.set_text(token, c"No gesture detected yet.").is_ok()
        && label
            .set_text_font(token, Font::Size20, StyleState::Default)
            .is_ok()
        && label.set_width(token, 460).is_ok()
        && label.set_pos(token, 22, 24).is_ok();
    built.then_some(label)
}

fn discard_incomplete_screen(screen: Widget, token: &UiAccessToken) {
    if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
        render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
    }
}

fn create_overlay_demo_button(
    parent: &Widget,
    token: &UiAccessToken,
    lease: &CallbackLease,
) -> bool {
    let Ok(button) = parent.child(token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(token).is_ok()
        && button.set_size(token, 190, 64).is_ok()
        && button.set_pos(token, 292, 202).is_ok()
        && button
            .set_bg_color(token, white, StyleState::Default)
            .is_ok()
        && button.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && button
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_color(token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_width(token, 3, StyleState::Default)
            .is_ok()
        && button.set_radius(token, 8, StyleState::Default).is_ok()
        && button
            .set_bg_color(token, black, StyleState::Pressed)
            .is_ok()
        && button
            .set_text_color(token, white, StyleState::Pressed)
            .is_ok()
        && intent_bridge::bind_click(&button, token, RouteCallback::ShowConfirm, lease).is_ok();
    if !built {
        return false;
    }

    let Ok(label) = button.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, c"Overlay demo").is_ok()
        && label
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && label.center(token).is_ok()
}

const fn direction_label(direction: LvglGestureDirection) -> &'static str {
    match direction {
        LvglGestureDirection::Left => "left",
        LvglGestureDirection::Right => "right",
        LvglGestureDirection::Up => "up",
        LvglGestureDirection::Down => "down",
        LvglGestureDirection::Unknown => "unknown",
    }
}
