//! Shared no_std prepared-L8 custom draw unit (LVGL 9.5.4).
//!
//! One [`PreparedL8DrawUnit`] serves one prepared, opaque L8 scene image.
//! IMAGE tasks retain valid software descriptors. Claims use score 80 versus
//! software's 100; ordinary images and unsupported effects remain software
//! tasks. Preparation happens before refresh. Dispatch only copies clipped
//! pixels, then finishes synchronously. No callback prints, panics,
//! allocates, or awaits. Raw draw-unit FFI is quarantined here, next to the
//! [`super::ExternalL8CanvasBuffer`] storage it identifies; widget access
//! uses the checked render adapter.
//!
//! Per-unit counters live in the LVGL-owned [`UnitState`] extension (which
//! LVGL allocates and frees); there are no globals, atomics, or additional
//! production statics. The handle only stores the extension address plus its
//! cloned [`super::UiAccessToken`], so there is no lifetime self-reference.

#![allow(unsafe_code)]

use lightvgl_sys as lv;

use super::{ExternalL8CanvasBuffer, UiAccessToken};

const CLAIM_SCORE: u8 = 80;
const MAX_TAKE_PER_DISPATCH: i32 = 64;
static UNIT_NAME: &[u8] = b"clock-scene\0";

/// Per-unit served-task counters. Wrapping arithmetic keeps long uptimes
/// well-defined; all mutation happens in LVGL callbacks on the owning UI
/// task, so no atomics are needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreparedL8DrawStats {
    pub accepted: u32,
    pub completed: u32,
    pub pixels: u32,
    pub idle_polls: u32,
    pub fallbacks: u32,
}

/// LVGL allocates and frees this extension. The base must be the first field.
#[repr(C)]
struct UnitState {
    base: lv::lv_draw_unit_t,
    enabled: bool,
    source: usize,
    width: usize,
    height: usize,
    accepted: u32,
    completed: u32,
    pixels: u32,
    idle_polls: u32,
    fallbacks: u32,
}

/// Runtime-epoch-checked handle. The token also prevents cross-thread access.
/// Dropping the runtime first leaves an inert handle, never a dangling safe
/// API. Dropping the handle disables only its own extension state; LVGL
/// deallocates the extension itself at deinit.
pub struct PreparedL8DrawUnit {
    ptr: *mut UnitState,
    token: UiAccessToken,
}

impl PreparedL8DrawUnit {
    /// Registers a unit on the current runtime epoch and claims it for the
    /// cloned token. Returns `None` when the token is stale or LVGL refuses
    /// the allocation.
    pub fn register(token: &UiAccessToken) -> Option<Self> {
        token.check_current().ok()?;
        // Routing stores unit IDs in one byte. Decline before allocating an
        // extension that could wrap to the unassigned/software unit ID.
        // SAFETY: the current epoch proves LVGL is initialized on this UI task.
        if unsafe { lv::lv_draw_get_unit_count() } >= u8::MAX as u32 {
            return None;
        }
        // SAFETY: checked live runtime; LVGL returns zeroed, aligned owned storage.
        let ptr = unsafe { lv::lv_draw_create_unit(size_of::<UnitState>()).cast::<UnitState>() };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: exclusively initialize our extension and callback slots.
        unsafe {
            (*ptr).base.name = UNIT_NAME.as_ptr().cast();
            (*ptr).base.dispatch_cb = Some(dispatch_cb);
            (*ptr).base.evaluate_cb = Some(evaluate_cb);
            (*ptr).base.delete_cb = Some(delete_cb);
            (*ptr).enabled = true;
        }
        Some(Self {
            ptr,
            token: token.clone(),
        })
    }

    pub fn unit_idx(&self) -> Option<u8> {
        self.token.check_current().ok()?;
        // SAFETY: epoch check proves the LVGL-owned extension is still live.
        Some(unsafe { (*self.ptr).base.idx as u8 })
    }

    /// Binds caller-owned canvas storage by identity. Only the private
    /// [`ExternalL8CanvasBuffer::as_mut_ptr`] identity is retained; no raw
    /// user address is accepted, and no bytes are dereferenced here. Returns
    /// false (without mutation) for a stale token, empty geometry, or a
    /// frame that does not fit the storage.
    pub fn set_source(
        &self,
        storage: &'static ExternalL8CanvasBuffer,
        width: usize,
        height: usize,
    ) -> bool {
        if self.token.check_current().is_err()
            || width == 0
            || height == 0
            || width
                .checked_mul(height)
                .is_none_or(|need| need > storage.len())
        {
            return false;
        }
        let addr = storage.as_mut_ptr() as usize;
        // SAFETY: live epoch, single UI thread, no concurrent LVGL callback.
        unsafe {
            (*self.ptr).source = addr;
            (*self.ptr).width = width;
            (*self.ptr).height = height;
        }
        true
    }

    pub fn clear_source(&self) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        // SAFETY: live epoch and exclusive UI-thread access.
        unsafe {
            (*self.ptr).source = 0;
        }
        true
    }

    pub fn set_enabled(&self, enabled: bool) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        // SAFETY: epoch check before every handle dereference.
        unsafe {
            (*self.ptr).enabled = enabled;
        }
        true
    }

    /// Per-unit counters. Returns `None` for a stale token.
    pub fn telemetry(&self) -> Option<PreparedL8DrawStats> {
        self.token.check_current().ok()?;
        // SAFETY: epoch check proves the LVGL-owned extension is still live.
        unsafe {
            let state = &*self.ptr;
            Some(PreparedL8DrawStats {
                accepted: state.accepted,
                completed: state.completed,
                pixels: state.pixels,
                idle_polls: state.idle_polls,
                fallbacks: state.fallbacks,
            })
        }
    }

    /// Zeroes the per-unit counters. Returns false for a stale token.
    pub fn reset_telemetry(&self) -> bool {
        if self.token.check_current().is_err() {
            return false;
        }
        // SAFETY: live epoch and exclusive UI-thread access.
        unsafe {
            (*self.ptr).accepted = 0;
            (*self.ptr).completed = 0;
            (*self.ptr).pixels = 0;
            (*self.ptr).idle_polls = 0;
            (*self.ptr).fallbacks = 0;
        }
        true
    }
}

impl Drop for PreparedL8DrawUnit {
    fn drop(&mut self) {
        // Epoch check before every handle dereference, drop included: a
        // handle outliving its runtime must stay inert, and only this unit's
        // own state is touched -- LVGL frees the extension at deinit.
        if self.token.check_current().is_err() {
            return;
        }
        // SAFETY: live epoch and exclusive UI-thread access.
        unsafe {
            (*self.ptr).enabled = false;
            (*self.ptr).source = 0;
        }
    }
}

/// Fully validated row-copy plan; validated before the first write.
struct CopyPlan {
    src: *const u8,
    dst: *mut u8,
    src_off: usize,
    dst_off: usize,
    src_stride: usize,
    dst_stride: usize,
    row_len: usize,
    rows: usize,
}

fn extent(w: usize, h: usize, stride: usize, size: usize) -> Option<()> {
    if w == 0 || h == 0 || stride < w {
        return None;
    }
    let end = (h - 1).checked_mul(stride)?.checked_add(w)?;
    (end <= size).then_some(())
}

fn supported_image(d: &lv::lv_draw_image_dsc_t, header: lv::lv_image_header_t) -> bool {
    header.flags()
        & (lv::_lvimage_flags_t_LV_IMAGE_FLAGS_COMPRESSED
            | lv::_lvimage_flags_t_LV_IMAGE_FLAGS_CUSTOM_DRAW)
        == 0
        && d.rotation == 0
        && d.scale_x == lv::LV_SCALE_NONE as i32
        && d.scale_y == lv::LV_SCALE_NONE as i32
        && d.skew_x == 0
        && d.skew_y == 0
        && d.opa == 255
        && d.recolor_opa == 0
        && d.tile() == 0
        && d.clip_radius == 0
        && d.blend_mode() == lv::lv_blend_mode_t_LV_BLEND_MODE_NORMAL
        && d.bitmap_mask_src.is_null()
        && d.colorkey.is_null()
        && d.base.drop_shadow_opa == 0
}

/// All pointers come from live LVGL task metadata. VARIABLE sources are
/// checked before reading their common header/data prefix; filenames are
/// never structs. Source identity arrives as plain values (read from the
/// extension through narrow raw field accesses at the call sites) so the
/// whole-state borrow below never aliases a counter mutation.
unsafe fn copy_plan(
    enabled: bool,
    source: usize,
    width: usize,
    height: usize,
    t: &lv::lv_draw_task_t,
) -> Option<CopyPlan> {
    if !enabled
        || source == 0
        || t.type_ != lv::lv_draw_task_type_t_LV_DRAW_TASK_TYPE_IMAGE
        || t.draw_dsc.is_null()
        || t.target_layer.is_null()
    {
        return None;
    }
    // SAFETY: IMAGE task descriptor, valid throughout evaluate and dispatch.
    let d = unsafe { &*t.draw_dsc.cast::<lv::lv_draw_image_dsc_t>() };
    if d.src.is_null() {
        return None;
    }
    // SAFETY: LVGL image sources permit the discriminator's first-byte read.
    if unsafe { lv::lv_image_src_get_type(d.src) } != lv::lv_image_src_t_LV_IMAGE_SRC_VARIABLE {
        return None;
    }
    // SAFETY: VARIABLE source guarantees the shared image descriptor prefix.
    // Read only fields, without creating a reference to a larger draw-buffer type.
    let p = d.src.cast::<lv::lv_image_dsc_t>();
    let (header, src, src_size) = unsafe { ((*p).header, (*p).data, (*p).data_size as usize) };
    if src.is_null()
        || src as usize != source
        || header.cf() != lv::lv_color_format_t_LV_COLOR_FORMAT_L8
        || header.w() as usize != width
        || header.h() as usize != height
        || t.opa != 255
        || !supported_image(d, header)
    {
        return None;
    }
    let aw = i64::from(t.area.x2) - i64::from(t.area.x1) + 1;
    let ah = i64::from(t.area.y2) - i64::from(t.area.y1) + 1;
    if aw != width as i64
        || ah != height as i64
        || d.image_area.x1 != t.area.x1
        || d.image_area.y1 != t.area.y1
        || d.image_area.x2 != t.area.x2
        || d.image_area.y2 != t.area.y2
    {
        return None;
    }
    // SAFETY: the task retains its live target layer during both callbacks.
    unsafe { target_copy_plan(t, src, src_size, (width, height), header.stride() as usize) }
}

/// Validate the destination and entire clipped rectangle before any row is copied.
unsafe fn target_copy_plan(
    t: &lv::lv_draw_task_t,
    src: *const u8,
    src_size: usize,
    dimensions: (usize, usize),
    ss: usize,
) -> Option<CopyPlan> {
    let (width, height) = dimensions;
    // SAFETY: LVGL owns the target layer and its draw buffer for this task.
    let layer = unsafe { &*t.target_layer };
    if layer.draw_buf.is_null() || layer.opa != 255 {
        return None;
    }
    let dst = unsafe { &*layer.draw_buf };
    if dst.data.is_null() || dst.header.cf() != lv::lv_color_format_t_LV_COLOR_FORMAT_L8 {
        return None;
    }
    let ds = dst.header.stride() as usize;
    let dw = dst.header.w() as usize;
    let dh = dst.header.h() as usize;
    extent(width, height, ss, src_size)?;
    extent(dw, dh, ds, dst.data_size as usize)?;
    let src_end = (src as usize).checked_add(src_size)?;
    let dst_end = (dst.data as usize).checked_add(dst.data_size as usize)?;
    if (src as usize) < dst_end && (dst.data as usize) < src_end {
        return None;
    }
    let b = &layer.buf_area;
    if i64::from(b.x2) - i64::from(b.x1) + 1 > dw as i64
        || i64::from(b.y2) - i64::from(b.y1) + 1 > dh as i64
    {
        return None;
    }
    let x1 = t.area.x1.max(t.clip_area.x1).max(b.x1);
    let y1 = t.area.y1.max(t.clip_area.y1).max(b.y1);
    let x2 = t.area.x2.min(t.clip_area.x2).min(b.x2);
    let y2 = t.area.y2.min(t.clip_area.y2).min(b.y2);
    if x2 < x1 || y2 < y1 {
        return Some(CopyPlan {
            src,
            dst: dst.data,
            src_off: 0,
            dst_off: 0,
            src_stride: ss,
            dst_stride: ds,
            row_len: 0,
            rows: 0,
        });
    }
    let row_len = (i64::from(x2) - i64::from(x1) + 1) as usize;
    let rows = (i64::from(y2) - i64::from(y1) + 1) as usize;
    let src_off = ((i64::from(y1) - i64::from(t.area.y1)) as usize)
        .checked_mul(ss)?
        .checked_add((i64::from(x1) - i64::from(t.area.x1)) as usize)?;
    let dst_off = ((i64::from(y1) - i64::from(b.y1)) as usize)
        .checked_mul(ds)?
        .checked_add((i64::from(x1) - i64::from(b.x1)) as usize)?;
    let se = src_off
        .checked_add((rows - 1).checked_mul(ss)?)?
        .checked_add(row_len)?;
    let de = dst_off
        .checked_add((rows - 1).checked_mul(ds)?)?
        .checked_add(row_len)?;
    if se > src_size || de > dst.data_size as usize {
        return None;
    }
    Some(CopyPlan {
        src,
        dst: dst.data,
        src_off,
        dst_off,
        src_stride: ss,
        dst_stride: ds,
        row_len,
        rows,
    })
}

unsafe extern "C" fn evaluate_cb(
    unit: *mut lv::lv_draw_unit_t,
    task: *mut lv::lv_draw_task_t,
) -> i32 {
    if unit.is_null() || task.is_null() {
        return 0;
    }
    // SAFETY: callback registered only on UnitState extensions; task is LVGL-owned.
    // Narrow field reads keep the whole-state borrow from aliasing the
    // counter mutation below.
    let (enabled, source, width, height, idx) = unsafe {
        let state = &*unit.cast::<UnitState>();
        (
            state.enabled,
            state.source,
            state.width,
            state.height,
            state.base.idx,
        )
    };
    if unsafe { copy_plan(enabled, source, width, height, &*task) }.is_some() {
        // SAFETY: live task routing fields are the documented evaluate contract,
        // and this unit's own extension outlives the callback.
        unsafe {
            (*task).preferred_draw_unit_id = idx as u8;
            (*task).preference_score = CLAIM_SCORE;
            let accepted = core::ptr::addr_of_mut!((*unit.cast::<UnitState>()).accepted);
            accepted.write(accepted.read().wrapping_add(1));
        }
    }
    0
}

unsafe extern "C" fn dispatch_cb(unit: *mut lv::lv_draw_unit_t, layer: *mut lv::lv_layer_t) -> i32 {
    if unit.is_null() || layer.is_null() {
        return lv::LV_DRAW_UNIT_IDLE;
    }
    // SAFETY: LVGL calls our callback only with our live extension.
    let (enabled, source, width, height, idx) = unsafe {
        let state = &*unit.cast::<UnitState>();
        (
            state.enabled,
            state.source,
            state.width,
            state.height,
            state.base.idx as u8,
        )
    };
    let mut taken = 0;
    let mut previous = core::ptr::null_mut();
    while taken < MAX_TAKE_PER_DISPATCH {
        // SAFETY: standard task picker with a live layer and optional live cursor.
        let task = unsafe { lv::lv_draw_get_next_available_task(layer, previous, idx) };
        if task.is_null() {
            break;
        }
        previous = task;
        // SAFETY: picker returns a live WAITING task; skip unassigned ordinary tasks.
        if unsafe { (*task).preferred_draw_unit_id } != idx {
            continue;
        }
        let plan = unsafe { copy_plan(enabled, source, width, height, &*task) };
        if let Some(p) = plan {
            for row in 0..p.rows {
                // SAFETY: complete rectangle, strides, and disjoint allocations were
                // validated before any write. Metadata cannot change on this thread.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        p.src.add(p.src_off + row * p.src_stride),
                        p.dst.add(p.dst_off + row * p.dst_stride),
                        p.row_len,
                    );
                }
            }
            // SAFETY: this owned task was synchronously completed, and this
            // unit's own extension outlives the callback. Narrow counter
            // writes only; the validated plan holds raw pointers, never a
            // whole-state borrow.
            unsafe {
                (*task).state = lv::lv_draw_task_state_t_LV_DRAW_TASK_STATE_FINISHED as i32;
                let completed = core::ptr::addr_of_mut!((*unit.cast::<UnitState>()).completed);
                completed.write(completed.read().wrapping_add(1));
                let pixels = core::ptr::addr_of_mut!((*unit.cast::<UnitState>()).pixels);
                pixels.write(pixels.read().wrapping_add((p.rows * p.row_len) as u32));
            }
        } else {
            // SAFETY: valid IMAGE descriptor is retained. This pinned configuration
            // always has SW unit 1; reroute without writing any pixels or claiming success.
            unsafe {
                (*task).preferred_draw_unit_id = 1;
                (*task).preference_score = 100;
                (*task).state = lv::lv_draw_task_state_t_LV_DRAW_TASK_STATE_WAITING as i32;
                let fallbacks = core::ptr::addr_of_mut!((*unit.cast::<UnitState>()).fallbacks);
                fallbacks.write(fallbacks.read().wrapping_add(1));
            }
        }
        taken += 1;
    }
    if taken > 0 {
        // SAFETY: completion/rerouting can unblock software tasks in the next pass.
        unsafe {
            lv::lv_draw_dispatch_request();
        }
        taken
    } else {
        // SAFETY: LVGL calls our callback only with our live extension.
        unsafe {
            let idle = core::ptr::addr_of_mut!((*unit.cast::<UnitState>()).idle_polls);
            idle.write(idle.read().wrapping_add(1));
        }
        lv::LV_DRAW_UNIT_IDLE
    }
}

unsafe extern "C" fn delete_cb(unit: *mut lv::lv_draw_unit_t) -> i32 {
    if unit.is_null() {
        return 0;
    }
    // SAFETY: LVGL invokes delete before freeing our owned extension.
    let s = unsafe { &mut *unit.cast::<UnitState>() };
    s.enabled = false;
    s.source = 0;
    0
}

#[cfg(test)]
mod tests {
    //! Metadata-contract checks use local C-layout fixtures, without an LVGL
    //! runtime. Real-callback behavior is proven by the host prototype.

    use super::*;

    fn header() -> lv::lv_image_header_t {
        // SAFETY: this C struct contains only integer/bitfield storage.
        let mut h: lv::lv_image_header_t = unsafe { core::mem::zeroed() };
        h.set_magic(lv::LV_IMAGE_HEADER_MAGIC);
        h.set_cf(lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
        h.set_w(4);
        h.set_h(3);
        h.set_stride(6);
        h
    }

    #[test]
    fn extension_starts_with_draw_unit_base() {
        assert_eq!(core::mem::offset_of!(UnitState, base), 0);
    }

    #[test]
    fn stats_default_to_zero() {
        assert_eq!(
            PreparedL8DrawStats::default(),
            PreparedL8DrawStats {
                accepted: 0,
                completed: 0,
                pixels: 0,
                idle_polls: 0,
                fallbacks: 0,
            }
        );
    }

    #[test]
    fn source_discrimination_and_rectangle_validation() {
        let source = [123u8; 18];
        let mut target = [45u8; 18];
        // SAFETY: these generated C records contain integer fields and nullable pointers.
        let mut image: lv::lv_image_dsc_t = unsafe { core::mem::zeroed() };
        image.header = header();
        image.data = source.as_ptr();
        image.data_size = source.len() as u32;
        let mut buffer: lv::lv_draw_buf_t = unsafe { core::mem::zeroed() };
        buffer.header = header();
        buffer.data = target.as_mut_ptr();
        buffer.data_size = target.len() as u32;
        let area = lv::lv_area_t {
            x1: 10,
            y1: 20,
            x2: 13,
            y2: 22,
        };
        let mut layer: lv::lv_layer_t = unsafe { core::mem::zeroed() };
        layer.draw_buf = &mut buffer;
        layer.buf_area = area;
        layer.opa = 255;
        let mut descriptor: lv::lv_draw_image_dsc_t = unsafe { core::mem::zeroed() };
        descriptor.src = (&image as *const lv::lv_image_dsc_t).cast();
        descriptor.opa = 255;
        descriptor.scale_x = lv::LV_SCALE_NONE as i32;
        descriptor.scale_y = lv::LV_SCALE_NONE as i32;
        descriptor.image_area = area;
        let mut task: lv::lv_draw_task_t = unsafe { core::mem::zeroed() };
        task.type_ = lv::lv_draw_task_type_t_LV_DRAW_TASK_TYPE_IMAGE;
        task.area = area;
        task.clip_area = lv::lv_area_t {
            x1: 11,
            y1: 21,
            x2: 12,
            y2: 22,
        };
        task.draw_dsc = (&mut descriptor as *mut lv::lv_draw_image_dsc_t).cast();
        task.target_layer = &mut layer;
        task.opa = 255;
        let enabled = true;
        let (mut addr, w, h) = (source.as_ptr() as usize, 4usize, 3usize);
        // Explicit borrows keep each metadata record alive and mark updates as
        // read at the FFI boundary, where the task otherwise retains raw
        // pointers.
        let check = |enabled: bool,
                     addr: usize,
                     w: usize,
                     h: usize,
                     task: &lv::lv_draw_task_t,
                     _descriptor: &lv::lv_draw_image_dsc_t,
                     _image: &lv::lv_image_dsc_t,
                     _buffer: &lv::lv_draw_buf_t| {
            // SAFETY: fixture pointers refer to the records borrowed for this call.
            unsafe { copy_plan(enabled, addr, w, h, task) }
        };
        // SAFETY: all fixture pointers remain valid for the duration of each call.
        let plan = check(enabled, addr, w, h, &task, &descriptor, &image, &buffer)
            .expect("padded/offset plan");
        assert_eq!(
            (plan.src_off, plan.dst_off, plan.row_len, plan.rows),
            (7, 7, 2, 2)
        );

        image.data_size = 10;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "short source declined before writing"
        );
        image.data_size = 18;
        buffer.data_size = 10;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "short destination declined"
        );
        buffer.data_size = 18;
        descriptor.rotation = 150;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "rotation needs software"
        );
        descriptor.rotation = 0;
        descriptor.opa = 128;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "opacity needs blending"
        );
        descriptor.opa = 255;
        descriptor.clip_radius = 2;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "rounded clipping needs software"
        );
        descriptor.clip_radius = 0;
        image.data = target.as_ptr();
        addr = target.as_ptr() as usize;
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "overlapping allocations declined"
        );
        image.data = source.as_ptr();
        // A tiny string specifically catches accidentally reading an image struct.
        descriptor.src = c"x".as_ptr().cast();
        assert!(
            check(enabled, addr, w, h, &task, &descriptor, &image, &buffer).is_none(),
            "filename never read as metadata"
        );
        assert_eq!(target, [45; 18], "validation never writes partial pixels");
    }
}
