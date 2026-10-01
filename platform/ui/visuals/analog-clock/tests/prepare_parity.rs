//! Prepared-context parity: rows prepared once per row must match the
//! per-pixel convenience path exactly.
//!
//! `render_gray_row` / `render_region_row` / `render_compose_row` snapshot
//! the frame invariants (hand sin/cos, sprite scales, light taps) once per
//! row, while `evaluate_pixel` snapshots them per pixel. The snapshot
//! cadence must be unobservable: same gray bytes, same region labels, same
//! footprint flags. The independent pre-change golden frames the parent
//! compares remain the reference for absolute output; this test pins the
//! hoisting itself across rotations, times, geometry, light taps, unequal
//! heights, spec, and darkness.

use analog_clock::{
    angles_for_time, evaluate_pixel, render_base_row, render_compose_row, render_gray_row,
    render_region_row, ClockScene, DitherRegion, Footprint, HandMaps, Hands,
};

const SW: usize = 16;
const SH: usize = 20;

struct Sprites {
    albedo: Vec<u8>,
    alpha: Vec<u8>,
    normal: Vec<u8>,
    spec: Vec<u8>,
}

fn sprites() -> Sprites {
    let n = SW * SH;
    let mut albedo = vec![0u8; n * 3];
    let mut alpha = vec![0u8; n];
    let mut normal = vec![0u8; n * 3];
    let mut spec = vec![0u8; n];
    for y in 0..SH {
        for x in 0..SW {
            let i = y * SW + x;
            let g = ((x * 13 + y * 29 + ((x ^ y) & 7) * 11) % 200 + 20) as u8;
            albedo[i * 3] = g;
            albedo[i * 3 + 1] = g;
            albedo[i * 3 + 2] = g;
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 9.5;
            let d = (dx * dx + dy * dy).sqrt();
            alpha[i] = if d < 3.0 {
                255
            } else if d > 7.0 {
                0
            } else {
                (255.0 * (1.0 - (d - 3.0) / 4.0)) as u8
            };
            let (nx, ny) = if x > 10 {
                (200u8, 127u8)
            } else {
                (127u8, 127u8)
            };
            normal[i * 3] = nx;
            normal[i * 3 + 1] = ny;
            normal[i * 3 + 2] = 250;
            spec[i] = ((x * 17 + y * 5) % 256) as u8;
        }
    }
    Sprites {
        albedo,
        alpha,
        normal,
        spec,
    }
}

fn hands(s: &Sprites) -> Hands<'_> {
    Hands {
        hour: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.albedo,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
        minute: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.albedo,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
    }
}

/// Scene variants spanning rotations, wall-clock times, geometry, light
/// taps/positions, unequal heights (both layer orders), and the
/// spec/darkness/ambient finish.
fn scene_variants() -> Vec<ClockScene<'static>> {
    let mut out = Vec::new();
    for (ha, ma) in [(0.0, 0.0), (1.7, 4.2), (5.2, 0.3), (2.399_963, 3.7)] {
        let mut s = ClockScene::for_size(24, 24);
        s.hour_angle = ha;
        s.minute_angle = ma;
        out.push(s);
    }
    // Wall-clock times, including the parent snapshot's 10:09.
    for (h, m) in [(10u8, 9u8), (3, 0), (6, 30), (11, 59)] {
        let mut s = ClockScene::for_size(24, 24);
        let (ha, ma) = angles_for_time(h, m, 0);
        s.hour_angle = ha;
        s.minute_angle = ma;
        out.push(s);
    }
    // Geometry: off-center dial, small radius.
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 1.1;
    s.minute_angle = 2.6;
    s.center_x = 10.0;
    s.center_y = 14.0;
    s.radius = 8.0;
    out.push(s);
    // Light taps: point light, wide area light with many taps, moved light.
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 2.2;
    s.minute_angle = 1.3;
    s.light_size = 0.0;
    s.samples = 1;
    out.push(s);
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 4.4;
    s.minute_angle = 0.9;
    s.light_size = 0.3;
    s.samples = 24;
    s.light_x = -0.8;
    s.light_y = 0.6;
    s.light_h = 2.0;
    out.push(s);
    // Unequal heights, both layer orders.
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 0.9;
    s.minute_angle = 3.3;
    s.hour_h = 0.2;
    s.minute_h = 0.05;
    out.push(s);
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 0.9;
    s.minute_angle = 3.3;
    s.hour_h = 0.05;
    s.minute_h = 0.2;
    out.push(s);
    // Finish: boosted/sharp specular, flat specular, darkness sweeps.
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 1.1;
    s.minute_angle = 2.6;
    s.hour_spec = 3.0;
    s.minute_spec = 0.25;
    s.spec_strength = 2.0;
    s.shininess = 128.0;
    out.push(s);
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 0.7;
    s.minute_angle = 5.9;
    s.spec_strength = 0.0;
    s.hand_darkness = 0.85;
    s.ambient = 0.05;
    out.push(s);
    out
}

#[test]
fn prepared_rows_match_per_pixel_reference() {
    let s = sprites();
    let hands = hands(&s);
    assert!(hands.validate());
    for scene in scene_variants() {
        let w = scene.width as usize;
        for y in 0..scene.height {
            let mut gray = vec![0u8; w];
            let mut regions = vec![DitherRegion::Background; w];
            let mut c_gray = vec![0u8; w];
            let mut c_regions = vec![DitherRegion::Background; w];
            let mut b_gray = vec![0u8; w];
            let mut b_regions = vec![DitherRegion::Background; w];
            let mut foot = vec![Footprint::NONE; w];
            let mut base_gray = vec![0u8; w];
            let mut base_regions = vec![DitherRegion::Background; w];
            render_gray_row(&scene, &hands, y, &mut gray);
            // Region gray rides the same composite: identical bytes.
            render_region_row(&scene, &hands, y, &mut c_gray, &mut c_regions);
            assert_eq!(gray, c_gray, "region gray diverged at row {y}");
            render_compose_row(
                &scene,
                &hands,
                y,
                &mut gray.clone(),
                &mut regions.clone(),
                &mut b_gray,
                &mut b_regions,
                &mut foot,
            );
            render_compose_row(
                &scene,
                &hands,
                y,
                &mut gray,
                &mut regions,
                &mut b_gray,
                &mut b_regions,
                &mut foot,
            );
            render_base_row(&scene, y, &mut base_gray, &mut base_regions);
            assert_eq!(c_gray, gray, "compose full gray diverged at row {y}");
            assert_eq!(
                c_regions, regions,
                "compose full regions diverged at row {y}"
            );
            for x in 0..scene.width {
                let i = x as usize;
                // Row-once preparation vs per-pixel preparation: the
                // snapshot cadence must be unobservable in the byte.
                let px = (evaluate_pixel(&scene, &hands, x, y) * 255.0) as u8;
                assert_eq!(
                    gray[i], px,
                    "row/per-pixel gray diverged at ({x},{y}) ha={} ma={}",
                    scene.hour_angle, scene.minute_angle
                );
                // Footprint/region contract: HAND needs real coverage
                // (cover > 0), which the Hands label (cover >= 0.5)
                // implies; NONE means no coverage and full visibility, so
                // neither Hands nor Shadows may claim the pixel; Shadows
                // needs shortfall (vis < 1), hence a non-NONE footprint.
                if regions[i] == DitherRegion::Hands {
                    assert!(
                        foot[i].hand(),
                        "Hands region without HAND footprint at ({x},{y})"
                    );
                }
                if foot[i].is_empty() {
                    assert!(
                        regions[i] == DitherRegion::Background || regions[i] == DitherRegion::Clock,
                        "empty footprint with {:?} region at ({x},{y})",
                        regions[i]
                    );
                }
                if regions[i] == DitherRegion::Shadows {
                    assert!(
                        !foot[i].is_empty(),
                        "Shadows region with empty footprint at ({x},{y})"
                    );
                }
            }
            // Base plane agrees with the standalone base row.
            let mut solo_gray = vec![0u8; w];
            let mut solo_regions = vec![DitherRegion::Background; w];
            render_base_row(&scene, y, &mut solo_gray, &mut solo_regions);
            assert_eq!(b_gray, solo_gray, "compose base gray diverged at row {y}");
            assert_eq!(
                b_regions, solo_regions,
                "compose base regions diverged at row {y}"
            );
        }
    }
}
