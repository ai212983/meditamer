use core::ptr;

use lightvgl_sys as lv;

use crate::firmware::ui::lvgl::intent_bridge;
#[cfg(feature = "ui-provider-fixture")]
use crate::firmware::ui::shell::types::ProviderToken;
use crate::firmware::ui::shell::{
    lifecycle::DestroyFailure,
    types::{
        CompositionIntent, OverlayInput, OverlayInstance, OwnedCompositionIntent,
        OwnedRefreshIntent, RefreshIntent, SurfaceInstanceToken,
    },
};

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;

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
    Confirm(ConfirmModal),
}

impl ActiveOverlay {
    pub(in crate::firmware::ui) fn base(
        instance: OverlayInstance,
        kind: BaseOverlayKind,
    ) -> Result<Self, OverlayEnterError> {
        match kind {
            BaseOverlayKind::NavigationCue => unsafe { PassiveOverlay::create(instance) }
                .map(Self::Passive)
                .ok_or(OverlayEnterError::ObjectCreation),
            BaseOverlayKind::RefreshControl => {
                unsafe { RefreshControl::create(instance) }.map(Self::Refresh)
            }
        }
    }

    pub(in crate::firmware::ui) fn confirm(
        instance: OverlayInstance,
    ) -> Result<Self, OverlayEnterError> {
        unsafe { ConfirmModal::create(instance) }.map(Self::Confirm)
    }

    pub(in crate::firmware::ui) fn token(&self) -> SurfaceInstanceToken {
        self.instance().token
    }

    pub(in crate::firmware::ui) fn instance(&self) -> OverlayInstance {
        match self {
            Self::Passive(overlay) => overlay.instance,
            Self::Refresh(overlay) => overlay.instance,
            Self::Confirm(overlay) => overlay.instance,
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

    pub(in crate::firmware::ui) fn show(&self) {
        unsafe { lv::lv_obj_remove_flag(self.root(), lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) };
    }

    pub(in crate::firmware::ui) fn hide(&self) {
        unsafe { lv::lv_obj_add_flag(self.root(), lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) };
    }

    pub(in crate::firmware::ui) fn destroy(self) -> Result<(), DestroyFailure<Self>> {
        if self.disable().is_err() {
            return Err(DestroyFailure::Live(self));
        }
        intent_bridge::purge_instance(self.token());
        let root = self.root();
        unsafe { lv::lv_obj_delete(root) };
        if unsafe { lv::lv_obj_is_valid(root) } {
            return Err(DestroyFailure::Live(self));
        }
        if self
            .callbacks()
            .is_some_and(|callbacks| intent_bridge::release(callbacks).is_err())
        {
            return Err(DestroyFailure::Audit);
        }
        Ok(())
    }

    fn callbacks(&self) -> Option<&intent_bridge::CallbackLease> {
        match self {
            Self::Passive(_) => None,
            Self::Refresh(overlay) => Some(&overlay.callbacks),
            Self::Confirm(overlay) => Some(&overlay.callbacks),
        }
    }

    fn root(&self) -> *mut lv::lv_obj_t {
        match self {
            Self::Passive(overlay) => overlay.root,
            Self::Refresh(overlay) => overlay.root,
            Self::Confirm(overlay) => overlay.root,
        }
    }
}

pub(in crate::firmware::ui) struct PassiveOverlay {
    instance: OverlayInstance,
    root: *mut lv::lv_obj_t,
}

impl PassiveOverlay {
    pub(in crate::firmware::ui) unsafe fn create(instance: OverlayInstance) -> Option<Self> {
        if instance.input != OverlayInput::Passive {
            return None;
        }
        let parent = unsafe { lv::lv_layer_sys() };
        if parent.is_null() {
            return None;
        }
        let root = unsafe { lv::lv_obj_create(parent) };
        if root.is_null() {
            return None;
        }
        unsafe {
            lv::lv_obj_add_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            lv::lv_obj_remove_style_all(root);
        }

        if !unsafe { build_navigation_cue(root) } {
            unsafe { lv::lv_obj_delete(root) };
            return None;
        }
        unsafe { make_passive(root) };
        if !unsafe { passive_tree_is_valid(root) } {
            unsafe { lv::lv_obj_delete(root) };
            return None;
        }
        Some(Self { instance, root })
    }
}

pub(in crate::firmware::ui) struct RefreshControl {
    instance: OverlayInstance,
    root: *mut lv::lv_obj_t,
    callbacks: intent_bridge::CallbackLease,
}

impl RefreshControl {
    unsafe fn create(instance: OverlayInstance) -> Result<Self, OverlayEnterError> {
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
        let parent = unsafe { lv::lv_layer_sys() };
        if parent.is_null() {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        let root = unsafe { lv::lv_button_create(parent) };
        if root.is_null() {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        unsafe {
            lv::lv_obj_add_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            lv::lv_obj_remove_style_all(root);
        }
        if !unsafe { build_refresh_control(root, callbacks.user_data()) } {
            unsafe { lv::lv_obj_delete(root) };
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        if intent_bridge::enable(&callbacks).is_err() {
            unsafe { lv::lv_obj_delete(root) };
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::CallbackRoute);
        }
        Ok(Self {
            instance,
            root,
            callbacks,
        })
    }
}

pub(in crate::firmware::ui) struct ConfirmModal {
    instance: OverlayInstance,
    root: *mut lv::lv_obj_t,
    callbacks: intent_bridge::CallbackLease,
}

impl ConfirmModal {
    unsafe fn create(instance: OverlayInstance) -> Result<Self, OverlayEnterError> {
        if instance.input != OverlayInput::Modal {
            return Err(OverlayEnterError::ObjectCreation);
        }
        let callbacks = intent_bridge::claim(intent_bridge::IntentBindings::Modal {
            dismiss: OwnedCompositionIntent {
                source: instance.token,
                intent: CompositionIntent::DismissActiveModal,
            },
        })
        .map_err(|_| OverlayEnterError::CallbackRoute)?;
        let parent = unsafe { lv::lv_layer_sys() };
        if parent.is_null() {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        let root = unsafe { lv::lv_obj_create(parent) };
        if root.is_null() {
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        unsafe {
            lv::lv_obj_add_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            lv::lv_obj_remove_style_all(root);
        }
        if !unsafe { build_confirm(root, callbacks.user_data()) } {
            unsafe { lv::lv_obj_delete(root) };
            let _ = intent_bridge::release(&callbacks);
            return Err(OverlayEnterError::ObjectCreation);
        }
        Ok(Self {
            instance,
            root,
            callbacks,
        })
    }
}

unsafe fn build_navigation_cue(root: *mut lv::lv_obj_t) -> bool {
    let black = unsafe { lv::lv_color_black() };
    unsafe {
        lv::lv_obj_set_size(root, 180, 64);
        lv::lv_obj_set_pos(root, 210, 42);
        lv::lv_obj_set_style_bg_opa(root, 0, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(root, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(root, 2, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(root, 8, STYLE_DEFAULT);
    }
    unsafe { create_label(root, c"PASS THROUGH".as_ptr(), 24, 6) }
}

unsafe fn build_refresh_control(
    root: *mut lv::lv_obj_t,
    user_data: *mut core::ffi::c_void,
) -> bool {
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_set_size(root, 112, 38);
        lv::lv_obj_set_pos(root, 474, 12);
        lv::lv_obj_set_style_bg_color(root, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(root, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(root, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(root, 2, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(root, 6, STYLE_DEFAULT);
        if lv::lv_obj_add_event_cb(
            root,
            Some(intent_bridge::full_repaint_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        )
        .is_null()
        {
            return false;
        }
    }
    unsafe { create_label(root, c"STICKY".as_ptr(), 22, 8) }
}

unsafe fn build_confirm(root: *mut lv::lv_obj_t, user_data: *mut core::ffi::c_void) -> bool {
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_set_size(root, 380, 244);
        lv::lv_obj_set_pos(root, 110, 178);
        lv::lv_obj_set_style_bg_color(root, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(root, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(root, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(root, 4, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(root, 10, STYLE_DEFAULT);
    }
    if !unsafe { create_label(root, c"Confirm action".as_ptr(), 110, 28) }
        || !unsafe { create_label(root, c"Modal input is captured here.".as_ptr(), 68, 82) }
    {
        return false;
    }
    unsafe {
        create_modal_button(root, 36, c"Cancel".as_ptr(), user_data)
            && create_modal_button(root, 204, c"Accept".as_ptr(), user_data)
    }
}

unsafe fn create_modal_button(
    parent: *mut lv::lv_obj_t,
    x: i32,
    text: *const core::ffi::c_char,
    user_data: *mut core::ffi::c_void,
) -> bool {
    let button = unsafe { lv::lv_button_create(parent) };
    if button.is_null() {
        return false;
    }
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 140, 64);
        lv::lv_obj_set_pos(button, x, 150);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, lv::lv_state_t_LV_STATE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, lv::lv_state_t_LV_STATE_PRESSED);
        if lv::lv_obj_add_event_cb(
            button,
            Some(intent_bridge::dismiss_modal_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        )
        .is_null()
        {
            return false;
        }
    }
    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, text);
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    true
}

unsafe fn create_label(
    parent: *mut lv::lv_obj_t,
    text: *const core::ffi::c_char,
    x: i32,
    y: i32,
) -> bool {
    let label = unsafe { lv::lv_label_create(parent) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, text);
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_14),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(label, x, y);
    }
    true
}

unsafe fn make_passive(root: *mut lv::lv_obj_t) {
    unsafe { clear_input_flags(root) };
    let count = unsafe { lv::lv_obj_get_child_count(root) };
    for index in 0..count {
        let child = unsafe { lv::lv_obj_get_child(root, index as i32) };
        if !child.is_null() {
            unsafe { make_passive(child) };
        }
    }
}

unsafe fn clear_input_flags(object: *mut lv::lv_obj_t) {
    unsafe {
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE);
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE);
    }
}

unsafe fn passive_tree_is_valid(root: *mut lv::lv_obj_t) -> bool {
    if unsafe {
        lv::lv_obj_has_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE)
            || lv::lv_obj_has_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE)
            || lv::lv_obj_has_flag(root, lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE)
    } {
        return false;
    }
    let count = unsafe { lv::lv_obj_get_child_count(root) };
    for index in 0..count {
        let child = unsafe { lv::lv_obj_get_child(root, index as i32) };
        if !child.is_null() && !unsafe { passive_tree_is_valid(child) } {
            return false;
        }
    }
    true
}
