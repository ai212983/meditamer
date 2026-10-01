//! Checked LVGL widget ownership and operations.
//!
//! This module owns the public widget handle and its registry-facing checks.
//! Raw LVGL access remains behind the sibling typed gateway.

use core::ffi::CStr;
use core::sync::atomic::Ordering;

use heapless::Vec;
use lightvgl_sys as lv;

use super::external_canvas::ExternalL8CanvasBuffer;
use super::raw;
use super::retained::{
    RetainedResourceError, StaticFontRef, StaticL8CanvasBuffer, StaticLinePoints,
};
use super::StaticI1CanvasBuffer;
use super::{
    next_generation, registry_commit, registry_release_reservation, registry_remove,
    registry_reserve, Registry, UiAccessToken, WIDGET_GENERATION, WIDGET_REGISTRY,
    WIDGET_REGISTRY_CAPACITY, WIDGET_RUNTIME_ID,
};

/// Why a handle was rejected before LVGL was touched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessFault {
    /// The handle was minted under a different [`UiAccessToken`] runtime.
    WrongRuntime,
    /// The generation no longer matches: the object was already destroyed,
    /// directly or with an ancestor.
    Stale,
    /// The operation requires a specific [`WidgetKind`] and this handle was
    /// created as another kind.
    WrongKind,
    /// Screen activation requires a top-level object with no parent.
    NotScreen,
}

/// Stable, non-pointer identity for diagnostics that need to prove the same
/// adapter-managed object survived an operation. It cannot be dereferenced or
/// passed to LVGL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WidgetIdentity {
    addr: usize,
    generation: u32,
    runtime_id: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum WidgetCreateError {
    Access(AccessFault),
    /// The underlying `lv_*_create` call returned null.
    ObjectCreation,
    /// The registry has no free slot. Capacity is reserved before LVGL creates
    /// an object, so this failure owns no cleanup.
    RegistryFull,
    /// The generation counter is exhausted (`u32::MAX` widgets created this
    /// process). Not reachable in practice; handled the same way
    /// `ShellNavigationError::InstanceGenerationExhausted` is.
    GenerationExhausted,
    /// LVGL refused cleanup after admission failed. The handle is returned
    /// so the caller retains ownership and can retry deletion.
    CleanupFailed(Widget),
}

impl From<AccessFault> for WidgetCreateError {
    fn from(fault: AccessFault) -> Self {
        Self::Access(fault)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetCallbackError {
    Access(AccessFault),
    /// `lv_obj_add_event_cb` returned null.
    Registration,
}

impl From<AccessFault> for WidgetCallbackError {
    fn from(fault: AccessFault) -> Self {
        Self::Access(fault)
    }
}

/// [`Widget::delete`]'s failure: either the handle was already invalid and
/// nothing was touched, or LVGL reported the object still valid after
/// deletion and the widget is handed back so the caller can retry — the same
/// shape as [`shell::lifecycle::DestroyFailure`], one level below it.
#[derive(Debug, Eq, PartialEq)]
pub enum WidgetDeleteFailure {
    AlreadyGone,
    StillValid(Widget),
}

/// Which `lv_*_create` a [`Widget`] wraps. Recorded on the handle itself and
/// checked by the kind-specific operations whose safety contract depends on
/// it ([`Widget::set_l8_canvas_buffer`], [`Widget::set_line_points`], and the
/// [`Widget::set_slider_range`] family) so none can be called on a widget
/// built as a different kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
    Container,
    Button,
    Label,
    /// An `lv_canvas`, painted by hand into a caller-owned pixel buffer --
    /// see [`Widget::set_l8_canvas_buffer`]. Nothing else about a canvas
    /// object is checked-adapter-specific; it takes every other setter
    /// (`set_pos`, `align`, ...) like any other widget.
    Canvas,
    /// An `lv_line`, drawn from a caller-owned point buffer -- see
    /// [`Widget::set_line_points`]. Otherwise an ordinary widget, like
    /// [`Self::Canvas`].
    Line,
    /// An `lv_slider`, ranged and read through [`Widget::set_slider_range`],
    /// [`Widget::set_slider_value`], [`Widget::slider_value`], and
    /// [`Widget::slider_range`], with drag levels routed through
    /// [`crate::intent_bridge`]'s value callbacks. Otherwise an ordinary
    /// widget, like [`Self::Canvas`].
    Slider,
}

/// `lv_obj_set_style_radius`'s sentinel for "as round as this object's own
/// size allows" -- LVGL computes it from the object's bounding box rather
/// than accepting it as a literal pixel radius, unlike every other value
/// [`Widget::set_radius`] takes.
pub const RADIUS_CIRCLE: i32 = lv::LV_RADIUS_CIRCLE as i32;

/// `lv_obj_set_style_text_align`'s alignment values this slice's screens use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextAlign {
    Center,
}

impl TextAlign {
    pub(super) fn raw(self) -> lv::lv_text_align_t {
        match self {
            Self::Center => lv::lv_text_align_t_LV_TEXT_ALIGN_CENTER,
        }
    }
}

/// `lv_obj_align`'s alignment reference points this slice's screens use --
/// horizontally-centered text whose rendered width depends on its (often
/// dynamic) content, which `Widget::set_pos`'s fixed coordinates cannot
/// express and `Widget::center` centers on both axes rather than one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    Center,
    TopMid,
    BottomMid,
    TopRight,
}

impl Align {
    pub(super) fn raw(self) -> lv::lv_align_t {
        match self {
            Self::Center => lv::lv_align_t_LV_ALIGN_CENTER,
            Self::TopMid => lv::lv_align_t_LV_ALIGN_TOP_MID,
            Self::BottomMid => lv::lv_align_t_LV_ALIGN_BOTTOM_MID,
            Self::TopRight => lv::lv_align_t_LV_ALIGN_TOP_RIGHT,
        }
    }
}

/// Style selector: the joint `lv_part_t`/`lv_state_t` LVGL expects on every
/// style setter. Only the two states this slice's presentation modules use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StyleState {
    Default,
    Pressed,
}

impl StyleState {
    pub(super) fn selector(self) -> lv::lv_style_selector_t {
        match self {
            Self::Default => 0,
            Self::Pressed => lv::lv_state_t_LV_STATE_PRESSED,
        }
    }
}

/// The compiled-in fonts this slice's presentation modules reference, kept as
/// an enum so callers never hold a raw `*const lv_font_t`. `Size20`/`Size32`
/// need the `font-20`/`font-32` features -- see their definitions in
/// `Cargo.toml` for why these two variants, unlike the rest of `Font`, are
/// not available to every consumer's `lv_conf.h`. `Custom` (built through
/// [`Self::custom`]) covers a product's generated, validated glyph table -- e.g.
/// Meditamer's Ambient View clock/environment faces, generated by
/// `tools/lvgl_font_compiler` past LVGL's built-in Montserrat sizes --
/// without this enum growing one variant per product-specific face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Font {
    Size14,
    Size18,
    #[cfg(feature = "font-20")]
    Size20,
    Size24,
    #[cfg(feature = "font-32")]
    Size32,
    Custom(StaticFontRef),
}

impl Font {
    /// Uses a custom font validated by the repository's font generator.
    pub const fn custom(font: StaticFontRef) -> Self {
        Self::Custom(font)
    }
}

/// LVGL's black/white, the only colors this slice's presentation modules use.
pub fn black() -> lv::lv_color_t {
    raw::black()
}

pub fn white() -> lv::lv_color_t {
    raw::white()
}

pub(crate) fn registry_check<const N: usize>(
    registry: &Registry<N>,
    addr: usize,
    generation: u32,
) -> bool {
    registry.lock(|entries| {
        entries
            .borrow()
            .iter()
            .any(|(a, g)| *a == addr && *g == generation)
    })
}

/// An owned, checked handle to an `lv_obj_t`. Not `Clone`: only the value
/// returned by the `create`/`child` calls below can ever be consumed by
/// [`Widget::delete`], so exactly one owner can delete it.
#[derive(Debug, Eq, PartialEq)]
pub struct Widget {
    pub(super) obj: raw::Object,
    pub(super) addr: usize,
    pub(super) generation: u32,
    pub(super) runtime_id: u32,
    pub(super) kind: WidgetKind,
}

impl Widget {
    fn register<F>(
        kind: WidgetKind,
        token: &UiAccessToken,
        create: F,
    ) -> Result<Self, WidgetCreateError>
    where
        F: FnOnce() -> Option<raw::Object>,
    {
        let capability = token.capability()?;
        let Some(generation) = next_generation(&WIDGET_GENERATION) else {
            return Err(WidgetCreateError::GenerationExhausted);
        };
        if !registry_reserve(&WIDGET_REGISTRY, generation) {
            return Err(WidgetCreateError::RegistryFull);
        }
        let Some(obj) = create() else {
            registry_release_reservation(&WIDGET_REGISTRY, generation);
            return Err(WidgetCreateError::ObjectCreation);
        };
        let addr = obj.addr();
        if !registry_commit(&WIDGET_REGISTRY, generation, addr) {
            if !raw::delete(&capability, obj) {
                return Err(WidgetCreateError::CleanupFailed(Self {
                    obj,
                    addr,
                    generation,
                    runtime_id: token.runtime_id,
                    kind,
                }));
            }
            return Err(WidgetCreateError::RegistryFull);
        }
        Ok(Self {
            obj,
            addr,
            generation,
            runtime_id: token.runtime_id,
            kind,
        })
    }

    fn check(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        if self.runtime_id != token.runtime_id
            || token.runtime_id != WIDGET_RUNTIME_ID.load(Ordering::Acquire)
        {
            return Err(AccessFault::WrongRuntime);
        }
        if !registry_check(&WIDGET_REGISTRY, self.addr, self.generation) {
            return Err(AccessFault::Stale);
        }
        Ok(())
    }

    /// As [`Self::check`], plus rejecting a handle whose [`WidgetKind`] is
    /// not `expected` -- for the two operations (`set_l8_canvas_buffer`,
    /// `set_line_points`) whose safety contract depends on which `lv_*_create`
    /// actually built this object, not just that it is still valid.
    fn check_kind(&self, token: &UiAccessToken, expected: WidgetKind) -> Result<(), AccessFault> {
        self.check(token)?;
        if self.kind != expected {
            return Err(AccessFault::WrongKind);
        }
        Ok(())
    }

    fn check_screen(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        if raw::has_parent(&token.capability()?, self.obj) {
            return Err(AccessFault::NotScreen);
        }
        Ok(())
    }

    /// Creates a new top-level screen object (`lv_obj_create(NULL)`).
    pub fn screen(token: &UiAccessToken) -> Result<Self, WidgetCreateError> {
        let capability = token.capability()?;
        Self::register(WidgetKind::Container, token, || {
            raw::create_screen(&capability, WidgetKind::Container)
        })
    }

    /// Creates a child of this widget.
    pub fn child(
        &self,
        token: &UiAccessToken,
        kind: WidgetKind,
    ) -> Result<Self, WidgetCreateError> {
        self.check(token)?;
        let capability = token.capability()?;
        Self::register(kind, token, || {
            raw::create_child(&capability, self.obj, kind)
        })
    }

    /// Creates a widget parented to LVGL's system layer (`lv_layer_sys`),
    /// for base overlays that must draw above every screen.
    pub fn on_system_layer(
        token: &UiAccessToken,
        kind: WidgetKind,
    ) -> Result<Self, WidgetCreateError> {
        let capability = token.capability()?;
        Self::register(kind, token, || {
            raw::create_on_system_layer(&capability, kind)
        })
    }

    pub fn set_pos(&self, token: &UiAccessToken, x: i32, y: i32) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_pos(&token.capability()?, self.obj, x, y);
        Ok(())
    }

    pub fn set_size(
        &self,
        token: &UiAccessToken,
        width: i32,
        height: i32,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_size(&token.capability()?, self.obj, width, height);
        Ok(())
    }

    /// Sets only the object's width, leaving height at LVGL's own default
    /// (typically content-sized for a label) -- distinct from [`Self::
    /// set_size`], which fixes both.
    pub fn set_width(&self, token: &UiAccessToken, width: i32) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_width(&token.capability()?, self.obj, width);
        Ok(())
    }

    pub fn set_ext_click_area(
        &self,
        token: &UiAccessToken,
        padding: i32,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_ext_click_area(&token.capability()?, self.obj, padding);
        Ok(())
    }

    pub fn remove_style_all(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::remove_style_all(&token.capability()?, self.obj);
        Ok(())
    }

    pub fn set_bg_color(
        &self,
        token: &UiAccessToken,
        color: lv::lv_color_t,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_bg_color(&token.capability()?, self.obj, color, state);
        Ok(())
    }

    pub fn set_bg_opa(
        &self,
        token: &UiAccessToken,
        opa: u8,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_bg_opa(&token.capability()?, self.obj, opa, state);
        Ok(())
    }

    pub fn set_text_color(
        &self,
        token: &UiAccessToken,
        color: lv::lv_color_t,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_text_color(&token.capability()?, self.obj, color, state);
        Ok(())
    }

    pub fn set_border_color(
        &self,
        token: &UiAccessToken,
        color: lv::lv_color_t,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_border_color(&token.capability()?, self.obj, color, state);
        Ok(())
    }

    pub fn set_border_width(
        &self,
        token: &UiAccessToken,
        width: i32,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_border_width(&token.capability()?, self.obj, width, state);
        Ok(())
    }

    pub fn set_radius(
        &self,
        token: &UiAccessToken,
        radius: i32,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_radius(&token.capability()?, self.obj, radius, state);
        Ok(())
    }

    pub fn set_text_font(
        &self,
        token: &UiAccessToken,
        font: Font,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_text_font(&token.capability()?, self.obj, font, state);
        Ok(())
    }

    pub fn set_text_align(
        &self,
        token: &UiAccessToken,
        align: TextAlign,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_text_align(&token.capability()?, self.obj, align, state);
        Ok(())
    }

    pub fn set_line_color(
        &self,
        token: &UiAccessToken,
        color: lv::lv_color_t,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_line_color(&token.capability()?, self.obj, color, state);
        Ok(())
    }

    pub fn set_line_width(
        &self,
        token: &UiAccessToken,
        width: i32,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_line_width(&token.capability()?, self.obj, width, state);
        Ok(())
    }

    pub fn set_line_rounded(
        &self,
        token: &UiAccessToken,
        rounded: bool,
        state: StyleState,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_line_rounded(&token.capability()?, self.obj, rounded, state);
        Ok(())
    }

    /// Sets a label's text. No lifetime requirement on `text` beyond the
    /// call itself: `lv_label_set_text` copies the string into a buffer the
    /// label object owns (`lv_malloc` plus a byte copy, in
    /// `set_text_internal`) rather than retaining the caller's pointer, so a
    /// stack-local or otherwise short-lived buffer -- built fresh each time
    /// a dynamic label (a clock, a reading, a status line) updates -- is as
    /// safe to pass as a `c"..."` literal.
    pub fn set_text(&self, token: &UiAccessToken, text: &CStr) -> Result<(), AccessFault> {
        self.check_kind(token, WidgetKind::Label)?;
        raw::set_label_text(&token.capability()?, self.obj, text);
        Ok(())
    }

    /// Sets an `lv_slider`'s (see [`WidgetKind::Slider`]) minimum and maximum.
    /// LVGL normalizes a reversed range itself and clamps the current value,
    /// so any `i32` bounds are safe once the kind check below passes.
    pub fn set_slider_range(
        &self,
        token: &UiAccessToken,
        min: i32,
        max: i32,
    ) -> Result<(), AccessFault> {
        self.check_kind(token, WidgetKind::Slider)?;
        raw::set_slider_range(&token.capability()?, self.obj, min, max);
        Ok(())
    }

    /// Sets an `lv_slider`'s current value without animation. LVGL clamps to
    /// the stored range. A programmatic set does not itself emit
    /// `LV_EVENT_VALUE_CHANGED`, so the value-routing callback only fires for
    /// genuine LVGL value events (a drag, or [`Self::send_value_changed`]).
    pub fn set_slider_value(&self, token: &UiAccessToken, value: i32) -> Result<(), AccessFault> {
        self.check_kind(token, WidgetKind::Slider)?;
        raw::set_slider_value(&token.capability()?, self.obj, value);
        Ok(())
    }

    /// Reads an `lv_slider`'s current value back from LVGL.
    pub fn slider_value(&self, token: &UiAccessToken) -> Result<i32, AccessFault> {
        self.check_kind(token, WidgetKind::Slider)?;
        Ok(raw::slider_value(&token.capability()?, self.obj))
    }

    /// Reads an `lv_slider`'s stored `(min, max)` range back from LVGL.
    pub fn slider_range(&self, token: &UiAccessToken) -> Result<(i32, i32), AccessFault> {
        self.check_kind(token, WidgetKind::Slider)?;
        Ok(raw::slider_range(&token.capability()?, self.obj))
    }

    pub fn center(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::center(&token.capability()?, self.obj);
        Ok(())
    }

    /// Marks the object's area dirty so the next `lv_timer_handler`/`lv_refr_now`
    /// pass redraws it -- the checked-adapter counterpart of a raw
    /// `lv_obj_invalidate`, needed after a canvas repaints its buffer
    /// directly (LVGL cannot detect that on its own) and after any other
    /// content change a style/geometry setter above doesn't already imply.
    pub fn invalidate(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::invalidate(&token.capability()?, self.obj);
        Ok(())
    }

    /// Removes clickable/click-focusable/scrollable from the object's
    /// flags, for a passive sticky overlay that must never intercept a
    /// touch or claim modal capture -- a status chip, not a control.
    pub fn set_non_interactive(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_non_interactive(&token.capability()?, self.obj);
        Ok(())
    }

    /// Whether any of clickable/click-focusable/scrollable is currently set
    /// -- the read-back [`Self::set_non_interactive`] itself has no
    /// counterpart for, used to confirm LVGL actually cleared them rather
    /// than assuming a flag-removal call that returns nothing succeeded.
    pub fn is_interactive(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check(token)?;
        Ok(raw::is_interactive(&token.capability()?, self.obj))
    }

    /// Adds or removes only the clickable flag, leaving click-focusable and
    /// scrollable untouched -- narrower than [`Self::set_non_interactive`],
    /// for a catalogue row whose availability toggles between a live control
    /// and inert text without ever needing the other two flags touched.
    pub fn set_clickable(&self, token: &UiAccessToken, clickable: bool) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_clickable(&token.capability()?, self.obj, clickable);
        Ok(())
    }

    /// Changes scrolling without changing click or keyboard-focus behavior.
    pub fn set_scrollable(
        &self,
        token: &UiAccessToken,
        scrollable: bool,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_scrollable(&token.capability()?, self.obj, scrollable);
        Ok(())
    }

    pub fn is_scrollable(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check(token)?;
        Ok(raw::is_scrollable(&token.capability()?, self.obj))
    }

    /// Positions the object relative to its parent -- see [`Align`] for why
    /// this exists alongside [`Self::set_pos`]/[`Self::center`].
    pub fn align(
        &self,
        token: &UiAccessToken,
        align: Align,
        x_ofs: i32,
        y_ofs: i32,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::align(&token.capability()?, self.obj, align, x_ofs, y_ofs);
        Ok(())
    }

    /// Points an `lv_canvas` (see [`WidgetKind::Canvas`]) at static,
    /// single-byte-per-pixel (`LV_COLOR_FORMAT_L8`) storage. The dimensions
    /// must be positive and the buffer must hold at least `width * height`
    /// bytes. LVGL retains the pointer; later writes remain serialized through
    /// [`StaticL8CanvasBuffer::with_writer`].
    pub fn set_l8_canvas_buffer<const N: usize>(
        &self,
        token: &UiAccessToken,
        buffer: &'static StaticL8CanvasBuffer<N>,
        width: i32,
        height: i32,
    ) -> Result<(), RetainedResourceError> {
        self.check_kind(token, WidgetKind::Canvas)?;
        let width = usize::try_from(width)
            .ok()
            .filter(|width| *width > 0)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        let height = usize::try_from(height)
            .ok()
            .filter(|height| *height > 0)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        let required = width
            .checked_mul(height)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        if buffer.len() < required {
            return Err(RetainedResourceError::CanvasBufferTooSmall);
        }
        raw::attach_l8_canvas(
            &token.capability()?,
            self.obj,
            buffer,
            width as i32,
            height as i32,
        );
        Ok(())
    }

    /// Points an `lv_canvas` (see [`WidgetKind::Canvas`]) at externally
    /// allocated boot-lifetime L8 storage. The dimensions must be positive
    /// and the buffer must hold at least `width * height` bytes, exactly as
    /// for [`Self::set_l8_canvas_buffer`]. LVGL retains the pointer; later
    /// writes remain serialized through
    /// [`ExternalL8CanvasBuffer::with_writer`], and repainting never
    /// invalidates the widget — call [`Self::invalidate`] afterwards.
    pub fn set_external_l8_canvas_buffer(
        &self,
        token: &UiAccessToken,
        buffer: &'static ExternalL8CanvasBuffer,
        width: i32,
        height: i32,
    ) -> Result<(), RetainedResourceError> {
        self.check_kind(token, WidgetKind::Canvas)?;
        let width = usize::try_from(width)
            .ok()
            .filter(|width| *width > 0)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        let height = usize::try_from(height)
            .ok()
            .filter(|height| *height > 0)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        let required = width
            .checked_mul(height)
            .ok_or(RetainedResourceError::InvalidCanvasDimensions)?;
        if buffer.len() < required {
            return Err(RetainedResourceError::CanvasBufferTooSmall);
        }
        let capability = token.capability()?;
        buffer.attach_to_canvas(&capability, self.obj.addr(), width as i32, height as i32);
        Ok(())
    }

    /// Attaches packed black/white storage using its constructor-checked geometry.
    /// LVGL retains both pixels and descriptor; repaint through `with_writer`
    /// and invalidate this widget after changes, just as for an L8 canvas.
    pub fn set_i1_canvas_buffer<const N: usize>(
        &self,
        token: &UiAccessToken,
        buffer: &'static StaticI1CanvasBuffer<N>,
    ) -> Result<(), RetainedResourceError> {
        self.check_kind(token, WidgetKind::Canvas)?;
        raw::attach_indexed_canvas(&token.capability()?, self.obj, buffer.indexed())?;
        Ok(())
    }

    /// Attaches a static indexed canvas with its fixed palette and geometry.
    /// Supports native LVGL 1/2/4/8-bit storage; eight-shade canvases use 4 bits.
    pub fn set_indexed_canvas_buffer<const BITS: usize, const N: usize>(
        &self,
        token: &UiAccessToken,
        buffer: &'static super::StaticIndexedCanvasBuffer<BITS, N>,
    ) -> Result<(), RetainedResourceError> {
        self.check_kind(token, WidgetKind::Canvas)?;
        raw::attach_indexed_canvas(&token.capability()?, self.obj, buffer)?;
        Ok(())
    }

    /// Points an `lv_line` (see [`WidgetKind::Line`]) at non-empty static
    /// point storage. LVGL retains the pointer; later writes remain serialized
    /// through [`StaticLinePoints::with_writer`].
    pub fn set_line_points<const N: usize>(
        &self,
        token: &UiAccessToken,
        points: &'static StaticLinePoints<N>,
    ) -> Result<(), RetainedResourceError> {
        self.check_kind(token, WidgetKind::Line)?;
        if points.is_empty() {
            return Err(RetainedResourceError::EmptyLinePoints);
        }
        let count =
            u32::try_from(points.len()).map_err(|_| RetainedResourceError::TooManyLinePoints)?;
        raw::attach_line_points(&token.capability()?, self.obj, points, count);
        Ok(())
    }

    pub fn set_hidden(&self, token: &UiAccessToken, hidden: bool) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_hidden(&token.capability()?, self.obj, hidden);
        Ok(())
    }

    /// Reports whether LVGL currently marks this object hidden.
    pub fn is_hidden(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check(token)?;
        Ok(raw::is_hidden(&token.capability()?, self.obj))
    }

    /// Sets the object's user data to `render::intent_bridge`'s
    /// navigation-index encoding, for widgets bound through
    /// [`crate::intent_bridge::bind_click`].
    pub fn set_navigation_index(
        &self,
        token: &UiAccessToken,
        index: usize,
    ) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_navigation_index(&token.capability()?, self.obj, index);
        Ok(())
    }

    fn bind_click(
        &self,
        token: &UiAccessToken,
        callback: raw::ClickCallback,
        route: Option<u32>,
    ) -> Result<(), WidgetCallbackError> {
        self.check(token)?;
        if !raw::register_click(&token.capability()?, self.obj, callback, route) {
            return Err(WidgetCallbackError::Registration);
        }
        Ok(())
    }

    pub(crate) fn bind_navigation_click(
        &self,
        token: &UiAccessToken,
        index: usize,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.set_navigation_index(token, index)?;
        self.bind_click(token, raw::ClickCallback::Navigation, Some(route))
    }

    pub(crate) fn bind_show_confirm_click(
        &self,
        token: &UiAccessToken,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.bind_click(token, raw::ClickCallback::ShowConfirm, Some(route))
    }

    pub(crate) fn bind_dismiss_modal_click(
        &self,
        token: &UiAccessToken,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.bind_click(token, raw::ClickCallback::DismissModal, Some(route))
    }

    pub(crate) fn bind_full_repaint_click(
        &self,
        token: &UiAccessToken,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.bind_click(token, raw::ClickCallback::FullRepaint, Some(route))
    }

    pub(crate) fn bind_action_click(
        &self,
        token: &UiAccessToken,
        index: usize,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.set_navigation_index(token, index)?;
        self.bind_click(token, raw::ClickCallback::Action, Some(route))
    }

    pub(crate) fn bind_value_changed(
        &self,
        token: &UiAccessToken,
        index: usize,
        route: u32,
    ) -> Result<(), WidgetCallbackError> {
        self.set_navigation_index(token, index)?;
        self.check_kind(token, WidgetKind::Slider)?;
        if !raw::register_value_changed(&token.capability()?, self.obj, route) {
            return Err(WidgetCallbackError::Registration);
        }
        Ok(())
    }

    pub(crate) fn bind_ambient_tap(
        &self,
        token: &UiAccessToken,
    ) -> Result<(), WidgetCallbackError> {
        self.bind_click(token, raw::ClickCallback::AmbientTap, None)
    }

    /// Synthesizes a click, running every registered `LV_EVENT_CLICKED`
    /// callback synchronously as if a real touch/click had landed -- for a
    /// fixture that drives a UI action programmatically rather than waiting
    /// on physical input. Returns whether LVGL reported the dispatch itself
    /// successful (unrelated to what any callback then decided to do).
    pub fn send_click(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check(token)?;
        Ok(raw::send_click(&token.capability()?, self.obj))
    }

    /// Synthesizes a value-changed event, running every registered
    /// `LV_EVENT_VALUE_CHANGED` callback synchronously as if a drag had
    /// moved the slider -- the value-path counterpart of [`Self::send_click`],
    /// for a fixture that drives slider delivery programmatically. The kind
    /// check keeps the typed pairing tight: the value callback captures an
    /// `lv_slider` value, so only a slider handle may send one. Returns
    /// whether LVGL reported the dispatch itself successful.
    pub fn send_value_changed(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check_kind(token, WidgetKind::Slider)?;
        Ok(raw::send_value_changed(&token.capability()?, self.obj))
    }

    /// Stable identity for diagnostics without exposing LVGL's object pointer.
    pub fn identity(&self) -> WidgetIdentity {
        WidgetIdentity {
            addr: self.addr,
            generation: self.generation,
            runtime_id: self.runtime_id,
        }
    }

    /// Loads this widget as LVGL's active screen (`lv_screen_load`), the
    /// checked equivalent of a product's screen-swap glue calling
    /// `lv_screen_load`/`lv_screen_active` on a raw pointer directly. Returns
    /// whether LVGL actually reports this object active afterward.
    pub fn activate(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check_screen(token)?;
        Ok(raw::activate(&token.capability()?, self.obj))
    }

    /// Whether LVGL currently reports this widget as the active screen --
    /// the checked counterpart of comparing a raw pointer against
    /// `lv_screen_active()` directly.
    pub fn is_active_screen(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check_screen(token)?;
        Ok(raw::is_active_screen(&token.capability()?, self.obj))
    }

    /// Deletes the object. Descendant registrations are removed only after
    /// LVGL confirms the root is gone, so a retryable root retains coherent
    /// child handles while a successful delete invalidates them all.
    pub fn delete(self, token: &UiAccessToken) -> Result<(), WidgetDeleteFailure> {
        if self.check(token).is_err() {
            return Err(WidgetDeleteFailure::AlreadyGone);
        }
        let mut descendants = Vec::<usize, WIDGET_REGISTRY_CAPACITY>::new();
        let capability = token
            .capability()
            .map_err(|_| WidgetDeleteFailure::AlreadyGone)?;
        collect_registered_descendants(&capability, self.obj, &mut descendants);
        if !raw::delete(&capability, self.obj) {
            return Err(WidgetDeleteFailure::StillValid(self));
        }
        for addr in descendants {
            registry_remove(&WIDGET_REGISTRY, addr);
        }
        registry_remove(&WIDGET_REGISTRY, self.addr);
        Ok(())
    }
}

pub(crate) fn registry_contains_addr(
    registry: &Registry<WIDGET_REGISTRY_CAPACITY>,
    addr: usize,
) -> bool {
    registry.lock(|entries| {
        entries
            .borrow()
            .iter()
            .any(|(registered, _)| *registered == addr)
    })
}

fn collect_registered_descendants(
    capability: &raw::UiCapability<'_>,
    obj: raw::Object,
    descendants: &mut Vec<usize, WIDGET_REGISTRY_CAPACITY>,
) {
    let count = raw::child_count(capability, obj);
    for index in 0..count {
        let Some(child) = raw::child_at(capability, obj, index) else {
            continue;
        };
        collect_registered_descendants(capability, child, descendants);
        let addr = child.addr();
        if registry_contains_addr(&WIDGET_REGISTRY, addr) {
            descendants
                .push(addr)
                .expect("registered descendants fit the widget registry capacity");
        }
    }
}
