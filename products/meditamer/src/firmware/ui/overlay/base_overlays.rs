#![forbid(unsafe_code)]

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};
#[cfg(feature = "ui-provider-fixture")]
use shell::types::ProviderToken;
use shell::{
    lifecycle::DestroyFailure,
    types::{
        CompositionIntent, OverlayInput, OverlayInstance, OwnedCompositionIntent,
        SurfaceInstanceToken,
    },
};

use super::clock::ClockOverlay;
use super::frontlight_calibration::{FrontlightCalibration, FrontlightCalibrationEffect};
use super::refresh_control::RefreshControl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::firmware::ui) enum BaseOverlayKind {
    NavigationCue,
    RefreshControl,
}

impl BaseOverlayKind {
    pub(in crate::firmware::ui) const fn input(self) -> OverlayInput {
        match self {
            Self::NavigationCue => OverlayInput::Passive,
            Self::RefreshControl => OverlayInput::Interactive,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::firmware::ui) enum OverlayEnterError {
    CallbackRoute,
    ObjectCreation,
}

pub(in crate::firmware::ui) enum ActiveOverlay {
    Passive(PassiveOverlay),
    Refresh(RefreshControl),
    Clock(ClockOverlay),
    Confirm(ConfirmModal),
    Settings(SettingsPanel),
    FrontlightCalibration(FrontlightCalibration),
}

impl ActiveOverlay {
    pub(in crate::firmware::ui) fn base(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
        kind: BaseOverlayKind,
    ) -> Result<Self, OverlayEnterError> {
        match kind {
            BaseOverlayKind::NavigationCue => {
                PassiveOverlay::create(ui_token, instance).map(Self::Passive)
            }
            BaseOverlayKind::RefreshControl => {
                RefreshControl::create(ui_token, instance).map(Self::Refresh)
            }
        }
    }

    pub(in crate::firmware::ui) fn confirm(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        ConfirmModal::create(ui_token, instance).map(Self::Confirm)
    }

    pub(in crate::firmware::ui) fn clock(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
        launcher: shell::types::SurfaceRef,
    ) -> Result<Self, OverlayEnterError> {
        ClockOverlay::create(ui_token, instance, launcher).map(Self::Clock)
    }

    pub(in crate::firmware::ui) fn settings(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        SettingsPanel::create(ui_token, instance).map(Self::Settings)
    }

    pub(in crate::firmware::ui) fn frontlight_calibration(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
        initial_level: u8,
    ) -> Result<Self, OverlayEnterError> {
        FrontlightCalibration::create(ui_token, instance, initial_level)
            .map(Self::FrontlightCalibration)
    }

    pub(in crate::firmware::ui) fn token(&self) -> SurfaceInstanceToken {
        self.instance().token
    }

    pub(in crate::firmware::ui) fn instance(&self) -> OverlayInstance {
        match self {
            Self::Passive(overlay) => overlay.instance,
            Self::Refresh(overlay) => overlay.instance(),
            Self::Clock(overlay) => overlay.instance(),
            Self::Confirm(overlay) => overlay.instance,
            Self::Settings(overlay) => overlay.instance,
            Self::FrontlightCalibration(overlay) => overlay.instance(),
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(in crate::firmware::ui) fn references_provider(&self, owner: ProviderToken) -> bool {
        let instance = self.instance();
        instance.token.surface.owner == owner || instance.request_owner == owner
    }

    pub(in crate::firmware::ui) fn is_modal(&self) -> bool {
        self.instance().input == OverlayInput::Modal
    }

    pub(in crate::firmware::ui) fn enable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        self.callbacks().map_or(Ok(()), intent_bridge::enable)
    }

    pub(in crate::firmware::ui) fn disable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        self.callbacks().map_or(Ok(()), intent_bridge::disable)
    }

    pub(in crate::firmware::ui) fn show(&self, ui_token: &UiAccessToken) {
        let _ = self.root().set_hidden(ui_token, false);
    }

    pub(in crate::firmware::ui) fn hide(&self, ui_token: &UiAccessToken) {
        let _ = self.root().set_hidden(ui_token, true);
    }

    pub(in crate::firmware::ui) fn destroy(
        self,
        ui_token: &UiAccessToken,
    ) -> Result<(), DestroyFailure<Self>> {
        if self.disable().is_err() {
            return Err(DestroyFailure::Live(self));
        }
        intent_bridge::purge_instance(self.token());
        let callbacks = match self {
            Self::Passive(overlay) => {
                return overlay
                    .destroy_root(ui_token)
                    .map_err(|overlay| DestroyFailure::Live(Self::Passive(overlay)));
            }
            Self::Refresh(overlay) => overlay
                .destroy_root(ui_token)
                .map_err(|overlay| DestroyFailure::Live(Self::Refresh(overlay)))?,
            Self::Clock(overlay) => overlay
                .destroy_root(ui_token)
                .map_err(|overlay| DestroyFailure::Live(Self::Clock(overlay)))?,
            Self::Confirm(overlay) => overlay
                .destroy_root(ui_token)
                .map_err(|overlay| DestroyFailure::Live(Self::Confirm(overlay)))?,
            Self::Settings(overlay) => overlay
                .destroy_root(ui_token)
                .map_err(|overlay| DestroyFailure::Live(Self::Settings(overlay)))?,
            Self::FrontlightCalibration(overlay) => overlay
                .destroy_root(ui_token)
                .map_err(|overlay| DestroyFailure::Live(Self::FrontlightCalibration(overlay)))?,
        };
        if intent_bridge::release(&callbacks).is_err() {
            return Err(DestroyFailure::Audit);
        }
        Ok(())
    }

    fn callbacks(&self) -> Option<&intent_bridge::CallbackLease> {
        match self {
            Self::Passive(_) => None,
            Self::Refresh(overlay) => Some(overlay.callbacks()),
            Self::Clock(overlay) => Some(overlay.callbacks()),
            Self::Confirm(overlay) => Some(&overlay.callbacks),
            Self::Settings(overlay) => Some(&overlay.callbacks),
            Self::FrontlightCalibration(overlay) => Some(overlay.callbacks()),
        }
    }

    fn root(&self) -> &Widget {
        match self {
            Self::Passive(overlay) => &overlay.root,
            Self::Refresh(overlay) => overlay.root_widget(),
            Self::Clock(overlay) => overlay.root(),
            Self::Confirm(overlay) => &overlay.root,
            Self::Settings(overlay) => &overlay.root,
            Self::FrontlightCalibration(overlay) => overlay.root(),
        }
    }

    pub(in crate::firmware::ui) fn clock_deadline_ms(&self) -> Option<u64> {
        if let Self::Clock(overlay) = self {
            overlay.timeout_deadline_ms()
        } else {
            None
        }
    }

    pub(in crate::firmware::ui) fn clock_timeout_elapsed(&self, now_ms: u64) -> bool {
        matches!(self, Self::Clock(overlay) if overlay.timeout_elapsed(now_ms))
    }

    pub(in crate::firmware::ui) fn update_clock(
        &mut self,
        ui_token: &UiAccessToken,
        local_epoch_seconds: u32,
        now_ms: u64,
    ) -> Option<Result<bool, ()>> {
        match self {
            Self::Clock(overlay) => {
                Some(overlay.update_time(ui_token, local_epoch_seconds, now_ms))
            }
            Self::Passive(_)
            | Self::Refresh(_)
            | Self::Confirm(_)
            | Self::Settings(_)
            | Self::FrontlightCalibration(_) => None,
        }
    }

    pub(in crate::firmware::ui) fn apply_frontlight_action(
        &mut self,
        index: usize,
    ) -> Option<FrontlightCalibrationEffect> {
        match self {
            Self::FrontlightCalibration(overlay) => overlay.apply_action(index),
            Self::Passive(_)
            | Self::Refresh(_)
            | Self::Clock(_)
            | Self::Confirm(_)
            | Self::Settings(_) => None,
        }
    }

    pub(in crate::firmware::ui) fn apply_frontlight_value(
        &mut self,
        ui_token: &UiAccessToken,
        action: intent_bridge::IndexedValueAction,
    ) -> Option<FrontlightCalibrationEffect> {
        match self {
            Self::FrontlightCalibration(overlay) => overlay.apply_value(ui_token, action),
            Self::Passive(_)
            | Self::Refresh(_)
            | Self::Clock(_)
            | Self::Confirm(_)
            | Self::Settings(_) => None,
        }
    }
}

pub(in crate::firmware::ui) struct PassiveOverlay {
    instance: OverlayInstance,
    root: Widget,
}

impl PassiveOverlay {
    fn create(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Passive {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let root = Widget::on_system_layer(ui_token, WidgetKind::Container)
            .map_err(|_| OverlayEnterError::ObjectCreation)?;
        if root.set_hidden(ui_token, true).is_err() || root.remove_style_all(ui_token).is_err() {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            return Err(OverlayEnterError::ObjectCreation);
        }

        let label = match build_navigation_cue(&root, ui_token) {
            Some(label) => label,
            None => {
                if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                    render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
                }
                return Err(OverlayEnterError::ObjectCreation);
            }
        };
        // Every widget this overlay ever builds must be made -- and stay --
        // non-interactive: a passive overlay must never intercept a touch or
        // claim modal capture. Checked against `is_interactive`'s read-back
        // rather than assumed, since a flag-removal call returning `Ok` only
        // means the checked-handle contract accepted it, not that LVGL's
        // flags ended up the way this overlay actually needs.
        let made_passive = root.set_non_interactive(ui_token).is_ok()
            && !root.is_interactive(ui_token).unwrap_or(true)
            && label.set_non_interactive(ui_token).is_ok()
            && !label.is_interactive(ui_token).unwrap_or(true);
        if !made_passive {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            return Err(OverlayEnterError::ObjectCreation);
        }
        Ok(Self { instance, root })
    }

    fn destroy_root(self, ui_token: &UiAccessToken) -> Result<(), Self> {
        let Self { instance, root } = self;
        match root.delete(ui_token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { instance, root }),
        }
    }
}

pub(in crate::firmware::ui) struct ConfirmModal {
    instance: OverlayInstance,
    root: Widget,
    callbacks: intent_bridge::CallbackLease,
}

impl ConfirmModal {
    fn create(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Modal {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Modal {
            navigation: None,
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
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        if !build_confirm(&root, ui_token, &callbacks) {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        Ok(Self {
            instance,
            root,
            callbacks,
        })
    }

    fn destroy_root(self, ui_token: &UiAccessToken) -> Result<intent_bridge::CallbackLease, Self> {
        let Self {
            instance,
            root,
            callbacks,
        } = self;
        match root.delete(ui_token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(callbacks),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                instance,
                root,
                callbacks,
            }),
        }
    }
}

/// Minimal Settings overlay: proves the overlay-over-a-still-live-Home
/// lifecycle pattern. Deliberately not the existing `OverlayToggles`
/// screen's content (that stays a full-panel, Launcher-reached surface,
/// untouched) -- just a title and a close button, built the same way
/// `ConfirmModal` is: a modal `lv_layer_sys()` child, claimed through
/// `IntentBindings::Modal`, so dismissing it is `CompositionIntent::
/// DismissActiveModal` like every other modal overlay.
pub(in crate::firmware::ui) struct SettingsPanel {
    instance: OverlayInstance,
    root: Widget,
    callbacks: intent_bridge::CallbackLease,
}

impl SettingsPanel {
    fn create(
        ui_token: &UiAccessToken,
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Modal {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Modal {
            navigation: None,
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
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        if !build_settings(&root, ui_token, &callbacks) {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(ui_token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        Ok(Self {
            instance,
            root,
            callbacks,
        })
    }

    fn destroy_root(self, ui_token: &UiAccessToken) -> Result<intent_bridge::CallbackLease, Self> {
        let Self {
            instance,
            root,
            callbacks,
        } = self;
        match root.delete(ui_token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(callbacks),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                instance,
                root,
                callbacks,
            }),
        }
    }
}

/// Builds the navigation cue's one label and returns it, so the caller can
/// fold it into the "must end up non-interactive" check alongside `root`
/// itself -- this overlay's only child, so no recursive tree walk is needed
/// the way the pre-adapter version's `passive_tree_is_valid` required.
fn build_navigation_cue(root: &Widget, ui_token: &UiAccessToken) -> Option<Widget> {
    let black = render::lvgl_adapter::black();
    let built = root.set_size(ui_token, 180, 64).is_ok()
        && root.set_pos(ui_token, 210, 42).is_ok()
        && root.set_bg_opa(ui_token, 0, StyleState::Default).is_ok()
        && root
            .set_border_color(ui_token, black, StyleState::Default)
            .is_ok()
        && root
            .set_border_width(ui_token, 2, StyleState::Default)
            .is_ok()
        && root.set_radius(ui_token, 8, StyleState::Default).is_ok();
    if !built {
        return None;
    }
    let label = create_label(root, ui_token, c"PASS THROUGH", 24, 6)?;
    Some(label)
}

fn build_confirm(root: &Widget, ui_token: &UiAccessToken, lease: &CallbackLease) -> bool {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = root.set_size(ui_token, 380, 244).is_ok()
        && root.set_pos(ui_token, 110, 178).is_ok()
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
        return false;
    }
    if create_label(root, ui_token, c"Confirm action", 110, 28).is_none()
        || create_label(root, ui_token, c"Modal input is captured here.", 68, 82).is_none()
    {
        return false;
    }
    create_modal_button(root, ui_token, 36, c"Cancel", lease)
        && create_modal_button(root, ui_token, 204, c"Accept", lease)
}

fn build_settings(root: &Widget, ui_token: &UiAccessToken, lease: &CallbackLease) -> bool {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = root.set_size(ui_token, 380, 244).is_ok()
        && root.set_pos(ui_token, 110, 178).is_ok()
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
        return false;
    }
    if create_label(root, ui_token, c"Settings", 110, 28).is_none()
        || create_label(
            root,
            ui_token,
            c"Home stays live behind this panel.",
            44,
            82,
        )
        .is_none()
    {
        return false;
    }
    create_modal_button(root, ui_token, 120, c"Close", lease)
}

fn create_modal_button(
    parent: &Widget,
    ui_token: &UiAccessToken,
    x: i32,
    text: &'static core::ffi::CStr,
    lease: &CallbackLease,
) -> bool {
    let Ok(button) = parent.child(ui_token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(ui_token).is_ok()
        && button.set_size(ui_token, 140, 64).is_ok()
        && button.set_pos(ui_token, x, 150).is_ok()
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
        && intent_bridge::bind_click(&button, ui_token, RouteCallback::DismissModal, lease).is_ok();
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

fn create_label(
    parent: &Widget,
    ui_token: &UiAccessToken,
    text: &'static core::ffi::CStr,
    x: i32,
    y: i32,
) -> Option<Widget> {
    let label = parent.child(ui_token, WidgetKind::Label).ok()?;
    let built = label.set_text(ui_token, text).is_ok()
        && label
            .set_text_font(ui_token, Font::Size14, StyleState::Default)
            .is_ok()
        && label.set_pos(ui_token, x, y).is_ok();
    built.then_some(label)
}
