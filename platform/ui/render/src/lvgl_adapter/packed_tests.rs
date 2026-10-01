//! Pixel-level checks run inside the sole LVGL runtime test.

use super::*;

const WIDTH: usize = 13;
const HEIGHT: usize = 3;
const DISPLAY_WIDTH: usize = 32;
const DISPLAY_HEIGHT: usize = 8;
static PACKED: StaticI1CanvasBuffer<{ i1_canvas_buffer_bytes(WIDTH, HEIGHT) }> =
    StaticI1CanvasBuffer::new(WIDTH, HEIGHT, MonochromePixel::Paper);
static TWO_BIT: StaticIndexedCanvasBuffer<2, { indexed_canvas_buffer_bytes(WIDTH, HEIGHT, 2) }> =
    StaticIndexedCanvasBuffer::new(
        WIDTH,
        HEIGHT,
        &[
            PaletteColor::rgb(0, 0, 0),
            PaletteColor::rgb(127, 127, 127),
            PaletteColor::rgb(255, 255, 255),
        ],
        0,
    );
static FOUR_BIT: StaticIndexedCanvasBuffer<4, { indexed_canvas_buffer_bytes(WIDTH, HEIGHT, 4) }> =
    StaticIndexedCanvasBuffer::new(
        WIDTH,
        HEIGHT,
        &[
            PaletteColor::rgb(0, 0, 0),
            PaletteColor::rgb(32, 32, 32),
            PaletteColor::rgb(64, 64, 64),
            PaletteColor::rgb(96, 96, 96),
            PaletteColor::rgb(128, 128, 128),
            PaletteColor::rgb(160, 160, 160),
            PaletteColor::rgb(192, 192, 192),
            PaletteColor::rgb(255, 255, 255),
        ],
        0,
    );
static EIGHT_BIT: StaticIndexedCanvasBuffer<8, { indexed_canvas_buffer_bytes(WIDTH, HEIGHT, 8) }> =
    StaticIndexedCanvasBuffer::new(
        WIDTH,
        HEIGHT,
        &[
            PaletteColor::rgb(255, 0, 0),
            PaletteColor::rgb(0, 255, 0),
            PaletteColor::rgb(0, 0, 255),
            PaletteColor::rgba(0, 0, 0, 0),
        ],
        0,
    );
static REFERENCE: StaticL8CanvasBuffer<{ WIDTH * HEIGHT }> =
    StaticL8CanvasBuffer::new([255; WIDTH * HEIGHT]);
static DRAW: StaticL8DrawBuffer<16> = StaticL8DrawBuffer::new();

fn copy_pixels(area: crate::DirtyArea, pixels: &[u8], target: &mut [u8]) -> bool {
    let width = (area.x2 - area.x1 + 1) as usize;
    for (row, source) in pixels.chunks_exact(width).enumerate() {
        let start = (area.y1 as usize + row) * DISPLAY_WIDTH + area.x1 as usize;
        target[start..start + width].copy_from_slice(source);
    }
    true
}

fn refresh(token: &UiAccessToken) -> [u8; DISPLAY_WIDTH * DISPLAY_HEIGHT] {
    let mut pixels = [127; DISPLAY_WIDTH * DISPLAY_HEIGHT];
    let frame = token.publish_l8_frame(&mut pixels, copy_pixels).unwrap();
    token.invalidate_active_screen().unwrap();
    assert!(token.refresh_default_display().unwrap());
    frame.finish();
    pixels
}

pub(super) fn render_packed_canvas(session: &RuntimeSession, token: &UiAccessToken) {
    assert_eq!(i1_canvas_buffer_bytes(WIDTH, HEIGHT), 14);
    let display = session
        .create_l8_display(DISPLAY_WIDTH as i32, DISPLAY_HEIGHT as i32, &DRAW, 64)
        .unwrap();
    display.make_default(token).unwrap();
    let screen = Widget::screen(token).unwrap();
    screen.remove_style_all(token).unwrap();
    screen
        .set_bg_color(token, white(), StyleState::Default)
        .unwrap();
    screen.set_bg_opa(token, 255, StyleState::Default).unwrap();
    assert_eq!(
        screen.set_i1_canvas_buffer(token, &PACKED),
        Err(RetainedResourceError::Access(AccessFault::WrongKind))
    );
    let packed = screen.child(token, WidgetKind::Canvas).unwrap();
    packed.remove_style_all(token).unwrap();
    packed.set_i1_canvas_buffer(token, &PACKED).unwrap();
    packed.set_pos(token, 0, 0).unwrap();
    let reference = screen.child(token, WidgetKind::Canvas).unwrap();
    reference.remove_style_all(token).unwrap();
    reference
        .set_l8_canvas_buffer(token, &REFERENCE, WIDTH as i32, HEIGHT as i32)
        .unwrap();
    reference.set_pos(token, 16, 0).unwrap();
    PACKED
        .with_writer(token, |writer| {
            assert!(!writer.set(WIDTH, 0, MonochromePixel::Ink));
            assert!(!writer.set(0, HEIGHT, MonochromePixel::Ink));
            assert!(!writer.set(usize::MAX, usize::MAX, MonochromePixel::Ink));
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let pixel = if (x + y) % 3 == 0 {
                        MonochromePixel::Ink
                    } else {
                        MonochromePixel::Paper
                    };
                    assert!(writer.set(x, y, pixel));
                }
            }
        })
        .unwrap();
    REFERENCE
        .with_writer(token, |writer| {
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    assert!(writer.set(y * WIDTH + x, if (x + y) % 3 == 0 { 0 } else { 255 }));
                }
            }
        })
        .unwrap();
    assert!(screen.activate(token).unwrap());
    let pixels = refresh(token);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let actual = pixels[y * DISPLAY_WIDTH + x];
            assert_eq!(
                actual,
                pixels[y * DISPLAY_WIDTH + 16 + x],
                "I1/L8 mismatch at {x},{y}"
            );
            assert_eq!(
                actual,
                if (x + y) % 3 == 0 { 0 } else { 255 },
                "pixel {x},{y}"
            );
        }
    }
    let baseline = token.memory_snapshot().unwrap();
    for round in 0..20 {
        let (pixel, expected) = if round % 2 == 0 {
            (MonochromePixel::Ink, 0)
        } else {
            (MonochromePixel::Paper, 255)
        };
        PACKED
            .with_writer(token, |writer| writer.fill(pixel))
            .unwrap();
        packed.invalidate(token).unwrap();
        let pixels = refresh(token);
        for y in 0..HEIGHT {
            assert!(pixels[y * DISPLAY_WIDTH..y * DISPLAY_WIDTH + WIDTH]
                .iter()
                .all(|&value| value == expected));
        }
        let memory = token.memory_snapshot().unwrap();
        assert!(memory.integrity_ok);
        assert_eq!(
            memory.free_size, baseline.free_size,
            "indexed row decoding must release its temporary storage"
        );
    }
    packed.set_indexed_canvas_buffer(token, &TWO_BIT).unwrap();
    TWO_BIT
        .with_writer(token, |writer| {
            assert!(!writer.set(0, 0, 3));
            assert!(!writer.fill(3));
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    assert!(writer.set(x, y, ((x + y) % 3) as u8));
                }
            }
        })
        .unwrap();
    packed.invalidate(token).unwrap();
    let pixels = refresh(token);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(pixels[y * DISPLAY_WIDTH + x], [0, 127, 255][(x + y) % 3]);
        }
    }
    packed.set_indexed_canvas_buffer(token, &FOUR_BIT).unwrap();
    FOUR_BIT
        .with_writer(token, |writer| {
            assert!(!writer.set(0, 0, 8));
            assert!(!writer.fill(8));
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    assert!(writer.set(x, y, ((x + y) % 8) as u8));
                }
            }
        })
        .unwrap();
    packed.invalidate(token).unwrap();
    let pixels = refresh(token);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(
                pixels[y * DISPLAY_WIDTH + x],
                [0, 32, 64, 96, 128, 160, 192, 255][(x + y) % 8]
            );
        }
    }
    packed.set_indexed_canvas_buffer(token, &EIGHT_BIT).unwrap();
    EIGHT_BIT
        .with_writer(token, |writer| {
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    assert!(writer.set(x, y, ((x + y) % 4) as u8));
                }
            }
        })
        .unwrap();
    packed.invalidate(token).unwrap();
    let pixels = refresh(token);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(
                pixels[y * DISPLAY_WIDTH + x],
                [76, 150, 27, 255][(x + y) % 4],
                "palette channels/alpha at {x},{y}"
            );
        }
    }
    screen.delete(token).unwrap();
}

pub(super) fn reject_stale_writer(token: &UiAccessToken) {
    assert_eq!(
        PACKED.with_writer(token, |_| panic!("stale token entered writer")),
        Err(AccessFault::WrongRuntime)
    );
}

/// A writer already admitted by an old epoch must stop after re-entry replaces it.
pub(super) fn reject_expired_writers_inside_closures(token: &UiAccessToken) {
    PACKED
        .with_writer(token, |mono| {
            REFERENCE
                .with_writer(token, |luma| {
                    ARC_POINTS
                        .with_writer(token, |line| {
                            FOUR_BIT
                                .with_writer(token, |indexed| {
                                    assert!(mono.set(0, 0, MonochromePixel::Ink));
                                    assert!(luma.set(0, 0));
                                    assert!(line.set(0, LinePoint::new(0.0, 0.0)));
                                    assert!(indexed.set(0, 0, 0));
                                    let _replacement_epoch = UiAccessToken::issue();
                                    assert!(!mono.fill(MonochromePixel::Paper));
                                    assert!(!mono.set(0, 0, MonochromePixel::Paper));
                                    assert!(!luma.fill(255));
                                    assert!(!luma.set(0, 255));
                                    assert!(!line.set(0, LinePoint::new(1.0, 1.0)));
                                    assert!(!indexed.fill(1));
                                    assert!(!indexed.set(0, 0, 1));
                                })
                                .unwrap();
                        })
                        .unwrap();
                })
                .unwrap();
        })
        .unwrap();
}

#[test]
fn indexed_layout_sizes_cover_native_depths_and_header_limits() {
    for (bits, palette, stride) in [(1, 8, 2), (2, 16, 4), (4, 64, 7), (8, 1024, 13)] {
        assert_eq!(
            indexed_canvas_buffer_bytes(13, 3, bits),
            palette + 3 * stride
        );
        assert_eq!(indexed_canvas_buffer_bytes(1, 1, bits), palette + 1);
    }
    assert_eq!(indexed_canvas_buffer_bytes(65535, 65535, 8), 4_294_837_249);
    let wide = StaticIndexedCanvasBuffer::<8, { 65535 + 1024 }>::new(
        65535,
        1,
        &[PaletteColor::rgb(0, 0, 0)],
        0,
    );
    assert_eq!(wide.width(), 65535);
    assert_eq!(wide.height(), 1);
    assert_eq!(wide.stride_bytes(), 65535);
    assert_eq!(wide.capacity_bytes(), 66559);
}

#[test]
fn indexed_layout_rejects_invalid_geometry_and_depth() {
    for (width, height, bits) in [
        (0, 1, 1),
        (1, 0, 1),
        (65536, 1, 1),
        (1, 65536, 1),
        (usize::MAX, 1, 8),
        (1, usize::MAX, 8),
        (1, 1, 0),
        (1, 1, 3),
        (1, 1, 16),
        (1, 1, usize::MAX),
    ] {
        assert!(
            std::panic::catch_unwind(|| indexed_canvas_buffer_bytes(width, height, bits)).is_err()
        );
    }
}

#[test]
fn indexed_constructor_rejects_invalid_palette_initial_index_and_capacity() {
    let black = PaletteColor::rgb(0, 0, 0);
    assert!(
        std::panic::catch_unwind(|| StaticIndexedCanvasBuffer::<1, 9>::new(1, 1, &[], 0)).is_err()
    );
    assert!(
        std::panic::catch_unwind(|| StaticIndexedCanvasBuffer::<1, 9>::new(1, 1, &[black; 3], 0))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| StaticIndexedCanvasBuffer::<1, 9>::new(1, 1, &[black], 1))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| StaticIndexedCanvasBuffer::<1, 8>::new(1, 1, &[black], 0))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| StaticIndexedCanvasBuffer::<1, 10>::new(1, 1, &[black], 0))
            .is_err()
    );
}

#[test]
fn retained_writers_stay_on_the_ui_thread() {
    // If any writer gains Send or Sync, inference becomes ambiguous and this
    // test stops compiling. The token must not authorize cross-thread writes.
    trait AmbiguousIfThreadSafe<Marker> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfThreadSafe<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfThreadSafe<u8> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfThreadSafe<u16> for T {}
    let _ = <IndexedCanvasWriter<'static, 4, 64> as AmbiguousIfThreadSafe<_>>::check;
    let _ = <I1CanvasWriter<'static, 64> as AmbiguousIfThreadSafe<_>>::check;
    let _ = <L8CanvasWriter<'static, 64> as AmbiguousIfThreadSafe<_>>::check;
    let _ = <LinePointsWriter<'static, 3> as AmbiguousIfThreadSafe<_>>::check;
}
