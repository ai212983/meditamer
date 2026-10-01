//! Typed gateway for LVGL's raw object and runtime API.
//!
//! This is the only production module in the checked adapter that may call
//! LVGL's unsafe C entry points or hold an `lv_obj_t` pointer. Callers first
//! validate a [`super::UiAccessToken`] and, where applicable, a registered
//! widget generation; they then pass the private [`UiCapability`] and
//! [`Object`] values below. The gateway never accepts a public raw pointer or
//! asks its caller to uphold an unsafe precondition.

#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use core::{ffi::CStr, marker::PhantomData, mem::MaybeUninit, ptr::NonNull};

use lightvgl_sys as lv;

#[cfg(feature = "lvgl-gestures")]
use super::runtime_io::{GestureDirection, GestureEvent, PointerSampleKind, PointerState};
use super::{
    retained::{StaticL8CanvasBuffer, StaticLinePoints},
    runtime::LvglMemorySnapshot,
    Align, Font, StyleState, TextAlign, UiAccessToken, WidgetKind,
};
use crate::DirtyArea;

#[cfg(feature = "lvgl-gestures")]
mod gestures;
#[cfg(feature = "lvgl-gestures")]
use gestures::{delete_input, gesture_callback, input_callback};

static FLUSH_ACTIVE: AtomicBool = AtomicBool::new(false);
static FLUSH_RUNTIME_ID: AtomicU32 = AtomicU32::new(0);
static FLUSH_TARGET: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static FLUSH_TARGET_LEN: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "lvgl-gestures")]
static ACTIVE_INPUT: AtomicPtr<lv::lv_indev_t> = AtomicPtr::new(core::ptr::null_mut());
#[cfg(feature = "lvgl-gestures")]
static ACTIVE_INPUT_RUNTIME_ID: AtomicU32 = AtomicU32::new(0);

/// Proof that the facade validated the current UI runtime epoch.
pub(super) struct UiCapability<'a> {
    _token: &'a UiAccessToken,
}

impl<'a> UiCapability<'a> {
    pub(super) fn after_validation(token: &'a UiAccessToken) -> Self {
        Self { _token: token }
    }
}

/// A non-null LVGL object identity confined to this gateway and its facade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Object {
    ptr: NonNull<lv::lv_obj_t>,
    _not_send_or_sync: PhantomData<*const ()>,
}

#[derive(Clone, Copy)]
pub(super) struct Display(NonNull<lv::lv_display_t>);

#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
pub(super) struct Input(NonNull<lv::lv_indev_t>);

impl Object {
    fn from_ptr(ptr: *mut lv::lv_obj_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self {
            ptr,
            _not_send_or_sync: PhantomData,
        })
    }

    /// Restores an object identity previously parked by the checked facade.
    pub(super) fn from_registered_addr(addr: usize) -> Self {
        Self::from_ptr(core::ptr::with_exposed_provenance_mut(addr))
            .expect("registered LVGL object address is non-null")
    }

    pub(super) fn addr(self) -> usize {
        // Parked checked handles reconstruct this pointer with
        // `with_exposed_provenance_mut`; expose provenance here rather than
        // extracting a provenance-free address with `addr()`.
        self.ptr.as_ptr().expose_provenance()
    }
}

pub(super) fn is_initialized() -> bool {
    // SAFETY: LVGL documents this process-global query before initialization
    // and after deinitialization; it dereferences no caller-provided pointer.
    unsafe { lv::lv_is_initialized() }
}

pub(super) fn init() {
    // SAFETY: RuntimeSession's process-global claim admits one initialization.
    unsafe { lv::lv_init() };
}

pub(super) fn deinit() {
    // SAFETY: called by the sole owning RuntimeSession after publications are
    // cleared and its checked epoch has been invalidated.
    unsafe { lv::lv_deinit() };
}

pub(super) fn memory_snapshot(_: &UiCapability<'_>) -> LvglMemorySnapshot {
    let mut monitor = MaybeUninit::<lv::lv_mem_monitor_t>::zeroed();
    // SAFETY: the capability proves a live, exclusively driven runtime, and
    // the output points to writable storage for the duration of the call.
    unsafe { lv::lv_mem_monitor(monitor.as_mut_ptr()) };
    // SAFETY: lv_mem_monitor initializes every field of lv_mem_monitor_t.
    let monitor = unsafe { monitor.assume_init() };
    // SAFETY: same live-runtime capability; this query takes no raw input.
    let integrity_ok = unsafe { lv::lv_mem_test() == lv::lv_result_t_LV_RESULT_OK };
    LvglMemorySnapshot {
        total_size: monitor.total_size,
        free_count: monitor.free_cnt,
        free_size: monitor.free_size,
        largest_free_size: monitor.free_biggest_size,
        used_count: monitor.used_cnt,
        max_used: monitor.max_used,
        used_percent: monitor.used_pct,
        fragmentation_percent: monitor.frag_pct,
        integrity_ok,
    }
}

pub(super) fn black() -> lv::lv_color_t {
    // SAFETY: Pure LVGL color constructor with no pointer or runtime state.
    unsafe { lv::lv_color_black() }
}

pub(super) fn white() -> lv::lv_color_t {
    // SAFETY: Pure LVGL color constructor with no pointer or runtime state.
    unsafe { lv::lv_color_white() }
}

pub(super) fn create_display(_: &UiCapability<'_>, width: i32, height: i32) -> Option<Display> {
    // SAFETY: dimensions were validated by the facade and the capability
    // serializes this call with all other LVGL runtime access.
    NonNull::new(unsafe { lv::lv_display_create(width, height) }).map(Display)
}

pub(super) fn create_l8_display(
    capability: &UiCapability<'_>,
    width: i32,
    height: i32,
    draw_buffer: *mut core::ffi::c_void,
    buffer_bytes: u32,
) -> Option<Display> {
    let display = create_display(capability, width, height)?;
    // SAFETY: the facade requires a static, u32-aligned buffer at least
    // buffer_bytes long. The display and runtime are current and exclusive.
    unsafe {
        lv::lv_display_set_color_format(
            display.0.as_ptr(),
            lv::lv_color_format_t_LV_COLOR_FORMAT_L8,
        );
        lv::lv_display_set_buffers(
            display.0.as_ptr(),
            draw_buffer,
            core::ptr::null_mut(),
            buffer_bytes,
            lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        lv::lv_display_set_flush_cb(display.0.as_ptr(), Some(flush_callback));
    }
    Some(display)
}

pub(super) fn set_default_display(_: &UiCapability<'_>, display: Display) {
    // SAFETY: display was created in this validated runtime epoch.
    unsafe { lv::lv_display_set_default(display.0.as_ptr()) };
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn create_pointer_input(
    _: &UiCapability<'_>,
    runtime_id: u32,
    rotation_threshold_radians: f32,
) -> Option<Input> {
    // SAFETY: the capability serializes creation and all configuration calls.
    let input = NonNull::new(unsafe { lv::lv_indev_create() }).map(Input)?;
    unsafe {
        lv::lv_indev_set_type(input.0.as_ptr(), lv::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
        lv::lv_indev_set_read_cb(input.0.as_ptr(), Some(input_callback));
        lv::lv_indev_set_rotation_rad_threshold(input.0.as_ptr(), rotation_threshold_radians);
        lv::lv_indev_add_event_cb(
            input.0.as_ptr(),
            Some(gesture_callback),
            lv::lv_event_code_t_LV_EVENT_GESTURE,
            core::ptr::null_mut(),
        );
    }
    if ACTIVE_INPUT
        .compare_exchange(
            core::ptr::null_mut(),
            input.0.as_ptr(),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        delete_input(input.0.as_ptr());
        return None;
    }
    ACTIVE_INPUT_RUNTIME_ID.store(runtime_id, Ordering::Release);
    Some(input)
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn pointer_input_is_registered(runtime_id: u32, input: Input) -> bool {
    ACTIVE_INPUT_RUNTIME_ID.load(Ordering::Acquire) == runtime_id
        && ACTIVE_INPUT.load(Ordering::Acquire) == input.0.as_ptr()
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn read_pointer_input(_: &UiCapability<'_>, input: Input) {
    // SAFETY: input belongs to this runtime and the UI owner serializes reads.
    unsafe { lv::lv_indev_read(input.0.as_ptr()) };
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn reset_pointer_input(_: &UiCapability<'_>, input: Input) {
    // SAFETY: input belongs to this runtime. A null object resets this input's
    // state unconditionally without applying the reset to every input.
    unsafe { lv::lv_indev_reset(input.0.as_ptr(), core::ptr::null_mut()) };
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn delete_pointer_input(runtime_id: u32, input: Input) -> bool {
    if ACTIVE_INPUT_RUNTIME_ID.load(Ordering::Acquire) != runtime_id
        || ACTIVE_INPUT
            .compare_exchange(
                input.0.as_ptr(),
                core::ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
    {
        return false;
    }
    ACTIVE_INPUT_RUNTIME_ID.store(0, Ordering::Release);
    delete_input(input.0.as_ptr());
    true
}

pub(super) fn delete_active_pointer_input(runtime_id: u32) {
    #[cfg(not(feature = "lvgl-gestures"))]
    let _ = runtime_id;
    #[cfg(feature = "lvgl-gestures")]
    if ACTIVE_INPUT_RUNTIME_ID.load(Ordering::Acquire) == runtime_id {
        let input = ACTIVE_INPUT.swap(core::ptr::null_mut(), Ordering::AcqRel);
        ACTIVE_INPUT_RUNTIME_ID.store(0, Ordering::Release);
        if !input.is_null() {
            delete_input(input);
        }
    }
}

pub(super) fn publish_flush_target(
    _: &UiCapability<'_>,
    runtime_id: u32,
    framebuffer: &mut [u8],
) -> bool {
    if FLUSH_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    FLUSH_TARGET_LEN.store(framebuffer.len(), Ordering::Relaxed);
    FLUSH_TARGET.store(framebuffer.as_mut_ptr(), Ordering::Release);
    FLUSH_RUNTIME_ID.store(runtime_id, Ordering::Release);
    true
}

pub(super) fn clear_flush_target(runtime_id: u32) -> bool {
    if FLUSH_RUNTIME_ID.load(Ordering::Acquire) != runtime_id
        || FLUSH_ACTIVE
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return false;
    }
    FLUSH_TARGET.store(core::ptr::null_mut(), Ordering::Release);
    FLUSH_TARGET_LEN.store(0, Ordering::Relaxed);
    FLUSH_RUNTIME_ID.store(0, Ordering::Release);
    true
}

fn inclusive_extent(start: i32, end: i32) -> Option<usize> {
    let extent = end.checked_sub(start)?.checked_add(1)?;
    usize::try_from(extent).ok()
}

fn flush_pixel_len(area: DirtyArea) -> Option<usize> {
    let width = inclusive_extent(area.x1, area.x2)?;
    let height = inclusive_extent(area.y1, area.y2)?;
    let len = width.checked_mul(height)?;
    if len > isize::MAX as usize {
        None
    } else {
        Some(len)
    }
}

unsafe extern "C" fn flush_callback(
    display: *mut lv::lv_display_t,
    area: *const lv::lv_area_t,
    pixels: *mut u8,
) {
    let framebuffer = FLUSH_TARGET.load(Ordering::Acquire);
    let runtime_id = FLUSH_RUNTIME_ID.load(Ordering::Acquire);
    if !framebuffer.is_null() && !area.is_null() && !pixels.is_null() {
        // SAFETY: LVGL provides a valid area for this synchronous callback.
        let raw_area = unsafe { *area };
        let area = DirtyArea {
            x1: raw_area.x1,
            y1: raw_area.y1,
            x2: raw_area.x2,
            y2: raw_area.y2,
        };
        if let Some(pixel_len) = flush_pixel_len(area) {
            // SAFETY: LVGL's L8 flush contract supplies one tightly packed
            // byte per pixel for the callback area.
            let pixels = unsafe { core::slice::from_raw_parts(pixels.cast_const(), pixel_len) };
            // SAFETY: FlushFrame retains the exclusive slice borrow and
            // clears this publication before that borrow can expire.
            let framebuffer = unsafe {
                core::slice::from_raw_parts_mut(
                    framebuffer,
                    FLUSH_TARGET_LEN.load(Ordering::Relaxed),
                )
            };
            super::runtime_io::blit_flush(runtime_id, area, pixels, framebuffer);
        }
    }
    // SAFETY: LVGL requires every flush callback invocation to be completed.
    unsafe { lv::lv_display_flush_ready(display) };
}

fn create(parent: *mut lv::lv_obj_t, kind: WidgetKind) -> Option<Object> {
    // SAFETY: the parent is null for a screen or came from a validated Object
    // or the current system layer. The UI capability at each public gateway
    // entry serializes this call with all other LVGL access.
    let object = unsafe {
        match kind {
            WidgetKind::Container => lv::lv_obj_create(parent),
            WidgetKind::Button => lv::lv_button_create(parent),
            WidgetKind::Label => lv::lv_label_create(parent),
            WidgetKind::Canvas => lv::lv_canvas_create(parent),
            WidgetKind::Line => lv::lv_line_create(parent),
            WidgetKind::Slider => lv::lv_slider_create(parent),
        }
    };
    #[cfg(feature = "ui-interaction-trace")]
    if !object.is_null() {
        // Diagnostic event observation uses LVGL's existing callback mechanism.
        let callback = unsafe {
            lv::lv_obj_add_event_cb(
                object,
                Some(trace_input_event),
                lv::lv_event_code_t_LV_EVENT_ALL,
                core::ptr::null_mut(),
            )
        };
        if callback.is_null() {
            crate::interaction_trace::hook_failed();
        }
    }
    Object::from_ptr(object)
}

pub(super) fn create_screen(_: &UiCapability<'_>, kind: WidgetKind) -> Option<Object> {
    create(core::ptr::null_mut(), kind)
}

pub(super) fn create_child(
    _: &UiCapability<'_>,
    parent: Object,
    kind: WidgetKind,
) -> Option<Object> {
    create(parent.ptr.as_ptr(), kind)
}

pub(super) fn create_on_system_layer(_: &UiCapability<'_>, kind: WidgetKind) -> Option<Object> {
    // SAFETY: the current-runtime capability serializes access. A null layer
    // is handled without passing it on as a parent capability.
    Object::from_ptr(unsafe { lv::lv_layer_sys() })
        .and_then(|parent| create(parent.ptr.as_ptr(), kind))
}

pub(super) fn has_parent(_: &UiCapability<'_>, object: Object) -> bool {
    // SAFETY: object was validated by the facade for this runtime epoch.
    !unsafe { lv::lv_obj_get_parent(object.ptr.as_ptr()) }.is_null()
}

pub(super) fn set_pos(_: &UiCapability<'_>, object: Object, x: i32, y: i32) {
    unsafe { lv::lv_obj_set_pos(object.ptr.as_ptr(), x, y) };
}

pub(super) fn set_size(_: &UiCapability<'_>, object: Object, width: i32, height: i32) {
    unsafe { lv::lv_obj_set_size(object.ptr.as_ptr(), width, height) };
}

pub(super) fn set_width(_: &UiCapability<'_>, object: Object, width: i32) {
    unsafe { lv::lv_obj_set_width(object.ptr.as_ptr(), width) };
}

pub(super) fn set_ext_click_area(_: &UiCapability<'_>, object: Object, padding: i32) {
    unsafe { lv::lv_obj_set_ext_click_area(object.ptr.as_ptr(), padding) };
}

pub(super) fn remove_style_all(_: &UiCapability<'_>, object: Object) {
    unsafe { lv::lv_obj_remove_style_all(object.ptr.as_ptr()) };
}

pub(super) fn set_bg_color(
    _: &UiCapability<'_>,
    object: Object,
    color: lv::lv_color_t,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_bg_color(object.ptr.as_ptr(), color, state.selector()) };
}

pub(super) fn set_bg_opa(_: &UiCapability<'_>, object: Object, opacity: u8, state: StyleState) {
    unsafe { lv::lv_obj_set_style_bg_opa(object.ptr.as_ptr(), opacity, state.selector()) };
}

pub(super) fn set_text_color(
    _: &UiCapability<'_>,
    object: Object,
    color: lv::lv_color_t,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_text_color(object.ptr.as_ptr(), color, state.selector()) };
}

pub(super) fn set_border_color(
    _: &UiCapability<'_>,
    object: Object,
    color: lv::lv_color_t,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_border_color(object.ptr.as_ptr(), color, state.selector()) };
}

pub(super) fn set_border_width(
    _: &UiCapability<'_>,
    object: Object,
    width: i32,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_border_width(object.ptr.as_ptr(), width, state.selector()) };
}

pub(super) fn set_radius(_: &UiCapability<'_>, object: Object, radius: i32, state: StyleState) {
    unsafe { lv::lv_obj_set_style_radius(object.ptr.as_ptr(), radius, state.selector()) };
}

pub(super) fn set_text_font(_: &UiCapability<'_>, object: Object, font: Font, state: StyleState) {
    let font = match font {
        Font::Size14 => core::ptr::addr_of!(lv::lv_font_montserrat_14),
        Font::Size18 => core::ptr::addr_of!(lv::lv_font_montserrat_18),
        #[cfg(feature = "font-20")]
        Font::Size20 => core::ptr::addr_of!(lv::lv_font_montserrat_20),
        Font::Size24 => core::ptr::addr_of!(lv::lv_font_montserrat_24),
        #[cfg(feature = "font-32")]
        Font::Size32 => core::ptr::addr_of!(lv::lv_font_montserrat_32),
        Font::Custom(font) => core::ptr::from_ref(font.get()),
    };
    unsafe { lv::lv_obj_set_style_text_font(object.ptr.as_ptr(), font, state.selector()) };
}

pub(super) fn set_text_align(
    _: &UiCapability<'_>,
    object: Object,
    align: TextAlign,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_text_align(object.ptr.as_ptr(), align.raw(), state.selector()) };
}

pub(super) fn set_line_color(
    _: &UiCapability<'_>,
    object: Object,
    color: lv::lv_color_t,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_line_color(object.ptr.as_ptr(), color, state.selector()) };
}

pub(super) fn set_line_width(_: &UiCapability<'_>, object: Object, width: i32, state: StyleState) {
    unsafe { lv::lv_obj_set_style_line_width(object.ptr.as_ptr(), width, state.selector()) };
}

pub(super) fn set_line_rounded(
    _: &UiCapability<'_>,
    object: Object,
    rounded: bool,
    state: StyleState,
) {
    unsafe { lv::lv_obj_set_style_line_rounded(object.ptr.as_ptr(), rounded, state.selector()) };
}

pub(super) fn set_label_text(_: &UiCapability<'_>, object: Object, text: &CStr) {
    // SAFETY: the checked kind is Label and LVGL copies this NUL-terminated
    // text before returning rather than retaining its address.
    unsafe { lv::lv_label_set_text(object.ptr.as_ptr(), text.as_ptr()) };
}

pub(super) fn center(_: &UiCapability<'_>, object: Object) {
    unsafe { lv::lv_obj_center(object.ptr.as_ptr()) };
}

pub(super) fn invalidate(_: &UiCapability<'_>, object: Object) {
    unsafe { lv::lv_obj_invalidate(object.ptr.as_ptr()) };
}

pub(super) fn set_non_interactive(_: &UiCapability<'_>, object: Object) {
    unsafe {
        lv::lv_obj_remove_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        lv::lv_obj_remove_flag(
            object.ptr.as_ptr(),
            lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE,
        );
        lv::lv_obj_remove_flag(
            object.ptr.as_ptr(),
            lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE,
        );
    }
}

pub(super) fn is_interactive(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe {
        lv::lv_obj_has_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE)
            || lv::lv_obj_has_flag(
                object.ptr.as_ptr(),
                lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE,
            )
            || lv::lv_obj_has_flag(
                object.ptr.as_ptr(),
                lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE,
            )
    }
}

pub(super) fn set_scrollable(_: &UiCapability<'_>, object: Object, scrollable: bool) {
    unsafe {
        lv::lv_obj_set_flag(
            object.ptr.as_ptr(),
            lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE,
            scrollable,
        );
    }
}

pub(super) fn is_scrollable(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe {
        lv::lv_obj_has_flag(
            object.ptr.as_ptr(),
            lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE,
        )
    }
}

pub(super) fn set_clickable(_: &UiCapability<'_>, object: Object, clickable: bool) {
    unsafe {
        if clickable {
            lv::lv_obj_add_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        } else {
            lv::lv_obj_remove_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        }
    }
}

pub(super) fn align(
    _: &UiCapability<'_>,
    object: Object,
    align: Align,
    x_offset: i32,
    y_offset: i32,
) {
    unsafe { lv::lv_obj_align(object.ptr.as_ptr(), align.raw(), x_offset, y_offset) };
}

pub(super) fn attach_l8_canvas<const N: usize>(
    _: &UiCapability<'_>,
    object: Object,
    buffer: &'static StaticL8CanvasBuffer<N>,
    width: i32,
    height: i32,
) {
    // SAFETY: the facade checked Canvas kind, positive geometry, product fit,
    // and static storage. Mutation remains serialized by UiAccessToken.
    unsafe {
        lv::lv_canvas_set_buffer(
            object.ptr.as_ptr(),
            buffer.as_mut_ptr().cast(),
            width,
            height,
            lv::lv_color_format_t_LV_COLOR_FORMAT_L8,
        )
    };
}

pub(super) fn attach_indexed_canvas<const BITS: usize, const N: usize>(
    _: &UiCapability<'_>,
    object: Object,
    buffer: &'static super::StaticIndexedCanvasBuffer<BITS, N>,
) -> Result<(), super::RetainedResourceError> {
    // SAFETY: the facade checked Canvas kind and the current UI owner. The
    // constructor checks dimensions, alignment and palette-inclusive capacity;
    // both pointers remain static. Explicit stride avoids handler-dependent
    // layout, and the descriptor is never marked LVGL-owned/allocated.
    unsafe {
        let result = lv::lv_draw_buf_init(
            buffer.descriptor_ptr(),
            buffer.width() as u32,
            buffer.height() as u32,
            match BITS {
                1 => lv::lv_color_format_t_LV_COLOR_FORMAT_I1,
                2 => lv::lv_color_format_t_LV_COLOR_FORMAT_I2,
                4 => lv::lv_color_format_t_LV_COLOR_FORMAT_I4,
                8 => lv::lv_color_format_t_LV_COLOR_FORMAT_I8,
                _ => unreachable!("constructor checked indexed bit depth"),
            },
            buffer.stride_bytes() as u32,
            buffer.as_mut_ptr().cast(),
            buffer.capacity_bytes() as u32,
        );
        if result != lv::lv_result_t_LV_RESULT_OK {
            return Err(super::RetainedResourceError::InvalidCanvasLayout);
        }
        lv::lv_canvas_set_draw_buf(object.ptr.as_ptr(), buffer.descriptor_ptr());
    }
    Ok(())
}

pub(super) fn attach_line_points<const N: usize>(
    _: &UiCapability<'_>,
    object: Object,
    points: &'static StaticLinePoints<N>,
    count: u32,
) {
    // SAFETY: the facade checked Line kind, non-empty static storage, and the
    // count conversion; LinePoint is repr(C) over LVGL's configured aliases.
    unsafe {
        lv::lv_line_set_points(
            object.ptr.as_ptr(),
            points.as_ptr().cast::<lv::lv_point_precise_t>(),
            count,
        )
    };
}

pub(super) fn set_hidden(_: &UiCapability<'_>, object: Object, hidden: bool) {
    unsafe {
        if hidden {
            lv::lv_obj_add_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        } else {
            lv::lv_obj_remove_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        }
    }
}

pub(super) fn is_hidden(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe { lv::lv_obj_has_flag(object.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) }
}

pub(super) fn set_navigation_index(_: &UiCapability<'_>, object: Object, index: usize) {
    unsafe {
        lv::lv_obj_set_user_data(
            object.ptr.as_ptr(),
            core::ptr::without_provenance_mut(index + 1),
        )
    };
}

pub(super) fn set_slider_range(_: &UiCapability<'_>, object: Object, min: i32, max: i32) {
    // SAFETY: the facade checked Slider kind. LVGL normalizes a reversed
    // range itself and clamps the current value, so no caller range carries
    // an unsafe precondition.
    unsafe { lv::lv_slider_set_range(object.ptr.as_ptr(), min, max) };
}

pub(super) fn set_slider_value(_: &UiCapability<'_>, object: Object, value: i32) {
    // SAFETY: the facade checked Slider kind; LVGL clamps to the stored
    // range. Animation off: the UI owner drives presentation synchronously.
    // `lv_anim_enable_t` is `bool`, so `false` is `LV_ANIM_OFF`.
    unsafe { lv::lv_slider_set_value(object.ptr.as_ptr(), value, false) };
}

pub(super) fn slider_value(_: &UiCapability<'_>, object: Object) -> i32 {
    // SAFETY: the facade checked Slider kind; the getter dereferences no
    // caller-provided pointer.
    unsafe { lv::lv_slider_get_value(object.ptr.as_ptr()) }
}

pub(super) fn slider_range(_: &UiCapability<'_>, object: Object) -> (i32, i32) {
    // SAFETY: the facade checked Slider kind.
    unsafe {
        (
            lv::lv_slider_get_min_value(object.ptr.as_ptr()),
            lv::lv_slider_get_max_value(object.ptr.as_ptr()),
        )
    }
}

pub(super) fn register_value_changed(_: &UiCapability<'_>, object: Object, route: u32) -> bool {
    let user_data = core::ptr::without_provenance_mut(route as usize);
    // SAFETY: callback and payload are a closed typed pairing, exactly as in
    // `register_click`: the value callback receives a
    // `CallbackRoute::encoded` value and reads the slider's own value plus
    // its stamped index back. LVGL owns the event descriptor on success.
    !unsafe {
        lv::lv_obj_add_event_cb(
            object.ptr.as_ptr(),
            Some(crate::intent_bridge::callbacks::value_changed),
            lv::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
            user_data,
        )
    }
    .is_null()
}

pub(super) fn send_value_changed(_: &UiCapability<'_>, object: Object) -> bool {
    // SAFETY: object was validated by the facade for this runtime epoch.
    unsafe {
        lv::lv_obj_send_event(
            object.ptr.as_ptr(),
            lv::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
            core::ptr::null_mut(),
        ) == lv::lv_result_t_LV_RESULT_OK
    }
}

#[derive(Clone, Copy)]
pub(super) enum ClickCallback {
    Navigation,
    ShowConfirm,
    DismissModal,
    FullRepaint,
    AmbientTap,
    Action,
}

pub(super) fn register_click(
    _: &UiCapability<'_>,
    object: Object,
    callback: ClickCallback,
    route: Option<u32>,
) -> bool {
    let callback: lv::lv_event_cb_t = match callback {
        ClickCallback::Navigation => Some(crate::intent_bridge::callbacks::navigation),
        ClickCallback::ShowConfirm => Some(crate::intent_bridge::callbacks::show_confirm),
        ClickCallback::DismissModal => Some(crate::intent_bridge::callbacks::dismiss_modal),
        ClickCallback::FullRepaint => Some(crate::intent_bridge::callbacks::full_repaint),
        ClickCallback::AmbientTap => Some(crate::intent_bridge::callbacks::ambient_tap),
        ClickCallback::Action => Some(crate::intent_bridge::callbacks::action),
    };
    let user_data = route
        .map(|encoded| core::ptr::without_provenance_mut(encoded as usize))
        .unwrap_or(core::ptr::null_mut());
    // SAFETY: callback and payload are a closed typed pairing. Routed
    // callbacks receive CallbackRoute::encoded values; AmbientTap ignores a
    // null payload. LVGL owns the event descriptor returned on success.
    !unsafe {
        lv::lv_obj_add_event_cb(
            object.ptr.as_ptr(),
            callback,
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        )
    }
    .is_null()
}

pub(super) fn send_click(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe {
        lv::lv_obj_send_event(
            object.ptr.as_ptr(),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        ) == lv::lv_result_t_LV_RESULT_OK
    }
}

pub(super) fn activate(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe {
        lv::lv_screen_load(object.ptr.as_ptr());
        lv::lv_screen_active() == object.ptr.as_ptr()
    }
}

pub(super) fn is_active_screen(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe { lv::lv_screen_active() == object.ptr.as_ptr() }
}

/// Deletes an object and reports whether LVGL confirms it is gone.
pub(super) fn delete(_: &UiCapability<'_>, object: Object) -> bool {
    unsafe {
        lv::lv_obj_delete(object.ptr.as_ptr());
        !lv::lv_obj_is_valid(object.ptr.as_ptr())
    }
}

pub(super) fn child_count(_: &UiCapability<'_>, object: Object) -> u32 {
    unsafe { lv::lv_obj_get_child_count(object.ptr.as_ptr()) }
}

pub(super) fn child_at(_: &UiCapability<'_>, object: Object, index: u32) -> Option<Object> {
    Object::from_ptr(unsafe { lv::lv_obj_get_child(object.ptr.as_ptr(), index as i32) })
}

pub(super) fn run_timer_handler(_: &UiCapability<'_>, elapsed_ms: u32) -> u32 {
    unsafe {
        lv::lv_tick_inc(elapsed_ms);
        lv::lv_timer_handler()
    }
}

pub(super) fn refresh_default_display(_: &UiCapability<'_>) -> bool {
    let display = unsafe { lv::lv_display_get_default() };
    if display.is_null() {
        return false;
    }
    unsafe { lv::lv_refr_now(display) };
    true
}

pub(super) fn invalidate_active_screen(_: &UiCapability<'_>) -> bool {
    let Some(screen) = Object::from_ptr(unsafe { lv::lv_screen_active() }) else {
        return false;
    };
    unsafe { lv::lv_obj_invalidate(screen.ptr.as_ptr()) };
    true
}

pub(super) fn set_system_layer_capture(_: &UiCapability<'_>, enabled: bool) {
    let Some(layer) = Object::from_ptr(unsafe { lv::lv_layer_sys() }) else {
        return;
    };
    unsafe {
        if enabled {
            lv::lv_obj_add_flag(layer.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        } else {
            lv::lv_obj_remove_flag(layer.ptr.as_ptr(), lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        }
    }
}

pub(super) fn active_screen(_: &UiCapability<'_>) -> Option<Object> {
    Object::from_ptr(unsafe { lv::lv_screen_active() })
}

#[cfg(feature = "ui-interaction-trace")]
unsafe extern "C" fn trace_input_event(event: *mut lv::lv_event_t) {
    // LVGL owns the live event/object for the callback duration; only numeric IDs escape.
    let code = unsafe { lv::lv_event_get_code(event) };
    let stage = match code {
        lv::lv_event_code_t_LV_EVENT_PRESSED => 20,
        lv::lv_event_code_t_LV_EVENT_RELEASED => 21,
        lv::lv_event_code_t_LV_EVENT_CLICKED => 22,
        lv::lv_event_code_t_LV_EVENT_PRESS_LOST => 23,
        lv::lv_event_code_t_LV_EVENT_SCROLL_BEGIN => 24,
        lv::lv_event_code_t_LV_EVENT_SCROLL_END => 25,
        _ => return,
    };
    let target = unsafe { lv::lv_event_get_target_obj(event) };
    let current = unsafe { lv::lv_event_get_current_target_obj(event) };
    if target.is_null() || target != current {
        return;
    }
    let pressed = unsafe { lv::lv_obj_has_state(target, lv::lv_state_t_LV_STATE_PRESSED) };
    crate::interaction_trace::emit(stage, target.addr() as u32, u32::from(pressed));
}
