//! Checks the generated environmental face with LVGL's own text metrics.

use core::{ffi::c_void, ptr};

use lightvgl_sys as lv;
use ui_shell_host_harness::ambient_environment_font;

const WIDTH: i32 = 600;
const HEIGHT: i32 = 600;
const PIXELS: usize = (WIDTH * HEIGHT) as usize;
const LVGL_POOL_BYTES: usize = 128 * 1024;
const LETTER_SPACE: i32 = 0;
const LABEL_Y: i32 = 458;

#[repr(align(16))]
struct Aligned<const N: usize>([u8; N]);

static mut LVGL_POOL: Aligned<LVGL_POOL_BYTES> = Aligned([0; LVGL_POOL_BYTES]);
static mut RENDER_BUFFER: Aligned<PIXELS> = Aligned([0; PIXELS]);
static mut CAPTURED: [u8; PIXELS] = [0xff; PIXELS];

#[unsafe(no_mangle)]
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

fn ink_bounds() -> (i32, i32, i32, i32, usize) {
    let captured = unsafe { &*ptr::addr_of!(CAPTURED) };
    let (mut left, mut top, mut right, mut bottom) = (WIDTH, HEIGHT, -1, -1);
    let mut count = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if captured[(y * WIDTH + x) as usize] >= 128 {
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

#[test]
fn the_64_px_environment_face_fits_the_panel() {
    let font = ambient_environment_font::font();
    let mut size = lv::lv_point_t { x: 0, y: 0 };
    unsafe {
        lv::lv_text_get_size(
            &mut size,
            c"26.7 C   29% RH".as_ptr(),
            font,
            LETTER_SPACE,
            0,
            i32::MAX,
            lv::lv_text_flag_t_LV_TEXT_FLAG_NONE,
        );
    }

    assert!(font.line_height >= 64);
    assert!(size.x <= 600, "environment text width is {} px", size.x);
    assert_eq!(size.y, font.line_height);

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
        lv::lv_obj_set_style_bg_color(screen, lv::lv_color_white(), 0);
        lv::lv_obj_set_style_bg_opa(screen, 255, 0);
        let label = lv::lv_label_create(screen);
        lv::lv_obj_set_width(label, WIDTH);
        lv::lv_obj_set_pos(label, 0, LABEL_Y);
        lv::lv_obj_set_style_text_align(label, lv::lv_text_align_t_LV_TEXT_ALIGN_CENTER, 0);
        lv::lv_obj_set_style_text_font(label, font, 0);
        lv::lv_obj_set_style_text_letter_space(label, LETTER_SPACE, 0);
        lv::lv_obj_set_style_text_color(label, lv::lv_color_black(), 0);
        lv::lv_label_set_text(label, c"26.2 C   29% RH".as_ptr());
        lv::lv_refr_now(display);
    }

    let (left, top, right, bottom, count) = ink_bounds();
    assert!(count > 0, "the environment reading rendered nothing");
    assert!(left >= 10, "reading starts too close to the edge: {left}");
    assert!(right < 590, "reading ends too close to the edge: {right}");
    assert!(top >= LABEL_Y, "reading starts above its label: {top}");
    assert!(
        bottom < 590,
        "reading is clipped at the panel bottom: {bottom}"
    );
}
