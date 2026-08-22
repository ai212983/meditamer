//! Renders the compiled Ambient Home clock face through real LVGL.
//!
//! The face is generated (`tools/lvgl_font_compiler`) rather than one of
//! LVGL's built-in tables, so nothing but a render proves the glyph map,
//! bitmap packing, and metrics agree with what `lv_font_fmt_txt` expects.
//!
//! This is an integration test, and so its own process: the harness's LVGL is
//! global state, and `lv_init` may only run once per process.

use core::{ffi::c_void, ptr};

use lightvgl_sys as lv;
use ui_shell_host_harness::ambient_clock_font;

const WIDTH: i32 = 600;
const HEIGHT: i32 = 220;
const PIXELS: usize = (WIDTH * HEIGHT) as usize;
const LVGL_POOL_BYTES: usize = 128 * 1024;

/// Text origin, chosen so a 128 px line fits the surface with room to spare.
const TEXT_X: i32 = 20;
const TEXT_Y: i32 = 20;

/// L8 luminance below this reads as black, matching the firmware's flush path
/// (`src/firmware/ui/lvgl/dither.rs`).
const INK_THRESHOLD: u8 = 128;

#[repr(align(16))]
struct AlignedPool([u8; LVGL_POOL_BYTES]);

static mut LVGL_POOL: AlignedPool = AlignedPool([0; LVGL_POOL_BYTES]);
static mut RENDER_BUFFER: [u8; PIXELS] = [0; PIXELS];
static mut CAPTURED: [u8; PIXELS] = [0xff; PIXELS];

#[no_mangle]
extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut c_void {
    if size > LVGL_POOL_BYTES {
        return ptr::null_mut();
    }
    unsafe { ptr::addr_of_mut!(LVGL_POOL.0).cast() }
}

unsafe extern "C" fn flush(
    display: *mut lv::lv_display_t,
    area: *const lv::lv_area_t,
    pixels: *mut u8,
) {
    unsafe {
        let area = *area;
        let width = area.x2 - area.x1 + 1;
        let stride =
            lv::lv_draw_buf_width_to_stride(width as u32, lv::lv_color_format_t_LV_COLOR_FORMAT_L8)
                as usize;
        let captured = ptr::addr_of_mut!(CAPTURED).cast::<u8>();
        for row in 0..(area.y2 - area.y1 + 1) {
            let source = pixels.add(row as usize * stride);
            let target = captured.add(((area.y1 + row) * WIDTH + area.x1) as usize);
            ptr::copy_nonoverlapping(source, target, width as usize);
        }
        lv::lv_display_flush_ready(display);
    }
}

/// The bounding box of every pixel dark enough to reach the panel as black,
/// as `(left, top, right, bottom, count)`.
fn ink_bounds() -> (i32, i32, i32, i32, usize) {
    let captured = unsafe { &*ptr::addr_of!(CAPTURED) };
    let (mut left, mut top, mut right, mut bottom) = (WIDTH, HEIGHT, -1, -1);
    let mut count = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if captured[(y * WIDTH + x) as usize] >= INK_THRESHOLD {
                continue;
            }
            count += 1;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    (left, top, right, bottom, count)
}

unsafe fn render(text: &core::ffi::CStr) {
    unsafe {
        lv::lv_init();
        let display = lv::lv_display_create(WIDTH, HEIGHT);
        assert!(!display.is_null());
        lv::lv_display_set_color_format(display, lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
        lv::lv_display_set_buffers(
            display,
            ptr::addr_of_mut!(RENDER_BUFFER).cast(),
            ptr::null_mut(),
            PIXELS as u32,
            lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        lv::lv_display_set_flush_cb(display, Some(flush));

        let screen = lv::lv_screen_active();
        assert!(!screen.is_null());
        lv::lv_obj_set_style_bg_color(screen, lv::lv_color_white(), 0);
        lv::lv_obj_set_style_bg_opa(screen, 255, 0);

        let label = lv::lv_label_create(screen);
        assert!(!label.is_null());
        lv::lv_obj_set_style_text_font(label, ambient_clock_font::font(), 0);
        lv::lv_obj_set_style_text_color(label, lv::lv_color_black(), 0);
        lv::lv_label_set_text(label, text.as_ptr());
        lv::lv_obj_set_pos(label, TEXT_X, TEXT_Y);

        lv::lv_refr_now(display);
    }
}

#[test]
fn the_compiled_face_renders_a_128_px_clock() {
    let font = ambient_clock_font::font();

    // The table LVGL will read, before any drawing: a 128 px face whose digits
    // advance ~77 px and stand ~93 px tall.
    let mut glyph = unsafe { core::mem::zeroed::<lv::lv_font_glyph_dsc_t>() };
    glyph.resolved_font = font;
    assert!(unsafe { lv::lv_font_get_glyph_dsc_fmt_txt(font, &mut glyph, u32::from('5'), 0) });
    assert_eq!(glyph.adv_w, 77);
    assert_eq!(glyph.box_h, 92);
    assert_eq!(unsafe { (*font).line_height }, 166);
    assert_eq!(unsafe { (*font).base_line }, 35);

    // A code point outside the compiled ranges has no glyph.
    assert!(!unsafe { lv::lv_font_get_glyph_dsc_fmt_txt(font, &mut glyph, u32::from('A'), 0) });

    unsafe { render(c"12:34") };

    let (left, top, right, bottom, count) = ink_bounds();
    assert!(count > 0, "the clock rendered nothing");

    // '1' begins one bearing in from the text origin; '4' ends after four
    // digit advances plus a colon.
    assert!(
        (TEXT_X..TEXT_X + 16).contains(&left),
        "unexpected left edge {left}"
    );
    assert!(
        (330..380).contains(&(right - left)),
        "unexpected ink width {}",
        right - left
    );
    // Digits sit on the baseline at `line_height - base_line` below the text
    // origin and rise ~93 px above it.
    assert!(
        (85..100).contains(&(bottom - top)),
        "unexpected ink height {}",
        bottom - top
    );
    assert_eq!(top, TEXT_Y + 40, "unexpected ink top {top}");
}
