//! Board-owned LVGL-to-`St7305` flush bridge and framebuffer geometry.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use board::{DirtyArea, Panel, RefreshMode};
use lightvgl_sys as lv;
use static_cell::ConstStaticCell;

use crate::panel::St7305;

/// LVGL renders into this in L8 and hands us dirty rectangles; the panel packs
/// them. Sized for a horizontal band rather than the whole surface, which is
/// how LVGL prefers to work and keeps the buffer off the 120KB the full
/// 400x300 L8 image would need.
const DRAW_LINES: usize = 40;

// LVGL requires draw buffers to satisfy `LV_DRAW_BUF_ALIGN` (4 by default).
// A bare Rust `[u8; N]` has alignment 1 and can otherwise land at an address
// which sends LVGL's default assertion handler into its intentional halt loop.
#[repr(align(4))]
struct AlignedDrawBuffer([u8; crate::panel::WIDTH * DRAW_LINES]);

// `ConstStaticCell` retains static placement, adds only its one-shot state
// flag, guarantees the zero initializer never materializes on the boot stack,
// and replaces a globally accessible mutable static with one-shot ownership.
static DRAW_BUFFER: ConstStaticCell<AlignedDrawBuffer> =
    ConstStaticCell::new(AlignedDrawBuffer([0; crate::panel::WIDTH * DRAW_LINES]));

/// The panel the C flush callback writes into.
///
/// The stable raw address is unavoidable because LVGL's callback has no useful
/// Rust lifetime or user-data slot. Publication is one-shot, and all safe
/// control access is carried by [`PanelLvglSession`].
static ACTIVE_PANEL: AtomicPtr<St7305<'static>> = AtomicPtr::new(ptr::null_mut());

fn l8_len(area: DirtyArea) -> Option<usize> {
    let width = area.x2.saturating_sub(area.x1).saturating_add(1);
    let height = area.y2.saturating_sub(area.y1).saturating_add(1);
    if width <= 0 || height <= 0 {
        return None;
    }
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    let len = width.checked_mul(height)?;
    (len <= isize::MAX as usize).then_some(len)
}

unsafe extern "C" fn flush_cb(
    display: *mut lv::lv_display_t,
    area: *const lv::lv_area_t,
    pixels: *mut u8,
) {
    let panel = ACTIVE_PANEL.load(Ordering::Acquire);
    if !panel.is_null() && !area.is_null() && !pixels.is_null() {
        // SAFETY: LVGL invokes a display flush callback with an `lv_area_t`
        // valid for the duration of the call.
        let area = unsafe { *area };
        let area = DirtyArea {
            x1: area.x1,
            y1: area.y1,
            x2: area.x2,
            y2: area.y2,
        };
        if let Some(pixel_len) = l8_len(area) {
            // SAFETY: LVGL's flush contract provides width x height bytes for
            // an L8 display. This is the sole raw-pixel conversion boundary;
            // the panel API receives a validated slice.
            let pixels = unsafe { core::slice::from_raw_parts(pixels.cast_const(), pixel_len) };
            // SAFETY: `PanelLvglSession::init` publishes a unique, stable
            // `St7305` address once. LVGL and session controls stay on the
            // non-Send UI owner; callbacks are synchronous with those calls.
            let panel = unsafe { &mut *panel };
            let accepted = panel.blit_l8(area, pixels);
            // LVGL flushes a frame in several chunks and only the last is
            // marked; pushing to the glass on every chunk would tear and be
            // slow. Partial sends the bounding box accumulated by the blits.
            if accepted && unsafe { lv::lv_display_flush_is_last(display) } {
                let _ = panel.refresh(RefreshMode::Partial);
            }
        }
    }
    unsafe { lv::lv_display_flush_ready(display) };
}

/// One-shot ownership capability for the panel installed behind LVGL.
///
/// The value is deliberately non-cloneable and non-Send/non-Sync. The target
/// moves it into the one UI task alongside its `UiAccessToken`; product and
/// target helpers then use safe methods instead of reaching through a global
/// panel pointer. It is zero-sized, so containment adds no runtime buffer or
/// persistent state beyond the existing published panel address.
pub struct PanelLvglSession {
    _not_send_or_sync: core::marker::PhantomData<*const ()>,
}

impl PanelLvglSession {
    /// Initialize LVGL once and bind its flush callback to `panel` for the
    /// program lifetime.
    ///
    /// Panics if dimensions are invalid, a session was already initialized,
    /// or LVGL could not create its display. The consumed `&'static mut`
    /// reference is thereafter represented exclusively by this capability and
    /// the synchronous C callback installed here.
    pub fn init(panel: &'static mut St7305<'static>, width: i32, height: i32) -> Self {
        assert!(
            width > 0 && height > 0,
            "LVGL display dimensions must be positive"
        );

        let panel = panel as *mut St7305<'static>;
        assert!(
            ACTIVE_PANEL
                .compare_exchange(ptr::null_mut(), panel, Ordering::AcqRel, Ordering::Acquire,)
                .is_ok(),
            "Panel LVGL session already initialized"
        );

        let draw_buffer = DRAW_BUFFER.take();
        // SAFETY: one-shot publication above proves this is the only LVGL
        // initialization path. The draw buffer is aligned, static, and lives
        // for the runtime; `flush_cb` has the ABI LVGL requires.
        unsafe {
            lv::lv_init();
            let display = lv::lv_display_create(width, height);
            assert!(!display.is_null(), "LVGL display creation failed");
            lv::lv_display_set_flush_cb(display, Some(flush_cb));
            lv::lv_display_set_color_format(display, lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
            lv::lv_display_set_buffers(
                display,
                draw_buffer.0.as_mut_ptr().cast::<c_void>(),
                ptr::null_mut(),
                core::mem::size_of::<AlignedDrawBuffer>() as u32,
                lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
            );
        };

        Self {
            _not_send_or_sync: core::marker::PhantomData,
        }
    }

    /// Select the panel's drive mode through the uniquely owned session.
    pub fn set_power_mode(&mut self, mode: crate::panel::PowerMode) {
        let panel = ACTIVE_PANEL.load(Ordering::Acquire);
        debug_assert!(!panel.is_null());
        // SAFETY: the one-shot session owns the unique panel reference, this
        // method requires `&mut self`, and the non-Send UI owner serializes it
        // with LVGL's synchronous callback.
        unsafe { (&mut *panel).set_power_mode(mode) };
    }

    /// Push the complete framebuffer as an explicit visual cleanup/cue.
    pub fn force_full_refresh(&mut self) {
        let panel = ACTIVE_PANEL.load(Ordering::Acquire);
        debug_assert!(!panel.is_null());
        // SAFETY: same exclusive session/callback invariant as
        // `set_power_mode` above.
        let _ = unsafe { (&mut *panel).refresh(RefreshMode::Full) };
    }
}
