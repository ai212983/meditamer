//! Lossless grayscale albedo: gray hands must render byte-exact against
//! replicated-RGB hands across rotations, specular, and darkness, while
//! malformed albedo lengths stay rejected.

use analog_clock::{render_gray_row, render_region_row, ClockScene, HandMaps, Hands};

// --- Canonical test sprites ------------------------------------------------

/// 16x20 sprites: canonical RGB albedo (same gray in all three channels,
// varies per texel), soft alpha edges (exercises AA), tilted normal patch,
// spec gradient.
struct Sprites {
    rgb: Vec<u8>,
    gray: Vec<u8>,
    alpha: Vec<u8>,
    normal: Vec<u8>,
    spec: Vec<u8>,
}

const SW: usize = 16;
const SH: usize = 20;

fn sprites() -> Sprites {
    let n = SW * SH;
    let mut rgb = vec![0u8; n * 3];
    let mut gray = vec![0u8; n];
    let mut alpha = vec![0u8; n];
    let mut normal = vec![0u8; n * 3];
    let mut spec = vec![0u8; n];
    for y in 0..SH {
        for x in 0..SW {
            let i = y * SW + x;
            // Varied gray: gradient plus checker, kept well inside 0..255.
            let g = ((x * 13 + y * 29 + ((x ^ y) & 7) * 11) % 200 + 20) as u8;
            rgb[i * 3] = g;
            rgb[i * 3 + 1] = g;
            rgb[i * 3 + 2] = g;
            gray[i] = g;
            // Soft radial alpha: hard core, fading rim, transparent corners.
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
            // Normals: flat center, tilted rim patch.
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
        rgb,
        gray,
        alpha,
        normal,
        spec,
    }
}

fn rgb_hands(s: &Sprites) -> Hands<'_> {
    Hands {
        hour: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.rgb,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
        minute: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.rgb,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
    }
}

fn gray_hands(s: &Sprites) -> Hands<'_> {
    Hands {
        hour: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.gray,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
        minute: HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &s.gray,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        },
    }
}

/// Scene variants spanning rotations, anti-aliasing positions, specular
/// response, and darkness: each must render identically under both albedo
/// layouts.
fn scene_variants() -> Vec<ClockScene<'static>> {
    let mut out = Vec::new();
    // Rotations: on-axis, fractional (AA-heavy), and large.
    for (ha, ma) in [(0.0, 0.0), (1.7, 4.2), (5.2, 0.3), (2.399_963, 3.7)] {
        let mut s = ClockScene::for_size(24, 24);
        s.hour_angle = ha;
        s.minute_angle = ma;
        out.push(s);
    }
    // Specular sweep: boosted, suppressed, sharp lobe.
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
    out.push(s);
    // Darkness sweep: partial/full hand darkening, flat/dark ambient,
    // point vs area light, single vs many taps.
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 2.2;
    s.minute_angle = 1.3;
    s.hand_darkness = 0.5;
    s.ambient = 0.0;
    s.light_size = 0.0;
    s.samples = 1;
    out.push(s);
    let mut s = ClockScene::for_size(24, 24);
    s.hour_angle = 4.4;
    s.minute_angle = 0.9;
    s.hand_darkness = 1.0;
    s.ambient = 0.9;
    s.light_size = 0.3;
    out.push(s);
    out
}

#[test]
fn gray_albedo_renders_byte_exact_across_pipeline() {
    let s = sprites();
    let rgb = rgb_hands(&s);
    let gray = gray_hands(&s);
    assert!(rgb.validate());
    assert!(gray.validate());
    assert!(!rgb.hour.albedo_is_gray());
    assert!(gray.hour.albedo_is_gray());
    for scene in scene_variants() {
        for y in 0..scene.height {
            let mut a = vec![0u8; scene.width as usize];
            let mut b = vec![0u8; scene.width as usize];
            render_gray_row(&scene, &rgb, y, &mut a);
            render_gray_row(&scene, &gray, y, &mut b);
            assert_eq!(
                a, b,
                "gray row {y} diverged (ha={} ma={})",
                scene.hour_angle, scene.minute_angle
            );
        }
        // Region labels ride the same samples: gray bytes and labels match.
        for y in 0..scene.height {
            let mut ga = vec![0u8; scene.width as usize];
            let mut gb = vec![0u8; scene.width as usize];
            let mut ra = vec![analog_clock::DitherRegion::Background; scene.width as usize];
            let mut rb = vec![analog_clock::DitherRegion::Background; scene.width as usize];
            render_region_row(&scene, &rgb, y, &mut ga, &mut ra);
            render_region_row(&scene, &gray, y, &mut gb, &mut rb);
            assert_eq!(ga, gb, "region gray row {y} diverged");
            assert_eq!(ra, rb, "region labels row {y} diverged");
        }
    }
}

#[test]
fn malformed_albedo_lengths_reject() {
    let s = sprites();
    let n = SW * SH;
    // Neither pixels nor pixels*3: rejected, and not mistaken for gray.
    let bad = vec![7u8; n + 7];
    let hand = HandMaps {
        width: SW as u16,
        height: SH as u16,
        albedo: &bad,
        alpha: &s.alpha,
        normal: &s.normal,
        spec: &s.spec,
        pivot_x: 7.5,
        pivot_y: 15.0,
    };
    assert!(!hand.validate());
    assert!(!hand.albedo_is_gray());
    // Off-by-one on either side of the RGB length also rejects.
    for len in [n * 3 - 1, n * 3 + 1, n - 1, n + 1] {
        let buf = vec![0u8; len];
        let hand = HandMaps {
            width: SW as u16,
            height: SH as u16,
            albedo: &buf,
            alpha: &s.alpha,
            normal: &s.normal,
            spec: &s.spec,
            pivot_x: 7.5,
            pivot_y: 15.0,
        };
        // n+1 collides with neither valid length (n+1 != n, n+1 != 3n for
        // n > 1); every length here must fail validation.
        assert!(!hand.validate(), "length {len} unexpectedly valid");
    }
    // Empty and zero-geometry hands still reject.
    let empty: Vec<u8> = Vec::new();
    assert!(!HandMaps {
        width: SW as u16,
        height: SH as u16,
        albedo: &empty,
        alpha: &s.alpha,
        normal: &s.normal,
        spec: &s.spec,
        pivot_x: 7.5,
        pivot_y: 15.0,
    }
    .validate());
}
