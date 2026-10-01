use super::*;
use std::vec::Vec;

fn flat_normal(bufs: &mut [u8]) {
    for px in bufs.chunks_exact_mut(3) {
        px[0] = 127;
        px[1] = 127;
        px[2] = 255;
    }
}

/// Hour then minute planes: albedo, alpha, normal, spec each.
type HandBufs = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

fn test_hands(size: u16) -> HandBufs {
    let n = size as usize * size as usize;
    let (ha, hal, mut hn, hs) = (
        std::vec![128u8; n * 3],
        std::vec![255u8; n],
        std::vec![0u8; n * 3],
        std::vec![128u8; n],
    );
    let (ma, mal, mut mn, ms) = (
        std::vec![128u8; n * 3],
        std::vec![255u8; n],
        std::vec![0u8; n * 3],
        std::vec![128u8; n],
    );
    flat_normal(&mut hn);
    flat_normal(&mut mn);
    (ha, hal, hn, hs, ma, mal, mn, ms)
}

fn maps<'a>(
    w: u16,
    albedo: &'a [u8],
    alpha: &'a [u8],
    normal: &'a [u8],
    spec: &'a [u8],
) -> super::super::assets::HandMaps<'a> {
    super::super::assets::HandMaps {
        width: w,
        height: w,
        albedo,
        alpha,
        normal,
        spec,
        pivot_x: w as f32 / 2.0,
        pivot_y: w as f32 / 2.0,
    }
}

#[test]
fn hand_scale_follows_the_map_pivot() {
    // The maps own the only pivot: same scene, different map pivots must
    // draw at different scales. Minute is transparent so the pixel shows
    // hour-or-dial: dial x ~0.5 sits on the hand at small scale but past
    // its edge at large scale.
    let (ha, hal, hn, hs, ma, _, mn, ms) = test_hands(8);
    let clear: Vec<u8> = std::vec![0u8; 64];
    let scene = ClockScene::for_size(64, 64);
    let near = Hands {
        hour: super::super::assets::HandMaps {
            pivot_y: 2.0,
            ..maps(8, &ha, &hal, &hn, &hs)
        },
        minute: maps(8, &ma, &clear, &mn, &ms),
    };
    let far = Hands {
        hour: super::super::assets::HandMaps {
            pivot_y: 6.0,
            ..maps(8, &ha, &hal, &hn, &hs)
        },
        minute: maps(8, &ma, &clear, &mn, &ms),
    };
    let a = pixel_linear(&scene, &near, true, 47, 32);
    let b = pixel_linear(&scene, &far, true, 47, 32);
    assert_ne!(a, b, "map pivot change had no effect");
}

#[test]
fn sprite_mapping_is_identity_at_zero_angle() {
    let (ha, hal, hn, hs, _, _, _, _) = test_hands(4);
    let hand = maps(4, &ha, &hal, &hn, &hs);
    // Dial point (1, 0) with scale 2 sprite-px per unit lands at
    // pivot + (2, 0).
    let (fx, fy) = to_sprite_with(
        hand.pivot_x,
        hand.pivot_y,
        libm::sinf(0.0),
        libm::cosf(0.0),
        2.0,
        1.0,
        0.0,
    );
    assert!((fx - 4.0).abs() < 1e-5 && (fy - 2.0).abs() < 1e-5);
}

#[test]
fn higher_hand_is_visible_when_heights_are_reversed() {
    let (mut ha, hal, hn, hs, mut ma, mal, mn, ms) = test_hands(8);
    for pixel in ha.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[255, 0, 0]);
    }
    for pixel in ma.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[0, 0, 255]);
    }
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut scene = ClockScene::for_size(64, 64);
    scene.spec_strength = 0.0;
    for hour_above in [false, true] {
        scene.hour_h = if hour_above { 0.2 } else { 0.05 };
        scene.minute_h = if hour_above { 0.05 } else { 0.2 };
        let (red, _, blue) = pixel_linear(&scene, &hands, true, 32, 32);
        assert_eq!(red > blue, hour_above);
    }
}

#[test]
fn per_hand_spec_gain_tunes_one_hand_only() {
    let (ha, hal, hn, hs, ma, mut mal, mn, ms) = test_hands(8);
    // Minute hand fully transparent: the center pixel shows hour only.
    for a in mal.iter_mut() {
        *a = 0;
    }
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut scene = ClockScene::for_size(64, 64);
    scene.hour_spec = 0.0;
    let muted = pixel_linear(&scene, &hands, true, 32, 32);
    scene.hour_spec = 1.0;
    let full = pixel_linear(&scene, &hands, true, 32, 32);
    assert!(
        full.0 > muted.0,
        "hour gain had no effect: {full:?} vs {muted:?}"
    );
    // The hidden minute hand's gain must not move an hour-only pixel.
    scene.minute_spec = 0.0;
    let minute_muted = pixel_linear(&scene, &hands, true, 32, 32);
    assert_eq!(
        minute_muted, full,
        "minute gain leaked into hour: {minute_muted:?}"
    );
}

#[test]
fn shadow_removes_specular_but_preserves_ambient() {
    let scene = ClockScene::for_size(64, 64);
    let point = SurfacePoint {
        albedo: (0.2, 0.2, 0.2),
        normal: (0.0, 0.0, 1.0),
        spec_map: 1.0,
        pos: Point {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    };
    let shadow = shade(&scene, &point, 0.0);
    let mut no_spec = scene;
    no_spec.spec_strength = 0.0;
    assert_eq!(shadow, shade(&no_spec, &point, 0.0));
    assert!(shadow.0 > 0.0);
    assert!(shade(&scene, &point, 1.0).0 > shade(&no_spec, &point, 1.0).0);
}

#[test]
fn quarter_turn_moves_up_to_the_right() {
    let (ha, hal, hn, hs, _, _, _, _) = test_hands(4);
    let hand = maps(4, &ha, &hal, &hn, &hs);
    // The sprite's up direction (0, -1) rotated 90 degrees clockwise
    // must read dial point (1, 0): inverse-map (1,0) at 90 degrees and
    // expect sprite offset (0, -1) * scale.
    let a = core::f32::consts::FRAC_PI_2;
    let (fx, fy) = to_sprite_with(
        hand.pivot_x,
        hand.pivot_y,
        libm::sinf(a),
        libm::cosf(a),
        2.0,
        1.0,
        0.0,
    );
    assert!((fx - 2.0).abs() < 1e-4 && (fy - 0.0).abs() < 1e-4);
}

#[test]
fn rotated_normal_follows_the_sprite() {
    // +X normal turned 90 degrees clockwise (y down) points down-screen.
    let a = core::f32::consts::FRAC_PI_2;
    let (nx, ny) = rotate_normal_with(1.0, 0.0, libm::sinf(a), libm::cosf(a));
    assert!(nx.abs() < 1e-5 && (ny - 1.0).abs() < 1e-5);
}

#[test]
fn duplicate_blockers_do_not_double_darken() {
    // Two identical fully-opaque blockers must give exactly the single-
    // blocker visibility: union, not multiplied opacity.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let both = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let empty_alpha: Vec<u8> = std::vec![0u8; 64];
    let single = Hands {
        hour: maps(8, &ha, &empty_alpha, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut scene = ClockScene::for_size(32, 32);
    scene.center_x = 16.0;
    scene.center_y = 16.0;
    scene.radius = 16.0;
    scene.hour_len = 2.0;
    scene.minute_len = 2.0;
    scene.hour_h = 0.05;
    scene.minute_h = 0.05;
    scene.light_x = 0.8;
    scene.light_y = -0.6;
    scene.light_h = 0.8;
    scene.light_size = 0.3;
    scene.samples = 12;
    let scene = scene.clamped();
    let prep_both = Prepared::for_frame(&scene, &both);
    let prep_single = Prepared::for_frame(&scene, &single);
    // Receiver on the dial just outside the silhouette edge: probe a
    // spread of pixels and require exact equality everywhere.
    for y in 0..32 {
        for x in 0..32 {
            let r = scene.radius.max(1e-6);
            let px = (x as f32 - scene.center_x) / r;
            let py = (y as f32 - scene.center_y) / r;
            let a = visibility(&scene, &both, &prep_both, px, py, 0.0);
            let b = visibility(&scene, &single, &prep_single, px, py, 0.0);
            assert!(
                (a - b).abs() < 1e-6,
                "double-darken at ({x},{y}): {a} vs {b}"
            );
        }
    }
}

#[test]
fn lower_hand_never_shadows_upper() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut scene = ClockScene::for_size(16, 16);
    scene.hour_h = 0.05;
    scene.minute_h = 0.10;
    scene.light_x = 0.7;
    scene.light_size = 0.2;
    scene.samples = 16;
    let scene = scene.clamped();
    let prep = Prepared::for_frame(&scene, &hands);
    // On top of the minute hand the hour blocker sits below: full light.
    for (px, py) in [(0.0, 0.0), (0.3, -0.2), (-0.4, 0.3)] {
        let v = visibility(&scene, &hands, &prep, px, py, scene.minute_h);
        assert!((v - 1.0).abs() < 1e-6, "upper receiver shadowed: {v}");
    }
    // The same rays onto the dial are at least partly blocked.
    let v = visibility(&scene, &hands, &prep, 0.0, 0.0, 0.0);
    assert!(v < 1.0, "dial should see shadow, got {v}");
}

#[test]
fn rendering_is_deterministic() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut scene = ClockScene::for_size(48, 32);
    scene.hour_angle = 1.1;
    scene.minute_angle = 2.2;
    let scene = scene.clamped();
    let mut row_a = std::vec![0u8; 48];
    let mut row_b = std::vec![0u8; 48];
    for y in 0..32 {
        render_gray_row(&scene, &hands, y, &mut row_a);
        render_gray_row(&scene, &hands, y, &mut row_b);
        assert_eq!(row_a, row_b, "row {y} differs between runs");
    }
}

#[test]
fn clipping_never_panics_and_stays_paper() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Hands far smaller than the dial: corners see dial paper only.
    let scene = ClockScene::for_size(64, 64).clamped();
    let corner = evaluate_pixel(&scene, &hands, 0, 0);
    let edge = evaluate_pixel(&scene, &hands, 63, 63);
    assert!(corner > 0.5 && edge > 0.5);
    // Out-of-range rows are a silent no-op, short buffers clip.
    let mut tiny = [0u8; 4];
    render_gray_row(&scene, &hands, 9999, &mut tiny);
    assert_eq!(tiny, [0; 4]);
    render_gray_row(&scene, &hands, 0, &mut tiny);
    assert!(tiny.iter().all(|v| *v > 200), "row 0 is paper: {tiny:?}");
}

#[test]
fn invalid_buffers_fall_back_to_dial() {
    let empty: Vec<u8> = std::vec![];
    let bad = Hands {
        hour: super::super::assets::HandMaps {
            width: 8,
            height: 8,
            albedo: &empty,
            alpha: &empty,
            normal: &empty,
            spec: &empty,
            pivot_x: 4.0,
            pivot_y: 4.0,
        },
        minute: super::super::assets::HandMaps {
            width: 8,
            height: 8,
            albedo: &empty,
            alpha: &empty,
            normal: &empty,
            spec: &empty,
            pivot_x: 4.0,
            pivot_y: 4.0,
        },
    };
    let scene = ClockScene::for_size(32, 32).clamped();
    // Renders without touching the empty slices, and the hub reads
    // distinct from the surrounding paper (glossy cap under the light).
    let hub = evaluate_pixel(&scene, &bad, 16, 16);
    let paper = evaluate_pixel(&scene, &bad, 2, 2);
    assert!((hub - paper).abs() > 0.02, "hub {hub} vs paper {paper}");
}

#[test]
fn dithered_surface_covers_expected_area() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let scene = ClockScene::for_size(32, 32).clamped();
    let mut bits = std::vec![0u8; raster::BitCanvas::bytes_for(32, 32)];
    let mut canvas = raster::BitCanvas::new(32, 32, &mut bits).expect("canvas");
    render_surface(&scene, &hands, Dither::None, &mut canvas);
    // Mostly paper: only ticks, hub, and the small test hands ink.
    let ink = canvas.ink_count();
    assert!(ink > 0 && ink < 32 * 32 / 2, "unexpected ink {ink}");
}

#[test]
fn srgb_round_trip_is_sane() {
    for v in [0.0f32, 0.18, 0.5, 0.92, 1.0] {
        let back = to_srgb(to_linear(v));
        assert!((back - v).abs() < 0.002, "{v} -> {back}");
    }
}

fn dialed_scene(pixels: &[u8], side: u32, dim: u32) -> ClockScene<'_> {
    let mut scene = ClockScene::for_size(dim, dim);
    scene.dial = Some(super::super::assets::DialMap {
        width: side,
        height: side,
        pixels,
    });
    scene.clamped()
}

fn no_hands<'a>(
    ha: &'a [u8],
    hn: &'a [u8],
    hs: &'a [u8],
    ma: &'a [u8],
    mn: &'a [u8],
    ms: &'a [u8],
    clear: &'a [u8],
) -> Hands<'a> {
    Hands {
        hour: maps(8, ha, clear, hn, hs),
        minute: maps(8, ma, clear, mn, ms),
    }
}

#[test]
fn baked_dial_suppresses_hub_and_ticks() {
    // Uniform mid-gray artwork: the center (procedural hub) and the
    // 12-o'clock tick both report no furniture, base and full agree.
    let pixels = std::vec![128u8; 64];
    let scene = dialed_scene(&pixels, 8, 8);
    let base = dial_base_composite(&scene, 4, 4);
    assert!(!base.dial_clock, "baked center must not be hub");
    let tick = dial_base_composite(&scene, 4, 0);
    assert!(!tick.dial_clock, "baked tick row must not be clock");
    let (ha, _, hn, hs, ma, _, mn, ms) = test_hands(8);
    let clear: Vec<u8> = std::vec![0u8; 64];
    let hands = no_hands(&ha, &hn, &hs, &ma, &mn, &ms, &clear);
    let full = pixel_composite(&scene, &hands, true, 4, 4);
    assert!(!full.dial_clock, "full composite must share the authority");
}

#[test]
fn base_and_full_share_one_authority_without_hands() {
    // With no hand coverage, the full composite is the dial base:
    // same gray bytes across the frame for baked artwork.
    let pixels: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(4)).collect();
    let scene = dialed_scene(&pixels, 8, 8);
    let (ha, _, hn, hs, ma, _, mn, ms) = test_hands(8);
    let clear: Vec<u8> = std::vec![0u8; 64];
    let hands = no_hands(&ha, &hn, &hs, &ma, &mn, &ms, &clear);
    for y in 0..8 {
        for x in 0..8 {
            let b = dial_base_composite(&scene, x, y);
            let c = pixel_composite(&scene, &hands, true, x, y);
            assert_eq!(
                gray_byte(b.rgb),
                gray_byte(c.rgb),
                "base/full diverge at ({x},{y})"
            );
        }
    }
}

#[test]
fn hand_darkness_darkens_covered_leaves_dial() {
    // Center is always hand-covered (pivot); corner (0,0) is dial
    // only. Darkness 1 must darken the former and keep the latter
    // byte-exact, including any cast shadow (dial path untouched).
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut base = ClockScene::for_size(64, 64);
    base.hand_darkness = 0.0;
    let mut dark = ClockScene::for_size(64, 64);
    dark.hand_darkness = 1.0;
    let (base, dark) = (base.clamped(), dark.clamped());
    // Coverage comes from the composite itself: zero alpha on both
    // layers means pure dial (byte-exact across darkness), anything
    // above means the hand shade participates (never brightens).
    let (mut covered_strict, mut uncovered) = (0, 0);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let c = pixel_composite(&base, &hands, true, x, y);
            let g0 = evaluate_pixel(&base, &hands, x, y);
            let g1 = evaluate_pixel(&dark, &hands, x, y);
            if c.lower_alpha == 0.0 && c.upper_alpha == 0.0 {
                assert_eq!(g1, g0, "dial moved at ({x},{y})");
                uncovered += 1;
            } else {
                assert!(g1 <= g0, "darkness brightened ({x},{y})");
                if c.lower_alpha >= 0.5 || c.upper_alpha >= 0.5 {
                    assert!(g1 < g0, "covered pixel unchanged ({x},{y})");
                    covered_strict += 1;
                }
            }
        }
    }
    assert!(covered_strict > 0 && uncovered > 0);
}

#[test]
fn hand_darkness_scales_shading_variation() {
    // Hour-only hand with a horizontal specular ramp: two covered
    // pixels differ by spec alone, and darkness scales (not flattens)
    // that difference by exactly (1 - darkness) in linear light.
    let (ha, hal, hn, mut hs, ma, mut mal, mn, ms) = test_hands(8);
    for a in mal.iter_mut() {
        *a = 0;
    }
    for y in 0..8usize {
        for x in 0..8usize {
            hs[y * 8 + x] = (x * 36) as u8;
        }
    }
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut plain = ClockScene::for_size(64, 64);
    plain.hand_darkness = 0.0;
    let mut dimmed = ClockScene::for_size(64, 64);
    dimmed.hand_darkness = 0.85;
    let (plain, dimmed) = (plain.clamped(), dimmed.clamped());
    // Full-alpha interior pixels sampling different ramp columns.
    let (ax, ay, bx, by) = (28u32, 20u32, 36u32, 20u32);
    let pa = pixel_linear(&plain, &hands, true, ax, ay).0;
    let pb = pixel_linear(&plain, &hands, true, bx, by).0;
    assert!((pa - pb).abs() > 1e-4, "ramp pixels unexpectedly equal");
    let da = pixel_linear(&dimmed, &hands, true, ax, ay).0;
    let db = pixel_linear(&dimmed, &hands, true, bx, by).0;
    let ratio = (da - db) / (pa - pb);
    assert!((ratio - 0.15).abs() < 1e-3, "variation not scaled: {ratio}");
}

#[test]
fn hand_darkness_keeps_regions_and_footprint() {
    // Alpha, shadows, and masks are untouched: region labels and the
    // physical footprint must be identical at darkness 0 and 1.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let mut plain = ClockScene::for_size(64, 64);
    plain.hand_darkness = 0.0;
    let mut dark = ClockScene::for_size(64, 64);
    dark.hand_darkness = 1.0;
    let (plain, dark) = (plain.clamped(), dark.clamped());
    for y in 0..64u32 {
        let mut gray_a = std::vec![0u8; 64];
        let mut gray_b = std::vec![0u8; 64];
        let mut reg_a = std::vec![crate::DitherRegion::Background; 64];
        let mut reg_b = std::vec![crate::DitherRegion::Background; 64];
        crate::render_region_row(&plain, &hands, y, &mut gray_a, &mut reg_a);
        crate::render_region_row(&dark, &hands, y, &mut gray_b, &mut reg_b);
        assert_eq!(reg_a, reg_b, "regions moved at row {y}");
        let mut foot_a = std::vec![crate::Footprint::NONE; 64];
        let mut foot_b = std::vec![crate::Footprint::NONE; 64];
        let mut scratch = (
            std::vec![0u8; 64],
            std::vec![crate::DitherRegion::Background; 64],
        );
        crate::render_compose_row(
            &plain,
            &hands,
            y,
            &mut gray_a,
            &mut reg_a,
            &mut scratch.0,
            &mut scratch.1,
            &mut foot_a,
        );
        crate::render_compose_row(
            &dark,
            &hands,
            y,
            &mut gray_b,
            &mut reg_b,
            &mut scratch.0,
            &mut scratch.1,
            &mut foot_b,
        );
        assert_eq!(foot_a, foot_b, "footprint moved at row {y}");
        for x in 0..64usize {
            if !foot_a[x].contains(crate::Footprint::HAND) {
                assert_eq!(
                    gray_a[x], gray_b[x],
                    "uncovered receiver changed at ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn prepared_caches_match_fresh_per_pixel_math() {
    // The hoisted context must hold bit-exact copies of what the old
    // per-pixel path evaluated: sin/cos of each hand angle, pivot-owned
    // scales, and the golden-angle taps in tap order.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    for (hour_angle, minute_angle) in [(0.0, 0.0), (1.7, 4.2), (-2.5, 9.1)] {
        for (samples, light_size) in [(1u8, 0.0), (8, 0.12), (24, 0.3)] {
            let mut scene = ClockScene::for_size(32, 32);
            scene.hour_angle = hour_angle;
            scene.minute_angle = minute_angle;
            scene.samples = samples;
            scene.light_size = light_size;
            let scene = scene.clamped();
            let prep = Prepared::for_frame(&scene, &hands);
            assert_eq!(prep.hour_sin, libm::sinf(scene.hour_angle));
            assert_eq!(prep.hour_cos, libm::cosf(scene.hour_angle));
            assert_eq!(prep.min_sin, libm::sinf(scene.minute_angle));
            assert_eq!(prep.min_cos, libm::cosf(scene.minute_angle));
            assert_eq!(
                prep.hour_scale,
                hand_scale(hands.hour.pivot_y, scene.hour_len)
            );
            assert_eq!(
                prep.minute_scale,
                hand_scale(hands.minute.pivot_y, scene.minute_len)
            );
            assert_eq!(prep.taps_n, scene.samples.clamp(1, MAX_SAMPLES));
            for i in 0..prep.taps_n {
                assert_eq!(
                    prep.taps[usize::from(i)],
                    light_tap(i, prep.taps_n, scene.light_size),
                    "tap {i} diverged"
                );
            }
        }
    }
}

#[test]
fn invalid_dial_map_keeps_procedural_hub() {
    // A mis-sized map clamps to None, so the hub survives.
    let pixels = [0u8; 8];
    let mut scene = ClockScene::for_size(32, 32);
    scene.dial = Some(super::super::assets::DialMap {
        width: 8,
        height: 8,
        pixels: &pixels,
    });
    let scene = scene.clamped();
    assert!(scene.dial.is_none());
    assert!(dial_base_composite(&scene, 16, 16).dial_clock);
}

// Numerical regression for omitting specular work on zero-reflectivity surfaces.
fn original_shade(scene: &ClockScene, surf: &SurfacePoint, vis: f32) -> (f32, f32, f32) {
    let (dx, dy, dz) = (
        scene.light_x - surf.pos.x,
        scene.light_y - surf.pos.y,
        scene.light_h - surf.pos.z,
    );
    let dist = libm::sqrtf(dx * dx + dy * dy + dz * dz).max(1e-6);
    let (lx, ly, lz) = (dx / dist, dy / dist, dz / dist);
    let (nx, ny, nz) = surf.normal;
    let ndotl = (nx * lx + ny * ly + nz * lz).max(0.0);
    let (hx, hy, hz) = (lx, ly, lz + 1.0);
    let hlen = libm::sqrtf(hx * hx + hy * hy + hz * hz).max(1e-6);
    let ndoth = (nx * hx / hlen + ny * hy / hlen + nz * hz / hlen).max(0.0);
    let spec = surf.spec_map * scene.spec_strength * libm::powf(ndoth, scene.shininess) * vis;
    let diff = ndotl * vis + scene.ambient;
    (
        surf.albedo.0 * diff + spec,
        surf.albedo.1 * diff + spec,
        surf.albedo.2 * diff + spec,
    )
}

#[test]
fn zero_reflectivity_shading_preserves_original_float_bits() {
    for light in [-3.0, 0.0, 3.0] {
        for ambient in [0.0, 0.25, 0.8] {
            for shininess in [1.0, 24.0, 128.0] {
                let mut scene = ClockScene::for_size(600, 600);
                scene.light_x = light;
                scene.ambient = ambient;
                scene.shininess = shininess;
                for normal in [(0.0, 0.0, 1.0), (0.6, 0.0, 0.8), (0.0, 0.0, -1.0)] {
                    for albedo in [(0.0, 0.0, 0.0), (0.05, 0.4, 1.0)] {
                        for spec_map in [0.0, -0.0, 0.5] {
                            let surf = SurfacePoint {
                                albedo,
                                normal,
                                spec_map,
                                pos: Point {
                                    x: -0.7,
                                    y: 0.3,
                                    z: 0.1,
                                },
                            };
                            for vis in [0.0, 0.25, 1.0] {
                                let old = original_shade(&scene, &surf, vis);
                                let new = shade(&scene, &surf, vis);
                                assert_eq!(
                                    (new.0.to_bits(), new.1.to_bits(), new.2.to_bits()),
                                    (old.0.to_bits(), old.1.to_bits(), old.2.to_bits())
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

// Basic optimization steps 1 & 2: dynamic-row parity and the alpha-first
// sample fast path. The dynamic row must reproduce the compose row's full
// gray/regions/footprint bit-exactly; the zero-alpha sample must return the
// sentinel without touching hidden maps.

fn parity_scene(w: u32, h: u32, hour_angle: f32, minute_angle: f32) -> ClockScene<'static> {
    let mut scene = ClockScene::for_size(w, h);
    scene.hour_angle = hour_angle;
    scene.minute_angle = minute_angle;
    scene
}

fn parity_hands() -> HandBufs {
    test_hands(8)
}

fn check_dynamic_matches_compose(scene: &ClockScene<'_>, hands: &Hands<'_>, y: u32) {
    let w = scene.clamped().width as usize;
    let mut dyn_gray = std::vec![0u8; w];
    let mut dyn_reg = std::vec![crate::DitherRegion::Background; w];
    let mut dyn_foot = std::vec![crate::Footprint::NONE; w];
    let mut cmp_gray = std::vec![0u8; w];
    let mut cmp_reg = std::vec![crate::DitherRegion::Background; w];
    let mut base_gray = std::vec![0u8; w];
    let mut base_reg = std::vec![crate::DitherRegion::Background; w];
    let mut cmp_foot = std::vec![crate::Footprint::NONE; w];
    crate::render_dynamic_row(scene, hands, y, &mut dyn_gray, &mut dyn_reg, &mut dyn_foot);
    crate::render_compose_row(
        scene,
        hands,
        y,
        &mut cmp_gray,
        &mut cmp_reg,
        &mut base_gray,
        &mut base_reg,
        &mut cmp_foot,
    );
    assert_eq!(dyn_gray, cmp_gray, "gray drift at row {y}");
    assert_eq!(dyn_reg, cmp_reg, "region drift at row {y}");
    assert_eq!(dyn_foot, cmp_foot, "footprint drift at row {y}");
}

#[test]
fn dynamic_row_matches_compose_row_small_grids() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    for (w, h) in [(64u32, 64u32), (40, 30)] {
        for (hour_angle, minute_angle) in [(0.0f32, 0.0f32), (2.1, 5.3)] {
            let scene = parity_scene(w, h, hour_angle, minute_angle);
            for y in [0, h / 2, h - 1] {
                check_dynamic_matches_compose(&scene, &hands, y);
            }
        }
    }
}

#[test]
fn dynamic_row_matches_compose_row_clamped_and_invalid_hands() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Clamped scene: out-of-range samples count, NaN center, zero radius.
    let mut scene = parity_scene(48, 36, 1.2, 3.4);
    scene.samples = 255;
    scene.center_x = f32::NAN;
    scene.radius = 0.0;
    for y in [0, 17, 35] {
        check_dynamic_matches_compose(&scene, &hands, y);
    }
    // Invalid hands fall back to dial-only in both row APIs.
    let bad_albedo = std::vec![0u8; 7];
    let bad = Hands {
        hour: maps(8, &bad_albedo, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    assert!(!bad.validate());
    let scene = parity_scene(48, 36, 1.2, 3.4);
    for y in [0, 17, 35] {
        check_dynamic_matches_compose(&scene, &bad, y);
    }
}

#[test]
fn dynamic_row_edge_contracts_match_compose() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let scene = parity_scene(40, 30, 0.7, 2.9);
    // Out-of-frame rows are a no-op: sentinel-filled slices stay untouched.
    for y in [30u32, 35] {
        let mut dyn_gray = std::vec![0xA5u8; 40];
        let mut dyn_reg = std::vec![crate::DitherRegion::Clock; 40];
        let mut dyn_foot = std::vec![crate::Footprint::HAND; 40];
        crate::render_dynamic_row(
            &scene,
            &hands,
            y,
            &mut dyn_gray,
            &mut dyn_reg,
            &mut dyn_foot,
        );
        assert_eq!(dyn_gray, std::vec![0xA5u8; 40], "row {y} wrote past height");
        assert_eq!(dyn_reg, std::vec![crate::DitherRegion::Clock; 40]);
        assert_eq!(dyn_foot, std::vec![crate::Footprint::HAND; 40]);
    }
    // Short slices: both APIs clip to the shared prefix; tails stay put.
    let y = 7u32;
    let mut dyn_gray = std::vec![0xA5u8; 37];
    let mut dyn_reg = std::vec![crate::DitherRegion::Clock; 35];
    let mut dyn_foot = std::vec![crate::Footprint::HAND; 39];
    let mut cmp_gray = std::vec![0xA5u8; 37];
    let mut cmp_reg = std::vec![crate::DitherRegion::Clock; 35];
    let mut base_gray = std::vec![0u8; 37];
    let mut base_reg = std::vec![crate::DitherRegion::Background; 35];
    let mut cmp_foot = std::vec![crate::Footprint::HAND; 39];
    crate::render_dynamic_row(
        &scene,
        &hands,
        y,
        &mut dyn_gray,
        &mut dyn_reg,
        &mut dyn_foot,
    );
    crate::render_compose_row(
        &scene,
        &hands,
        y,
        &mut cmp_gray,
        &mut cmp_reg,
        &mut base_gray,
        &mut base_reg,
        &mut cmp_foot,
    );
    // Shared prefix is min(37, 35, 39) = 35 pixels.
    assert_eq!(&dyn_gray[..35], &cmp_gray[..35]);
    assert_eq!(dyn_reg, cmp_reg);
    assert_eq!(&dyn_foot[..35], &cmp_foot[..35]);
    assert_eq!(&dyn_gray[35..], &[0xA5u8; 2]);
    assert_eq!(&dyn_foot[35..], &[crate::Footprint::HAND; 4]);
}

#[test]
fn dynamic_row_matches_compose_row_native_frames() {
    // One full frame per native size; small grids above cover the parameter
    // space so these single frames only prove scale, not variety.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    for (w, h, hour, minute) in [(600u32, 600u32, 10u8, 9u8), (400, 300, 3, 47)] {
        let (hour_angle, minute_angle) = crate::angles_for_time(hour, minute, 0);
        let scene = parity_scene(w, h, hour_angle, minute_angle);
        let cw = w as usize;
        let mut dyn_gray = std::vec![0u8; cw];
        let mut dyn_reg = std::vec![crate::DitherRegion::Background; cw];
        let mut dyn_foot = std::vec![crate::Footprint::NONE; cw];
        let mut cmp_gray = std::vec![0u8; cw];
        let mut cmp_reg = std::vec![crate::DitherRegion::Background; cw];
        let mut base_gray = std::vec![0u8; cw];
        let mut base_reg = std::vec![crate::DitherRegion::Background; cw];
        let mut cmp_foot = std::vec![crate::Footprint::NONE; cw];
        for y in 0..h {
            crate::render_dynamic_row(
                &scene,
                &hands,
                y,
                &mut dyn_gray,
                &mut dyn_reg,
                &mut dyn_foot,
            );
            crate::render_compose_row(
                &scene,
                &hands,
                y,
                &mut cmp_gray,
                &mut cmp_reg,
                &mut base_gray,
                &mut base_reg,
                &mut cmp_foot,
            );
            assert_eq!(dyn_gray, cmp_gray, "gray drift at row {y} ({w}x{h})");
            assert_eq!(dyn_reg, cmp_reg, "region drift at row {y} ({w}x{h})");
            assert_eq!(dyn_foot, cmp_foot, "footprint drift at row {y} ({w}x{h})");
        }
    }
}

#[test]
fn zero_alpha_sample_returns_sentinel_without_hidden_maps() {
    // Zero coverage with loud hidden maps: albedo/normal/spec must not leak.
    let n = 8usize * 8usize;
    let albedo = std::vec![200u8; n * 3];
    let alpha = std::vec![0u8; n];
    let normal = std::vec![255u8; n * 3];
    let spec = std::vec![200u8; n];
    let hand = super::super::assets::HandMaps {
        width: 8,
        height: 8,
        albedo: &albedo,
        alpha: &alpha,
        normal: &normal,
        spec: &spec,
        pivot_x: 4.0,
        pivot_y: 4.0,
    };
    let s = sample_hand_with(&hand, 0.0, 1.0, 1.0, 0.0, 0.0);
    assert_eq!(s.alpha, 0.0);
    assert_eq!(s.albedo, (0.0, 0.0, 0.0));
    assert_eq!(s.normal, (0.0, 0.0, 1.0));
    assert_eq!(s.spec, 0.0);
}

#[test]
fn positive_and_tiny_alpha_keep_original_sample_values() {
    let n = 8usize * 8usize;
    let albedo = std::vec![128u8; n * 3];
    let mut normal = std::vec![0u8; n * 3];
    flat_normal(&mut normal);
    let spec = std::vec![128u8; n];
    let full_alpha = std::vec![255u8; n];
    let tiny_alpha = std::vec![1u8; n];
    let full = super::super::assets::HandMaps {
        width: 8,
        height: 8,
        albedo: &albedo,
        alpha: &full_alpha,
        normal: &normal,
        spec: &spec,
        pivot_x: 4.0,
        pivot_y: 4.0,
    };
    let tiny = super::super::assets::HandMaps {
        width: 8,
        height: 8,
        albedo: &albedo,
        alpha: &tiny_alpha,
        normal: &normal,
        spec: &spec,
        pivot_x: 4.0,
        pivot_y: 4.0,
    };
    let f = sample_hand_with(&full, 0.0, 1.0, 1.0, 0.0, 0.0);
    assert_eq!(f.alpha, 1.0);
    let expect_albedo = to_linear(128.0 / 255.0);
    assert_eq!(f.albedo, (expect_albedo, expect_albedo, expect_albedo));
    assert_eq!(f.spec, 128.0 / 255.0);
    assert!(
        f.normal.2 > 0.99,
        "flat normal must stay up, got {:?}",
        f.normal
    );
    // A faint antialiased texel takes the original path too: same albedo as
    // full coverage, only the coverage differs. No positive-alpha cut-off.
    let t = sample_hand_with(&tiny, 0.0, 1.0, 1.0, 0.0, 0.0);
    assert!(t.alpha > 0.0 && t.alpha < 0.01, "tiny alpha {}", t.alpha);
    assert_eq!(t.albedo, f.albedo);
    assert_eq!(t.spec, f.spec);
    assert_eq!(t.normal, f.normal);
}

// Shared shadow masks (option 3): the mask backend must reproduce the ray
// reference bit-exactly on every plane (measured below, per scene — no
// blanket assumption), while the affected interval stays conservative and
// the query counters stay bounded.

fn mask_check_row(scene: &ClockScene<'_>, hands: &Hands<'_>, y: u32) {
    let cw = scene.clamped().width as usize;
    let taps = scene.clamped().samples as u32;
    let mut ray_gray = std::vec![0u8; cw];
    let mut ray_reg = std::vec![crate::DitherRegion::Background; cw];
    let mut ray_base = std::vec![0u8; cw];
    let mut ray_base_reg = std::vec![crate::DitherRegion::Background; cw];
    let mut ray_foot = std::vec![crate::Footprint::NONE; cw];
    let mut m_gray = std::vec![0u8; cw];
    let mut m_reg = std::vec![crate::DitherRegion::Background; cw];
    let mut m_base = std::vec![0u8; cw];
    let mut m_base_reg = std::vec![crate::DitherRegion::Background; cw];
    let mut m_foot = std::vec![crate::Footprint::NONE; cw];
    let mut dial = std::vec![0u8; cw];
    let mut lower = std::vec![0u8; cw];
    let mut upper = std::vec![0u8; cw];
    crate::render_compose_row(
        scene,
        hands,
        y,
        &mut ray_gray,
        &mut ray_reg,
        &mut ray_base,
        &mut ray_base_reg,
        &mut ray_foot,
    );
    let masks = crate::MaskContext::for_frame(scene, hands);
    let stats = crate::render_compose_row_with_masks(
        scene,
        hands,
        &masks,
        y,
        crate::ComposeRowBuffers {
            full_gray: &mut m_gray,
            full_regions: &mut m_reg,
            base_gray: &mut m_base,
            base_regions: &mut m_base_reg,
            footprint: &mut m_foot,
        },
        crate::MaskRowBuffers {
            dial_open: &mut dial,
            lower_open: &mut lower,
            upper_open: &mut upper,
        },
    );
    assert_eq!(m_gray, ray_gray, "mask gray drift at row {y}");
    assert_eq!(m_reg, ray_reg, "mask region drift at row {y}");
    assert_eq!(m_base, ray_base, "mask base drift at row {y}");
    assert_eq!(
        m_base_reg, ray_base_reg,
        "mask base region drift at row {y}"
    );
    assert_eq!(m_foot, ray_foot, "mask footprint drift at row {y}");
    // Counters: every filled pixel counted once; queries bounded by the
    // analytic ceiling (2 paint samples plus 2 silhouette tests per tap
    // per plane, 3 planes).
    assert_eq!(stats.pixels, cw as u32, "row {y} pixel count");
    assert!(
        stats.alpha_queries <= cw as u32 * (2 + taps * 6),
        "row {y} queries {} over bound",
        stats.alpha_queries
    );
    // Conservativeness: any pixel the frame pass changes beyond the
    // stationary base must sit inside the affected interval.
    let (lo, hi) = crate::row_affected_range(&masks, hands, y, cw);
    assert!(lo <= hi && hi <= cw, "range ({lo},{hi}) at row {y}");
    for x in 0..cw {
        if m_gray[x] != m_base[x] || !m_foot[x].is_empty() {
            assert!(
                lo <= x && x < hi,
                "changed pixel {x} outside range ({lo},{hi}) at row {y}"
            );
        }
    }
}

#[test]
fn masks_match_ray_across_height_orders() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Default (minute above hour), reversed, and tied heights.
    let heights = [(0.06f32, 0.12f32), (0.2, 0.05), (0.1, 0.1)];
    for (hour_h, minute_h) in heights {
        for (hour_angle, minute_angle) in [(0.0f32, 0.0f32), (1.7, 4.2), (5.2, 0.3)] {
            let mut scene = parity_scene(32, 28, hour_angle, minute_angle);
            scene.hour_h = hour_h;
            scene.minute_h = minute_h;
            for y in 0..28 {
                mask_check_row(&scene, &hands, y);
            }
        }
    }
}

#[test]
fn masks_match_ray_light_shapes_and_sample_counts() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Point light, single tap.
    let mut scene = parity_scene(32, 28, 2.2, 1.3);
    scene.light_size = 0.0;
    scene.samples = 1;
    for y in 0..28 {
        mask_check_row(&scene, &hands, y);
    }
    // Wide disk, full tap load, moved low light.
    let mut scene = parity_scene(32, 28, 4.4, 0.9);
    scene.light_size = 0.3;
    scene.samples = 24;
    scene.light_x = -0.8;
    scene.light_y = 0.6;
    scene.light_h = 2.0;
    for y in 0..28 {
        mask_check_row(&scene, &hands, y);
    }
    // Light below both casters: nothing casts, dial stays fully lit.
    let mut scene = parity_scene(32, 28, 0.9, 3.3);
    scene.light_h = 0.05;
    scene.hour_h = 0.5;
    scene.minute_h = 0.4;
    for y in 0..28 {
        mask_check_row(&scene, &hands, y);
    }
    // Every supported tap count on one scene.
    let scene = parity_scene(24, 20, 1.1, 2.6);
    for samples in 1..=crate::scene::MAX_SAMPLES {
        let mut scene = scene;
        scene.samples = samples;
        for y in [0, 7, 13, 19] {
            mask_check_row(&scene, &hands, y);
        }
    }
}

#[test]
fn masks_match_ray_clipping_and_small_geometry() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Tiny frame the sprites mostly cover, and an off-center small dial.
    for (w, h) in [(8u32, 6u32), (12, 10)] {
        let scene = parity_scene(w, h, 1.1, 2.6);
        for y in 0..h {
            mask_check_row(&scene, &hands, y);
        }
    }
    let mut scene = parity_scene(24, 20, 1.1, 2.6);
    scene.center_x = 6.0;
    scene.center_y = 16.0;
    scene.radius = 5.0;
    for y in 0..20 {
        mask_check_row(&scene, &hands, y);
    }
    // Clamped extremes: oversized samples, NaN center, zero radius.
    let mut scene = parity_scene(24, 20, 1.2, 3.4);
    scene.samples = 255;
    scene.center_x = f32::NAN;
    scene.radius = 0.0;
    for y in [0, 9, 19] {
        mask_check_row(&scene, &hands, y);
    }
}

#[test]
fn masks_invalid_hands_fill_open_and_match_dial() {
    let (_ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let bad_albedo = std::vec![0u8; 7];
    let bad = Hands {
        hour: maps(8, &bad_albedo, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    assert!(!bad.validate());
    let scene = parity_scene(24, 20, 1.2, 3.4);
    let masks = crate::MaskContext::for_frame(&scene, &bad);
    let taps = scene.clamped().samples;
    assert_eq!(masks.taps_n(), taps);
    for y in [0, 9, 19] {
        let mut dial = std::vec![0u8; 24];
        let mut lower = std::vec![0u8; 24];
        let mut upper = std::vec![0u8; 24];
        let stats = crate::mask_row_counts(&masks, &bad, y, &mut dial, &mut lower, &mut upper);
        assert_eq!(stats.pixels, 24);
        assert_eq!(stats.alpha_queries, 0, "invalid hands sample nothing");
        assert!(dial.iter().all(|c| *c == taps));
        assert!(lower.iter().all(|c| *c == taps));
        assert!(upper.iter().all(|c| *c == taps));
        // Invalid hands render dial-only through both backends alike.
        mask_check_row(&scene, &bad, y);
        // Nothing can be affected without valid sprites.
        assert_eq!(crate::row_affected_range(&masks, &bad, y, 24), (0, 0));
    }
}

#[test]
fn masks_scratch_and_edge_contracts() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let scene = parity_scene(24, 20, 0.7, 2.9);
    let masks = crate::MaskContext::for_frame(&scene, &hands);
    assert_eq!(masks.width(), 24);
    assert_eq!(masks.height(), 20);
    assert_eq!(crate::mask_row_scratch_len(24), Some(72));
    assert_eq!(crate::mask_row_scratch_len(0), Some(0));
    assert_eq!(crate::mask_row_scratch_len(usize::MAX), None);
    // Past-height rows are a no-op: sentinel slices stay untouched.
    for y in [20u32, 27] {
        let mut dial = std::vec![0xA5u8; 24];
        let mut lower = std::vec![0xA5u8; 24];
        let mut upper = std::vec![0xA5u8; 24];
        let stats = crate::mask_row_counts(&masks, &hands, y, &mut dial, &mut lower, &mut upper);
        assert_eq!(stats, crate::MaskStats::ZERO, "row {y} past height");
        assert_eq!(dial, std::vec![0xA5u8; 24]);
        assert_eq!(crate::row_affected_range(&masks, &hands, y, 24), (0, 0));
    }
    // Short scratch clips to the shared prefix; tails stay put.
    let y = 7u32;
    let mut dial = std::vec![0xA5u8; 20];
    let mut lower = std::vec![0xA5u8; 18];
    let mut upper = std::vec![0xA5u8; 22];
    let stats = crate::mask_row_counts(&masks, &hands, y, &mut dial, &mut lower, &mut upper);
    assert_eq!(stats.pixels, 18, "shared prefix is min(20, 18, 22)");
    assert!(dial[..18].iter().all(|c| *c <= masks.taps_n()));
    assert_eq!(&dial[18..], &[0xA5u8; 2]);
    assert_eq!(&upper[18..], &[0xA5u8; 4]);
    // Compose row with short scratch matches the ray prefix too.
    let mut m_gray = std::vec![0xA5u8; 18];
    let mut m_reg = std::vec![crate::DitherRegion::Clock; 18];
    let mut m_base = std::vec![0xA5u8; 18];
    let mut m_base_reg = std::vec![crate::DitherRegion::Clock; 18];
    let mut m_foot = std::vec![crate::Footprint::HAND; 18];
    let mut dial = std::vec![0u8; 20];
    let mut lower = std::vec![0u8; 18];
    let mut upper = std::vec![0u8; 22];
    crate::render_compose_row_with_masks(
        &scene,
        &hands,
        &masks,
        y,
        crate::ComposeRowBuffers {
            full_gray: &mut m_gray,
            full_regions: &mut m_reg,
            base_gray: &mut m_base,
            base_regions: &mut m_base_reg,
            footprint: &mut m_foot,
        },
        crate::MaskRowBuffers {
            dial_open: &mut dial,
            lower_open: &mut lower,
            upper_open: &mut upper,
        },
    );
    let mut ray_gray = std::vec![0u8; 18];
    let mut ray_reg = std::vec![crate::DitherRegion::Background; 18];
    let mut ray_base = std::vec![0u8; 18];
    let mut ray_base_reg = std::vec![crate::DitherRegion::Background; 18];
    let mut ray_foot = std::vec![crate::Footprint::NONE; 18];
    crate::render_compose_row(
        &scene,
        &hands,
        y,
        &mut ray_gray,
        &mut ray_reg,
        &mut ray_base,
        &mut ray_base_reg,
        &mut ray_foot,
    );
    assert_eq!(m_gray, ray_gray, "short-scratch gray prefix");
    assert_eq!(m_foot, ray_foot, "short-scratch footprint prefix");
}

#[test]
fn masks_native_spot_rows_match_ray() {
    // Scale spot-check with synthetic sprites: every 37th row of both
    // native frames at two wall times.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    for (w, h, hour, minute) in [(600u32, 600u32, 10u8, 9u8), (400, 300, 3, 47)] {
        let (hour_angle, minute_angle) = crate::angles_for_time(hour, minute, 0);
        let scene = parity_scene(w, h, hour_angle, minute_angle);
        let mut y = 0;
        while y < h {
            mask_check_row(&scene, &hands, y);
            y += 37;
        }
    }
}

#[test]
fn mask_compose_clamps_geometry_and_preserves_short_prefix_tails() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = test_hands(8);
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    mask_check_row(&ClockScene::for_size(0, 0), &hands, 0);
    let scene = ClockScene::for_size(4, 4);
    let masks = crate::MaskContext::for_frame(&scene, &hands);
    let mut gray = [77; 3];
    let mut base = [77; 3];
    let mut regions = [crate::DitherRegion::Shadows; 3];
    let mut base_regions = regions;
    let mut footprint = [crate::Footprint::HAND; 3];
    let mut dial = [99; 1];
    let mut lower = [99; 2];
    let mut upper = [99; 3];
    let stats = crate::render_compose_row_with_masks(
        &scene,
        &hands,
        &masks,
        0,
        crate::ComposeRowBuffers {
            full_gray: &mut gray,
            full_regions: &mut regions,
            base_gray: &mut base,
            base_regions: &mut base_regions,
            footprint: &mut footprint,
        },
        crate::MaskRowBuffers {
            dial_open: &mut dial,
            lower_open: &mut lower,
            upper_open: &mut upper,
        },
    );
    assert_eq!(stats.pixels, 1);
    assert_eq!(&gray[1..], &[77; 2]);
    assert_eq!(&base[1..], &[77; 2]);
    assert_eq!(&regions[1..], &[crate::DitherRegion::Shadows; 2]);
    assert_eq!(&base_regions[1..], &[crate::DitherRegion::Shadows; 2]);
    assert_eq!(&footprint[1..], &[crate::Footprint::HAND; 2]);
    assert_eq!(&lower[1..], &[99; 1]);
    assert_eq!(&upper[1..], &[99; 2]);
}

// Bounded visibility-mask experiment: `prepare_mask_row` must fill the
// exact legacy counts while bounding paint sampling and per-tap pixel
// visits, and `shade_cached_row_with_masks` must reproduce the legacy
// cached row from the prepared range. The count oracle below re-traces
// the original ray expressions (`ray_hits_hand`, `to_sprite_with`,
// `bilinear`, same thresholds and height rule) per tap — never the
// optimized combo lines — so agreement proves the bounds skip only
// dead pixels instead of proving the optimizer against itself.

/// Independent per-tap open counts plus per-pixel paint flags, traced
/// with the verbatim reference ray test. Same fills, same prefix rule,
/// same invalid-hands behavior as the mask row; only the per-tap loop is
/// spelled out ray by ray.
fn reference_counts(
    scene: &ClockScene<'_>,
    hands: &Hands<'_>,
    y: u32,
    dial: &mut [u8],
    lower: &mut [u8],
    upper: &mut [u8],
    paints: &mut [bool],
) {
    let scene = scene.clamped();
    let n = dial
        .len()
        .min(lower.len())
        .min(upper.len())
        .min(paints.len())
        .min(scene.width as usize);
    if y >= scene.height || n == 0 {
        return;
    }
    let prep = super::Prepared::for_frame(&scene, hands);
    let taps_n = prep.taps_n;
    if !hands.validate() {
        dial[..n].fill(taps_n);
        lower[..n].fill(taps_n);
        upper[..n].fill(taps_n);
        paints[..n].fill(false);
        return;
    }
    dial[..n].fill(taps_n);
    lower[..n].fill(taps_n);
    upper[..n].fill(taps_n);
    paints[..n].fill(false);
    // Same tie rule as the mask context: hour is lower on ties.
    let hour_low = scene.hour_h <= scene.minute_h;
    let lower_h = if hour_low {
        scene.hour_h
    } else {
        scene.minute_h
    };
    let upper_h = if hour_low {
        scene.minute_h
    } else {
        scene.hour_h
    };
    let hour_plane = if hour_low { 1 } else { 2 };
    let caster = |height: f32, hour: bool| super::Blocker {
        hand: if hour { &hands.hour } else { &hands.minute },
        sin: if hour { prep.hour_sin } else { prep.min_sin },
        cos: if hour { prep.hour_cos } else { prep.min_cos },
        scale: if hour {
            prep.hour_scale
        } else {
            prep.minute_scale
        },
        height,
    };
    // Physics contract: every receiver plane traces the actual casters at
    // their scene heights; only recv.z changes per plane. A caster level
    // with its receiver (equal heights) rejects via the height rule.
    let hour = caster(scene.hour_h, true);
    let minute = caster(scene.minute_h, false);
    for x in 0..n {
        let px = (x as f32 - scene.center_x) / scene.radius;
        let py = (y as f32 - scene.center_y) / scene.radius;
        let (hfx, hfy) = super::to_sprite_with(
            hands.hour.pivot_x,
            hands.hour.pivot_y,
            prep.hour_sin,
            prep.hour_cos,
            prep.hour_scale,
            px,
            py,
        );
        let (mfx, mfy) = super::to_sprite_with(
            hands.minute.pivot_x,
            hands.minute.pivot_y,
            prep.min_sin,
            prep.min_cos,
            prep.minute_scale,
            px,
            py,
        );
        let paints_h = super::bilinear(
            hands.hour.alpha,
            hands.hour.width as i32,
            hands.hour.height as i32,
            hfx,
            hfy,
        ) > 0.0;
        let paints_m = super::bilinear(
            hands.minute.alpha,
            hands.minute.width as i32,
            hands.minute.height as i32,
            mfx,
            mfy,
        ) > 0.0;
        paints[x] = paints_h || paints_m;
        // Legacy gate fallback: past PAINT_BITS (640) the mask evaluates
        // every hand plane regardless of paint flags.
        let wide = n > 640;
        let need_lower = wide || if hour_plane == 1 { paints_h } else { paints_m };
        let need_upper = wide || if hour_plane == 2 { paints_h } else { paints_m };
        for i in 0..taps_n {
            let (ox, oy) = prep.taps[usize::from(i)];
            let light = super::Point {
                x: scene.light_x + ox,
                y: scene.light_y + oy,
                z: scene.light_h,
            };
            // Shared-tap union of both blockers before accumulating.
            let recv_dial = super::Point {
                x: px,
                y: py,
                z: 0.0,
            };
            if super::ray_hits_hand(&hour, &recv_dial, &light)
                || super::ray_hits_hand(&minute, &recv_dial, &light)
            {
                dial[x] -= 1;
            }
            if need_lower {
                let recv = super::Point {
                    x: px,
                    y: py,
                    z: lower_h,
                };
                if super::ray_hits_hand(&hour, &recv, &light)
                    || super::ray_hits_hand(&minute, &recv, &light)
                {
                    lower[x] -= 1;
                }
            }
            if need_upper {
                let recv = super::Point {
                    x: px,
                    y: py,
                    z: upper_h,
                };
                if super::ray_hits_hand(&hour, &recv, &light)
                    || super::ray_hits_hand(&minute, &recv, &light)
                {
                    upper[x] -= 1;
                }
            }
        }
    }
}

/// One bounded-prepare check against the independent ray oracle: exact
/// counts, exact legacy stats, a conservative range covering every
/// changed or painted pixel, zero work on clean rows, and queries under
/// the analytic ceiling. Returns the work for frame-level accounting.
fn check_prepare_row(scene: &ClockScene<'_>, hands: &Hands<'_>, y: u32) -> crate::MaskRowWork {
    let cw = scene.clamped().width as usize;
    let taps = scene.clamped().samples;
    let masks = crate::MaskContext::for_frame(scene, hands);
    let mut dial = std::vec![0u8; cw];
    let mut lower = std::vec![0u8; cw];
    let mut upper = std::vec![0u8; cw];
    let work = crate::prepare_mask_row(&masks, hands, y, &mut dial, &mut lower, &mut upper);
    let mut r_dial = std::vec![0u8; cw];
    let mut r_lower = std::vec![0u8; cw];
    let mut r_upper = std::vec![0u8; cw];
    let mut r_paints = std::vec![false; cw];
    reference_counts(
        scene,
        hands,
        y,
        &mut r_dial,
        &mut r_lower,
        &mut r_upper,
        &mut r_paints,
    );
    assert_eq!(dial, r_dial, "dial counts drift at row {y}");
    assert_eq!(lower, r_lower, "lower counts drift at row {y}");
    assert_eq!(upper, r_upper, "upper counts drift at row {y}");
    // The legacy entry point is a pure delegate: same stats, same planes.
    let mut l_dial = std::vec![0u8; cw];
    let mut l_lower = std::vec![0u8; cw];
    let mut l_upper = std::vec![0u8; cw];
    let stats = crate::mask_row_counts(&masks, hands, y, &mut l_dial, &mut l_lower, &mut l_upper);
    assert_eq!(stats, work.stats, "delegate stats drift at row {y}");
    assert_eq!(l_dial, dial, "delegate dial drift at row {y}");
    // Range conservativeness: any blocked count or any painted pixel —
    // coverage shades even with no shadow — must sit inside.
    let (lo, hi) = work.affected_range;
    assert!(lo <= hi && hi <= cw, "range ({lo},{hi}) at row {y}");
    for x in 0..cw {
        if dial[x] != taps || lower[x] != taps || upper[x] != taps || r_paints[x] {
            assert!(
                lo <= x && x < hi,
                "pixel {x} outside range ({lo},{hi}) at row {y}"
            );
        }
    }
    if work.affected_range == (0, 0) {
        assert_eq!(work.paint_queries, 0, "clean row {y} sampled paint");
        assert_eq!(work.tap_iterations, 0, "clean row {y} visited taps");
        assert_eq!(
            work.stats.alpha_queries, 0,
            "clean row {y} issued silhouette samples"
        );
    }
    assert!(
        work.stats.alpha_queries <= cw as u32 * (2 + u32::from(taps) * 6),
        "row {y} queries {} over bound",
        work.stats.alpha_queries
    );
    work
}

#[test]
fn bounded_prepare_matches_ray_adversarial() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Reversed, default, and tied heights across rotations.
    let heights = [(0.06f32, 0.12f32), (0.2, 0.05), (0.1, 0.1)];
    for (hour_h, minute_h) in heights {
        for (hour_angle, minute_angle) in [(0.0f32, 0.0f32), (1.7, 4.2), (5.2, 0.3)] {
            let mut scene = parity_scene(32, 28, hour_angle, minute_angle);
            scene.hour_h = hour_h;
            scene.minute_h = minute_h;
            for y in 0..28 {
                check_prepare_row(&scene, &hands, y);
            }
        }
    }
    // Tiny frame, off-center small dial, and a light below both casters
    // (nothing casts, dial stays fully lit).
    for (w, h) in [(8u32, 6u32), (12, 10)] {
        let scene = parity_scene(w, h, 1.1, 2.6);
        for y in 0..h {
            check_prepare_row(&scene, &hands, y);
        }
    }
    let mut scene = parity_scene(24, 20, 1.1, 2.6);
    scene.center_x = 6.0;
    scene.center_y = 16.0;
    scene.radius = 5.0;
    for y in 0..20 {
        check_prepare_row(&scene, &hands, y);
    }
    let mut scene = parity_scene(32, 28, 0.9, 3.3);
    scene.light_h = 0.05;
    scene.hour_h = 0.5;
    scene.minute_h = 0.4;
    for y in 0..28 {
        check_prepare_row(&scene, &hands, y);
    }
    // Clamped extremes: oversized samples, NaN center, zero radius.
    let mut scene = parity_scene(24, 20, 1.2, 3.4);
    scene.samples = 255;
    scene.center_x = f32::NAN;
    scene.radius = 0.0;
    for y in [0, 9, 19] {
        check_prepare_row(&scene, &hands, y);
    }
    // Fully offscreen dial: every row is clean, every count stays open.
    let mut scene = parity_scene(24, 20, 1.1, 2.6);
    scene.center_x = 100_000.0;
    scene.center_y = -100_000.0;
    let masks = crate::MaskContext::for_frame(&scene, &hands);
    let taps = scene.clamped().samples;
    for y in 0..20 {
        let work = check_prepare_row(&scene, &hands, y);
        assert_eq!(work.affected_range, (0, 0), "offscreen row {y}");
        assert_eq!(work.stats.pixels, 24, "offscreen row {y}");
        let mut dial = std::vec![0u8; 24];
        let mut lower = std::vec![0u8; 24];
        let mut upper = std::vec![0u8; 24];
        crate::prepare_mask_row(&masks, &hands, y, &mut dial, &mut lower, &mut upper);
        assert!(dial.iter().all(|c| *c == taps), "offscreen row {y}");
        assert!(lower.iter().all(|c| *c == taps), "offscreen row {y}");
        assert!(upper.iter().all(|c| *c == taps), "offscreen row {y}");
    }
}

#[test]
fn bounded_prepare_matches_ray_past_paint_gate() {
    // 700 columns clears `PAINT_BITS` (640): the paint gate falls back
    // to every-plane evaluation while the per-tap projected bounds still
    // bound the visits. Counts must still match the ray oracle exactly,
    // and the legacy full-row parity must hold on spot rows.
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let scene = parity_scene(700, 40, 1.1, 2.6);
    assert!(scene.clamped().width as usize > 640);
    for y in 0..40 {
        check_prepare_row(&scene, &hands, y);
    }
    for y in [0, 13, 39] {
        mask_check_row(&scene, &hands, y);
    }
}

#[test]
fn bounded_prepare_reduces_work_on_native_screens() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let mut hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Anchor near the base, like the production hand sprites. The generic
    // centered-square parity fixture extends a tip-length in both directions
    // and its projected bounds can legitimately cover every sampled row.
    hands.hour.pivot_y = 7.0;
    hands.minute.pivot_y = 7.0;
    // Both production frames at wall times, spot rows like the existing
    // native parity test.
    for (w, h, hour, minute) in [(600u32, 600u32, 10u8, 9u8), (400, 300, 3, 47)] {
        let (hour_angle, minute_angle) = crate::angles_for_time(hour, minute, 0);
        let scene = parity_scene(w, h, hour_angle, minute_angle);
        let taps = u64::from(scene.clamped().samples);
        let mut paint_total = 0u64;
        let mut tap_total = 0u64;
        let mut px_total = 0u64;
        let mut nontrivial_rows = 0u32;
        let mut clean_rows = 0u32;
        let mut y = 0;
        while y < h {
            let work = check_prepare_row(&scene, &hands, y);
            let n = u64::from(work.stats.pixels);
            px_total += n;
            paint_total += u64::from(work.paint_queries);
            tap_total += u64::from(work.tap_iterations);
            if work.affected_range == (0, 0) {
                clean_rows += 1;
            } else {
                nontrivial_rows += 1;
            }
            y += 37;
        }
        assert!(nontrivial_rows > 0, "{w}x{h} never touches a pixel");
        assert!(clean_rows > 0, "{w}x{h} never skips a row");
        // Unbounded baselines: 2 paint samples per pixel, one pixel
        // visit per tap. Bounds only — never pinned exact counts.
        assert!(
            paint_total < 2 * px_total,
            "{w}x{h} paint {paint_total} not below {px}",
            px = 2 * px_total
        );
        assert!(
            tap_total < taps * px_total,
            "{w}x{h} taps {tap_total} not below {bound}",
            bound = taps * px_total
        );
    }
}

#[test]
fn bounded_prepare_keeps_short_prefix_contracts() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    let scene = parity_scene(24, 20, 0.7, 2.9);
    let masks = crate::MaskContext::for_frame(&scene, &hands);
    // Short scratch clips to the shared prefix; tails stay put and the
    // prefix matches the ray oracle on the same shapes.
    let y = 7u32;
    let mut dial = std::vec![0xA5u8; 20];
    let mut lower = std::vec![0xA5u8; 18];
    let mut upper = std::vec![0xA5u8; 22];
    let work = crate::prepare_mask_row(&masks, &hands, y, &mut dial, &mut lower, &mut upper);
    assert_eq!(work.stats.pixels, 18, "shared prefix is min(20, 18, 22)");
    assert_eq!(&dial[18..], &[0xA5u8; 2]);
    assert_eq!(&upper[18..], &[0xA5u8; 4]);
    let mut r_dial = std::vec![0xA5u8; 20];
    let mut r_lower = std::vec![0xA5u8; 18];
    let mut r_upper = std::vec![0xA5u8; 22];
    let mut r_paints = std::vec![false; 22];
    reference_counts(
        &scene,
        &hands,
        y,
        &mut r_dial,
        &mut r_lower,
        &mut r_upper,
        &mut r_paints,
    );
    assert_eq!(dial, r_dial, "short-scratch dial prefix");
    assert_eq!(lower, r_lower, "short-scratch lower prefix");
    assert_eq!(upper, r_upper, "short-scratch upper prefix");
    // Invalid hands: pixel prefix filled open, clean range, zero work.
    let bad_albedo = std::vec![0u8; 7];
    let bad = Hands {
        hour: maps(8, &bad_albedo, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    assert!(!bad.validate());
    let bad_masks = crate::MaskContext::for_frame(&scene, &bad);
    let taps = scene.clamped().samples;
    let mut dial = std::vec![0u8; 24];
    let mut lower = std::vec![0u8; 24];
    let mut upper = std::vec![0u8; 24];
    let work = crate::prepare_mask_row(&bad_masks, &bad, y, &mut dial, &mut lower, &mut upper);
    assert_eq!(work.stats.pixels, 24);
    assert_eq!(work.stats.alpha_queries, 0);
    assert_eq!(work.affected_range, (0, 0));
    assert_eq!(work.paint_queries, 0);
    assert_eq!(work.tap_iterations, 0);
    assert!(dial.iter().all(|c| *c == taps));
    // Past-height and empty rows touch nothing and report zero work.
    for bad_y in [20u32, 27] {
        let mut dial = std::vec![0xA5u8; 24];
        let mut lower = std::vec![0xA5u8; 24];
        let mut upper = std::vec![0xA5u8; 24];
        let work =
            crate::prepare_mask_row(&masks, &hands, bad_y, &mut dial, &mut lower, &mut upper);
        assert_eq!(work, crate::MaskRowWork::ZERO, "row {bad_y} past height");
        assert_eq!(dial, std::vec![0xA5u8; 24]);
    }
    let mut empty_a: [u8; 0] = [];
    let mut empty_b: [u8; 0] = [];
    let mut empty_c: [u8; 0] = [];
    let work = crate::prepare_mask_row(&masks, &hands, y, &mut empty_a, &mut empty_b, &mut empty_c);
    assert_eq!(work, crate::MaskRowWork::ZERO, "empty scratch");
}

#[test]
fn split_prepare_shade_matches_legacy_cached_row() {
    let (ha, hal, hn, hs, ma, mal, mn, ms) = parity_hands();
    let hands = Hands {
        hour: maps(8, &ha, &hal, &hn, &hs),
        minute: maps(8, &ma, &mal, &mn, &ms),
    };
    // Default and reversed height orders on a small frame: every row,
    // prepared then shaded, must equal the legacy wrapper byte for byte.
    for (hour_h, minute_h) in [(0.06f32, 0.12f32), (0.2, 0.05)] {
        let mut scene = parity_scene(24, 20, 1.1, 2.6);
        scene.hour_h = hour_h;
        scene.minute_h = minute_h;
        let masks = crate::MaskContext::for_frame(&scene, &hands);
        for y in 0..20 {
            let cw = 24usize;
            let mut base_gray = std::vec![0u8; cw];
            let mut base_regions = std::vec![crate::DitherRegion::Background; cw];
            crate::render_base_row(&scene, y, &mut base_gray, &mut base_regions);
            // Legacy wrapper output.
            let mut l_gray = std::vec![0u8; cw];
            let mut l_reg = std::vec![crate::DitherRegion::Background; cw];
            let mut l_foot = std::vec![crate::Footprint::NONE; cw];
            let mut l_dial = std::vec![0u8; cw];
            let mut l_lower = std::vec![0u8; cw];
            let mut l_upper = std::vec![0u8; cw];
            let stats = crate::render_cached_row_with_masks(
                &scene,
                &hands,
                &masks,
                y,
                crate::CachedRowBuffers {
                    base_gray: &base_gray,
                    base_regions: &base_regions,
                    full_gray: &mut l_gray,
                    full_regions: &mut l_reg,
                    footprint: &mut l_foot,
                },
                crate::MaskRowBuffers {
                    dial_open: &mut l_dial,
                    lower_open: &mut l_lower,
                    upper_open: &mut l_upper,
                },
            );
            // Split output: same counts, prepared range, then shade.
            let mut s_gray = std::vec![0u8; cw];
            let mut s_reg = std::vec![crate::DitherRegion::Background; cw];
            let mut s_foot = std::vec![crate::Footprint::NONE; cw];
            let mut s_dial = std::vec![0u8; cw];
            let mut s_lower = std::vec![0u8; cw];
            let mut s_upper = std::vec![0u8; cw];
            let work =
                crate::prepare_mask_row(&masks, &hands, y, &mut s_dial, &mut s_lower, &mut s_upper);
            assert_eq!(work.stats, stats, "split stats drift at row {y}");
            crate::shade_cached_row_with_masks(
                &scene,
                &hands,
                &masks,
                y,
                crate::CachedRowBuffers {
                    base_gray: &base_gray,
                    base_regions: &base_regions,
                    full_gray: &mut s_gray,
                    full_regions: &mut s_reg,
                    footprint: &mut s_foot,
                },
                crate::MaskRowBuffers {
                    dial_open: &mut s_dial,
                    lower_open: &mut s_lower,
                    upper_open: &mut s_upper,
                },
                work.affected_range,
            );
            assert_eq!(s_gray, l_gray, "split gray drift at row {y}");
            assert_eq!(s_reg, l_reg, "split region drift at row {y}");
            assert_eq!(s_foot, l_foot, "split footprint drift at row {y}");
            // Shading the full prefix with an over-wide range is the same
            // picture: the range only bounds work, never the result.
            let mut f_gray = std::vec![0u8; cw];
            let mut f_reg = std::vec![crate::DitherRegion::Background; cw];
            let mut f_foot = std::vec![crate::Footprint::NONE; cw];
            let mut f_dial = s_dial.clone();
            let mut f_lower = s_lower.clone();
            let mut f_upper = s_upper.clone();
            crate::shade_cached_row_with_masks(
                &scene,
                &hands,
                &masks,
                y,
                crate::CachedRowBuffers {
                    base_gray: &base_gray,
                    base_regions: &base_regions,
                    full_gray: &mut f_gray,
                    full_regions: &mut f_reg,
                    footprint: &mut f_foot,
                },
                crate::MaskRowBuffers {
                    dial_open: &mut f_dial,
                    lower_open: &mut f_lower,
                    upper_open: &mut f_upper,
                },
                (0, cw + 1000),
            );
            assert_eq!(f_gray, l_gray, "full-range gray drift at row {y}");
            assert_eq!(f_foot, l_foot, "full-range footprint drift at row {y}");
        }
    }
    // Past-height shade is a no-op: sentinels stay untouched.
    let scene = parity_scene(24, 20, 1.1, 2.6);
    let masks = crate::MaskContext::for_frame(&scene, &hands);
    let mut gray = [77u8; 24];
    let mut regions = [crate::DitherRegion::Shadows; 24];
    let mut footprint = [crate::Footprint::HAND; 24];
    let mut dial = [1u8; 24];
    let mut lower = [1u8; 24];
    let mut upper = [1u8; 24];
    let base_gray = [9u8; 24];
    let base_regions = [crate::DitherRegion::Clock; 24];
    crate::shade_cached_row_with_masks(
        &scene,
        &hands,
        &masks,
        27,
        crate::CachedRowBuffers {
            base_gray: &base_gray,
            base_regions: &base_regions,
            full_gray: &mut gray,
            full_regions: &mut regions,
            footprint: &mut footprint,
        },
        crate::MaskRowBuffers {
            dial_open: &mut dial,
            lower_open: &mut lower,
            upper_open: &mut upper,
        },
        (0, 24),
    );
    assert_eq!(gray, [77u8; 24], "past-height shade touched gray");
    assert_eq!(regions, [crate::DitherRegion::Shadows; 24]);
    assert_eq!(footprint, [crate::Footprint::HAND; 24]);
}

// Prepared-ray hoist: per-query sampling reuses the setup `t`/`qy` with the
// exact reference expressions, so silhouette alphas straddling the `> 0.5`
// threshold and height-order boundaries must match the oracle bit-exactly.
#[test]
fn prepared_ray_matches_oracle_at_threshold_and_eligibility() {
    let (ha, _hal, hn, hs, ma, _mal, mn, ms) = parity_hands();
    // 127/255 sits just below the silhouette threshold, 128/255 just
    // above; the checkerboard forces bilinear blends across it.
    let solid_below = std::vec![127u8; 64];
    let solid_above = std::vec![128u8; 64];
    let mut checker = std::vec![0u8; 64];
    for (i, px) in checker.iter_mut().enumerate() {
        *px = if (i / 8 + i % 8) % 2 == 0 { 127 } else { 128 };
    }
    for alpha in [&solid_below, &solid_above, &checker] {
        let hands = Hands {
            hour: maps(8, &ha, alpha, &hn, &hs),
            minute: maps(8, &ma, alpha, &mn, &ms),
        };
        for (hour_h, minute_h) in [(0.06f32, 0.12f32), (0.2, 0.05), (0.1, 0.1)] {
            // Light below both casters (nothing eligible), between the hand
            // heights (only a caster below the light), exactly at a caster height (the strict
            // rule excludes it), and high (everything eligible).
            for light_h in [0.05f32, 0.08, 0.1, 0.2, 2.0] {
                for (hour_angle, minute_angle) in [(0.0f32, 0.0f32), (1.7, 4.2)] {
                    let mut scene = parity_scene(32, 28, hour_angle, minute_angle);
                    scene.hour_h = hour_h;
                    scene.minute_h = minute_h;
                    scene.light_h = light_h;
                    for y in 0..28 {
                        check_prepare_row(&scene, &hands, y);
                    }
                }
            }
        }
    }
}

// Normalization-table experiment: frozen pre-table sampler. This keeps the
// original `f32::from(byte) / 255.0` conversion with the identical
// expression order, clipping, and weights, so any table rounding bug shows
// as a bit mismatch here. Same-scene ray-vs-mask comparisons alone would
// miss such a bug because both sides share the sampler.
fn original_bilinear_strided(
    map: &[u8],
    w: i32,
    h: i32,
    fx: f32,
    fy: f32,
    ch: usize,
    stride: usize,
) -> f32 {
    let x0 = libm::floorf(fx) as i32;
    let y0 = libm::floorf(fy) as i32;
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let ty = (fy - y0 as f32).clamp(0.0, 1.0);
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            f32::from(map[((y as usize) * (w as usize) + (x as usize)) * stride + ch]) / 255.0
        }
    };
    let a = at(x0, y0);
    let b = at(x0 + 1, y0);
    let c = at(x0, y0 + 1);
    let d = at(x0 + 1, y0 + 1);
    a * (1.0 - tx) * (1.0 - ty) + b * tx * (1.0 - ty) + c * (1.0 - tx) * ty + d * tx * ty
}

#[test]
fn normalization_table_matches_original_division_all256() {
    assert_eq!(std::mem::size_of_val(&BYTE_NORM), 1024);
    for b in 0..=255u16 {
        let byte = b as u8;
        assert_eq!(
            BYTE_NORM[byte as usize].to_bits(),
            (f32::from(byte) / 255.0).to_bits(),
            "table entry differs for byte {byte}"
        );
    }
}

const ORACLE_XS: [f32; 12] = [
    -2.0, -1.0, -0.5, 0.0, 0.25, 0.5, 1.0, 1.5, 2.75, 3.0, 3.5, 6.25,
];
const ORACLE_YS: [f32; 12] = [
    -3.0, -1.0, -0.25, 0.0, 0.5, 0.75, 1.0, 2.0, 2.5, 3.0, 4.0, 7.5,
];

#[test]
fn bilinear_alpha_matches_original_oracle_gray() {
    // Mask-only arm: the gray comparisons exercise the LUT (`true`) path,
    // not the legacy `false` wrapper. 4x4 ramp covering 0 and 255; two
    // cells pinned to the 127/128 silhouette-threshold boundary.
    let mut map: Vec<u8> = (0u32..16).map(|i| (i * 17) as u8).collect();
    map[5] = 127;
    map[6] = 128;
    for fx in ORACLE_XS {
        for fy in ORACLE_YS {
            assert_eq!(
                bilinear_alpha(&map, 4, 4, fx, fy).to_bits(),
                original_bilinear_strided(&map, 4, 4, fx, fy, 0, 1).to_bits(),
                "gray mismatch at ({fx}, {fy})"
            );
        }
    }
}

#[test]
fn bilinear_strided_policies_match_original_oracle_rgb() {
    // 3x3 RGB plane with distinct per-channel ramps, incl 0/127/128/255.
    // The shared body is exercised directly with both const policies:
    // `true` (table) is the experiment path, `false` must stay identical
    // to the frozen oracle too, pinning the material/ray normalization.
    let mut map = std::vec![0u8; 27];
    for (i, px) in map.chunks_exact_mut(3).enumerate() {
        px[0] = ((i * 31) % 256) as u8;
        px[1] = ((i * 53 + 127) % 256) as u8;
        px[2] = ((i * 97 + 200) % 256) as u8;
    }
    map[0] = 0;
    map[4] = 127;
    map[8] = 128;
    map[26] = 255;
    for ch in 0..3 {
        for fx in ORACLE_XS {
            for fy in ORACLE_YS {
                let want = original_bilinear_strided(&map, 3, 3, fx, fy, ch, 3).to_bits();
                assert_eq!(
                    bilinear_strided_with::<true>(&map, 3, 3, fx, fy, ch, 3).to_bits(),
                    want,
                    "rgb table ch {ch} mismatch at ({fx}, {fy})"
                );
                assert_eq!(
                    bilinear_strided_with::<false>(&map, 3, 3, fx, fy, ch, 3).to_bits(),
                    want,
                    "rgb divide ch {ch} mismatch at ({fx}, {fy})"
                );
                assert_eq!(
                    bilinear_strided(&map, 3, 3, fx, fy, ch, 3).to_bits(),
                    want,
                    "rgb wrapper ch {ch} mismatch at ({fx}, {fy})"
                );
            }
        }
    }
}

#[test]
fn bilinear_alpha_threshold_matches_original_oracle() {
    // Mask-only arm: the threshold comparisons exercise the LUT (`true`)
    // path. 127/255 sits just below the silhouette cut, 128/255 just
    // above; the checkerboard forces fractional blends across `> 0.5`.
    let mut checker = std::vec![0u8; 64];
    for (i, px) in checker.iter_mut().enumerate() {
        *px = if (i / 8 + i % 8) % 2 == 0 { 127 } else { 128 };
    }
    for fx in ORACLE_XS {
        for fy in ORACLE_YS {
            let got = bilinear_alpha(&checker, 8, 8, fx, fy);
            let want = original_bilinear_strided(&checker, 8, 8, fx, fy, 0, 1);
            assert_eq!(got.to_bits(), want.to_bits(), "bits at ({fx}, {fy})");
            assert_eq!(got > 0.5, want > 0.5, "silhouette decision at ({fx}, {fy})");
        }
    }
}

#[test]
fn prepared_ray_exercises_silhouette_samples() {
    // Above-threshold solid hands must actually run the prepared sampler on
    // some row: exactness above is vacuous if every query were skipped.
    let (ha, _hal, hn, hs, ma, _mal, mn, ms) = parity_hands();
    let above = std::vec![128u8; 64];
    let hands = Hands {
        hour: maps(8, &ha, &above, &hn, &hs),
        minute: maps(8, &ma, &above, &mn, &ms),
    };
    let scene = parity_scene(32, 28, 1.7, 4.2);
    let mut total = 0u32;
    for y in 0..28 {
        let work = check_prepare_row(&scene, &hands, y);
        total += work.stats.alpha_queries - work.paint_queries;
    }
    assert!(total > 0, "prepared path issued no silhouette samples");
}
