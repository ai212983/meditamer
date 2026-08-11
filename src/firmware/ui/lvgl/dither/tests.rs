use super::*;

fn area(x1: i32, y1: i32, x2: i32, y2: i32) -> DirtyArea {
    DirtyArea { x1, y1, x2, y2 }
}

#[test]
fn maps_l8_extremes_into_rotated_panel_bytes() {
    let mut framebuffer = [0u8; FRAMEBUFFER_BYTES];
    let mut bitmap = [0u8; 64];
    bitmap[8..16].fill(u8::MAX);
    assert!(blit_l8_slice(
        area(0, 0, 7, 7),
        &bitmap,
        8,
        &mut framebuffer,
    ));
    assert_eq!(framebuffer[ROW_BYTES * 0 + 74], 0xBF);
    assert_eq!(framebuffer[ROW_BYTES * 7 + 74], 0xBF);
}

#[test]
fn preserves_bits_outside_a_partial_vertical_group() {
    let mut framebuffer = [0u8; FRAMEBUFFER_BYTES];
    assert!(blit_l8_slice(
        area(0, 2, 0, 5),
        &[0; 4],
        1,
        &mut framebuffer,
    ));
    assert_eq!(framebuffer[74], 0x3C);
}

#[test]
fn thresholds_l8_luminance() {
    assert!(dithered_black(0));
    assert!(dithered_black(127));
    assert!(!dithered_black(128));
    assert!(!dithered_black(u8::MAX));
}

#[test]
fn solid_rectangle_matches_direct_pixel_mapping() {
    let rectangle = area(210, 42, 389, 105);
    let width = (rectangle.x2 - rectangle.x1 + 1) as usize;
    let height = (rectangle.y2 - rectangle.y1 + 1) as usize;
    let bitmap = vec![0; width * height];
    let mut actual = [0u8; FRAMEBUFFER_BYTES];
    let mut expected = [0u8; FRAMEBUFFER_BYTES];

    assert!(unsafe { blit_l8(rectangle, bitmap.as_ptr(), &mut actual) });
    for x in rectangle.x1 as usize..=rectangle.x2 as usize {
        for y in rectangle.y1 as usize..=rectangle.y2 as usize {
            let panel_x = WIDTH - 1 - y;
            let panel_y = x;
            expected[ROW_BYTES * panel_y + panel_x / 8] |= 1 << (panel_x % 8);
        }
    }

    assert_eq!(actual, expected);
}

#[test]
fn unions_flush_regions() {
    assert_eq!(
        area(10, 20, 30, 40).union(area(5, 25, 35, 38)),
        area(5, 20, 35, 40)
    );
}
