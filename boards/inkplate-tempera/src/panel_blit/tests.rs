// This crate is `#![no_std]` for the embedded build, but `cargo test` always
// runs on a host target with `std` available -- borrow just `vec!` from it.
extern crate std;
use std::vec;

use super::*;

fn area(x1: i32, y1: i32, x2: i32, y2: i32) -> DirtyArea {
    DirtyArea { x1, y1, x2, y2 }
}

/// Byte index of `column` within panel `row`.
fn at(row: usize, column: usize) -> usize {
    ROW_BYTES * row + column
}

#[test]
fn maps_l8_extremes_into_rotated_panel_bytes() {
    let mut framebuffer = [0u8; FRAMEBUFFER_BYTES];
    let mut bitmap = [0u8; 64];
    bitmap[8..16].fill(u8::MAX);
    assert!(blit_l8(area(0, 0, 7, 7), &bitmap, &mut framebuffer));
    assert_eq!(framebuffer[at(0, 74)], 0xBF);
    assert_eq!(framebuffer[at(7, 74)], 0xBF);
}

#[test]
fn preserves_bits_outside_a_partial_vertical_group() {
    let mut framebuffer = [0u8; FRAMEBUFFER_BYTES];
    assert!(blit_l8(
        area(0, 2, 0, 5),
        [0; 4].as_slice(),
        &mut framebuffer
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

    assert!(blit_l8(rectangle, &bitmap, &mut actual));
    for x in rectangle.x1 as usize..=rectangle.x2 as usize {
        for y in rectangle.y1 as usize..=rectangle.y2 as usize {
            let panel_x = WIDTH - 1 - y;
            let panel_y = x;
            expected[at(panel_y, panel_x / 8)] |= 1 << (panel_x % 8);
        }
    }

    assert_eq!(actual, expected);
}

#[test]
fn clips_a_region_straddling_the_panel_edge_to_what_fits() {
    // Same shape as the Waveshare board's own edge test: a rectangle that
    // starts inside the panel and runs off the right edge must paint only the
    // in-bounds portion rather than reject the whole blit or index out of
    // range.
    let rectangle = area(WIDTH as i32 - 10, 270, WIDTH as i32 + 29, 289);
    let width = (rectangle.x2 - rectangle.x1 + 1) as usize;
    let height = (rectangle.y2 - rectangle.y1 + 1) as usize;
    let bitmap = vec![0; width * height];
    let mut actual = [0u8; FRAMEBUFFER_BYTES];
    let mut expected = [0u8; FRAMEBUFFER_BYTES];

    assert!(blit_l8(rectangle, &bitmap, &mut actual));
    for x in rectangle.x1 as usize..WIDTH {
        for y in rectangle.y1 as usize..=rectangle.y2 as usize {
            let panel_x = WIDTH - 1 - y;
            let panel_y = x;
            expected[at(panel_y, panel_x / 8)] |= 1 << (panel_x % 8);
        }
    }

    assert_eq!(actual, expected);
}

#[test]
fn rejects_an_empty_region_without_touching_the_framebuffer() {
    let mut framebuffer = [0xAAu8; FRAMEBUFFER_BYTES];
    let before = framebuffer;

    // Inverted corners: x2 < x1.
    assert!(!blit_l8(area(10, 10, 5, 15), &[0; 64], &mut framebuffer));
    assert_eq!(framebuffer, before);

    // Zero-height: y2 < y1.
    assert!(!blit_l8(area(10, 15, 20, 5), &[0; 64], &mut framebuffer));
    assert_eq!(framebuffer, before);
}

#[test]
fn rejects_a_region_entirely_outside_the_panel() {
    let mut framebuffer = [0xAAu8; FRAMEBUFFER_BYTES];
    let before = framebuffer;

    assert!(!blit_l8(
        area(
            WIDTH as i32,
            HEIGHT as i32,
            WIDTH as i32 + 49,
            HEIGHT as i32 + 49,
        ),
        &[0; 2_500],
        &mut framebuffer,
    ));
    assert_eq!(framebuffer, before);
}

#[test]
fn rejects_a_short_pixel_slice_without_touching_the_framebuffer() {
    let mut framebuffer = [0xAAu8; FRAMEBUFFER_BYTES];
    let before = framebuffer;

    assert!(!blit_l8(area(0, 0, 9, 9), &[0; 99], &mut framebuffer));
    assert_eq!(framebuffer, before);
}

#[test]
fn rejects_extreme_coordinates_without_overflow_or_writes() {
    let mut framebuffer = [0xAAu8; FRAMEBUFFER_BYTES];
    let before = framebuffer;

    assert!(!blit_l8(
        area(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
        &[],
        &mut framebuffer,
    ));
    assert_eq!(framebuffer, before);
}

#[test]
fn unions_flush_regions() {
    assert_eq!(
        area(10, 20, 30, 40).union(area(5, 25, 35, 38)),
        area(5, 20, 35, 40)
    );
}

/// `blit_l8_gray4`'s column-major layout is derived by analogy with
/// `blit_l8`. This pins it against an independent construction: writing the
/// same image row-major in `frame`'s authoring convention and rotating it
/// with `frame::rotate_gray4` -- the pair that was verified on the physical
/// panel. If the two disagree, one of them is wrong and grey frames would
/// come out transposed or nibble-swapped, which no runtime check would catch.
#[test]
fn gray4_blit_matches_the_hardware_verified_rotation_path() {
    use crate::frame::{level_from_luminance, rotate_gray4, GRAY4_ROW_BYTES};
    use crate::{E_INK_HEIGHT, E_INK_WIDTH, GRAYSCALE_FRAMEBUFFER_BYTES};

    // Asymmetric in both axes and in value, so a transpose, a flip or a
    // nibble swap all show up as a mismatch.
    let mut pixels = vec![0u8; E_INK_WIDTH * E_INK_HEIGHT];
    for y in 0..E_INK_HEIGHT {
        for x in 0..E_INK_WIDTH {
            pixels[y * E_INK_WIDTH + x] = ((x * 7 + y * 3) % 256) as u8;
        }
    }

    let area = DirtyArea {
        x1: 0,
        y1: 0,
        x2: E_INK_WIDTH as i32 - 1,
        y2: E_INK_HEIGHT as i32 - 1,
    };
    let mut direct = vec![0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
    assert!(blit_l8_gray4(area, &pixels, &mut direct));

    // Independent path: author row-major the way the host emitter does, then
    // rotate into the panel's axes.
    let mut authored = vec![0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
    for y in 0..E_INK_HEIGHT {
        for x in 0..E_INK_WIDTH {
            let level = level_from_luminance(pixels[y * E_INK_WIDTH + x]);
            let slot = &mut authored[y * GRAY4_ROW_BYTES + x / 2];
            if x % 2 == 0 {
                *slot = (*slot & 0x0f) | (level << 4);
            } else {
                *slot = (*slot & 0xf0) | (level & 0x0f);
            }
        }
    }
    let mut rotated = vec![0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
    assert!(rotate_gray4(&authored, &mut rotated));

    assert_eq!(
        direct, rotated,
        "direct Gray4 blit disagrees with the rotation path verified on hardware"
    );
}

#[test]
fn gray4_blit_rejects_a_wrong_sized_framebuffer() {
    let area = DirtyArea {
        x1: 0,
        y1: 0,
        x2: 1,
        y2: 1,
    };
    let pixels = [0u8; 4];
    let mut too_small = [0u8; 16];
    assert!(!blit_l8_gray4(area, &pixels, &mut too_small));
}
