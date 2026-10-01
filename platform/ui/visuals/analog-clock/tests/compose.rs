//! Composition experiment contracts: legacy parity, footprint honesty,
//! candidate selection, removal guards. Gray/region truth comes from the
//! current shared paths, never from re-implemented oracles.

use analog_clock::{
    compose_surface, dither_flat, dither_regions, packed_bits_len, physical_footprint,
    regional_scratch_len, removal_keep_row, render_base_row, render_compose_row, render_gray_row,
    render_region_row, Candidates, CompositionMode, DitherAlgorithm, DitherRegion, Footprint,
    HandMaps, Hands, RegionDithers, Surface, UnknownCompositionMode,
};

/// One-bit capture surface for assertions.
struct Frame {
    width: u32,
    height: u32,
    bits: Vec<bool>,
}

impl Frame {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits: vec![false; width as usize * height as usize],
        }
    }

    fn filled(width: u32, height: u32, ink: bool) -> Self {
        Self {
            width,
            height,
            bits: vec![ink; width as usize * height as usize],
        }
    }

    fn get(&self, x: u32, y: u32) -> bool {
        self.bits[y as usize * self.width as usize + x as usize]
    }
}

impl Surface for Frame {
    fn width(&self) -> i32 {
        self.width as i32
    }

    fn height(&self) -> i32 {
        self.height as i32
    }

    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.bits[y as usize * self.width as usize + x as usize] = ink;
        }
    }
}

/// Pack row-major ink flags MSB-first (`1 = ink`): the candidate convention.
fn pack_bits(inks: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; inks.len().div_ceil(8)];
    for (i, &ink) in inks.iter().enumerate() {
        if ink {
            out[i / 8] |= 0x80 >> (i % 8);
        }
    }
    out
}

/// Owned 8x8 block-sprite buffers: mid albedo, full alpha, flat normals.
struct Blocks {
    ha: Vec<u8>,
    hal: Vec<u8>,
    hn: Vec<u8>,
    hs: Vec<u8>,
    ma: Vec<u8>,
    mal: Vec<u8>,
    mn: Vec<u8>,
    ms: Vec<u8>,
}

fn flat_normal(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(3) {
        px[0] = 127;
        px[1] = 127;
        px[2] = 255;
    }
}

fn block_set() -> Blocks {
    let n = 8 * 8;
    let mut hn = vec![0u8; n * 3];
    let mut mn = vec![0u8; n * 3];
    flat_normal(&mut hn);
    flat_normal(&mut mn);
    Blocks {
        ha: vec![128u8; n * 3],
        hal: vec![255u8; n],
        hn,
        hs: vec![128u8; n],
        ma: vec![128u8; n * 3],
        mal: vec![255u8; n],
        mn,
        ms: vec![128u8; n],
    }
}

fn hand<'a>(albedo: &'a [u8], alpha: &'a [u8], normal: &'a [u8], spec: &'a [u8]) -> HandMaps<'a> {
    HandMaps {
        width: 8,
        height: 8,
        albedo,
        alpha,
        normal,
        spec,
        pivot_x: 4.0,
        pivot_y: 4.0,
    }
}

/// Compact scene: hands cover the center, ticks stay exposed, a point light
/// throws hard dial shadows down-left of the hands.
fn shadow_scene() -> analog_clock::ClockScene<'static> {
    let mut scene = analog_clock::ClockScene::for_size(32, 32);
    scene.hour_len = 0.4;
    scene.minute_len = 0.55;
    scene.hour_h = 0.05;
    scene.minute_h = 0.12;
    scene.light_x = 0.8;
    scene.light_y = -0.6;
    scene.light_h = 0.8;
    scene.light_size = 0.0;
    scene.samples = 12;
    scene
}

/// Duplicate-blocker scene: identical opaque sprites at equal heights, so
/// union and single blockers agree on exposed dial.
fn union_scene() -> analog_clock::ClockScene<'static> {
    let mut scene = analog_clock::ClockScene::for_size(32, 32);
    scene.center_x = 16.0;
    scene.center_y = 16.0;
    scene.radius = 16.0;
    scene.hour_len = 0.4;
    scene.minute_len = 0.55;
    scene.hour_h = 0.15;
    scene.minute_h = 0.15;
    scene.light_x = 0.8;
    scene.light_y = -0.6;
    scene.light_h = 0.8;
    scene.light_size = 0.0;
    scene.samples = 12;
    scene
}

struct Composed {
    full_gray: Vec<u8>,
    full_regions: Vec<DitherRegion>,
    base_gray: Vec<u8>,
    base_regions: Vec<DitherRegion>,
    footprint: Vec<Footprint>,
}

/// Whole frame through the compose row path.
fn compose_frame(scene: &analog_clock::ClockScene, hands: &Hands) -> Composed {
    let w = scene.width as usize;
    let h = scene.height as usize;
    let mut c = Composed {
        full_gray: vec![0u8; w * h],
        full_regions: vec![DitherRegion::Background; w * h],
        base_gray: vec![0u8; w * h],
        base_regions: vec![DitherRegion::Background; w * h],
        footprint: vec![Footprint::NONE; w * h],
    };
    for y in 0..scene.height {
        let row = y as usize * w;
        render_compose_row(
            scene,
            hands,
            y,
            &mut c.full_gray[row..row + w],
            &mut c.full_regions[row..row + w],
            &mut c.base_gray[row..row + w],
            &mut c.base_regions[row..row + w],
            &mut c.footprint[row..row + w],
        );
    }
    c
}

fn uniform(algo: DitherAlgorithm) -> RegionDithers {
    RegionDithers {
        background: algo,
        clock: algo,
        hands: algo,
        shadows: algo,
    }
}

/// Regional pass captured as ink flags.
fn dither_flags(
    width: u32,
    height: u32,
    gray: &[u8],
    regions: &[DitherRegion],
    profiles: &RegionDithers,
) -> Vec<bool> {
    let mut scratch = vec![0.0f32; regional_scratch_len(width as usize).expect("scratch")];
    let mut frame = Frame::new(width, height);
    dither_regions(
        width,
        height,
        gray,
        regions,
        profiles,
        &mut scratch,
        &mut frame,
    )
    .expect("dither");
    frame.bits
}

/// Uniform pass captured as ink flags.
fn flat_flags(width: u32, height: u32, gray: &[u8], algo: DitherAlgorithm) -> Vec<bool> {
    let mut scratch = vec![0.0f32; regional_scratch_len(width as usize).expect("scratch")];
    let mut frame = Frame::new(width, height);
    dither_flat(width, height, gray, algo, &mut scratch, &mut frame).expect("flat dither");
    frame.bits
}

fn opaque_hands(blocks: &Blocks) -> Hands<'_> {
    Hands {
        hour: hand(&blocks.ha, &blocks.hal, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    }
}

#[test]
fn compose_full_is_legacy_exact() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let c = compose_frame(&scene, &hands);
    let w = scene.width as usize;
    for y in 0..scene.height {
        let row = y as usize * w;
        let mut legacy_gray = vec![0u8; w];
        render_gray_row(&scene, &hands, y, &mut legacy_gray);
        let mut check_gray = vec![0u8; w];
        let mut check_regions = vec![DitherRegion::Background; w];
        render_region_row(&scene, &hands, y, &mut check_gray, &mut check_regions);
        assert_eq!(
            &c.full_gray[row..row + w],
            &check_gray,
            "gray drift row {y}"
        );
        assert_eq!(
            &c.full_regions[row..row + w],
            &check_regions,
            "region drift row {y}"
        );
        assert_eq!(&check_gray, &legacy_gray, "region path gray drift row {y}");
    }
    // Short slices share the prefix; past-the-frame rows are a no-op.
    let mut fg = vec![0u8; 5];
    let mut fr = vec![DitherRegion::Background; 5];
    let mut bg = vec![0u8; 7];
    let mut br = vec![DitherRegion::Background; 5];
    let mut fp = vec![Footprint::NONE; 5];
    render_compose_row(
        &scene, &hands, 4, &mut fg, &mut fr, &mut bg, &mut br, &mut fp,
    );
    let mut legacy = vec![0u8; 5];
    render_gray_row(&scene, &hands, 4, &mut legacy);
    assert_eq!(fg, legacy);
    let mut fg = vec![9u8; w];
    let mut fr = vec![DitherRegion::Hands; w];
    let mut bg = vec![9u8; w];
    let mut br = vec![DitherRegion::Hands; w];
    let mut fp = vec![Footprint::HAND; w];
    render_compose_row(
        &scene,
        &hands,
        scene.height + 1,
        &mut fg,
        &mut fr,
        &mut bg,
        &mut br,
        &mut fp,
    );
    assert!(fg.iter().all(|&g| g == 9));
    assert!(fr.iter().all(|&r| r == DitherRegion::Hands));
    assert!(bg.iter().all(|&g| g == 9));
    assert!(br.iter().all(|&r| r == DitherRegion::Hands));
    assert!(fp.iter().all(|&f| f == Footprint::HAND));
}

#[test]
fn base_is_unshadowed_dial_invariant_over_time_and_height() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let mut scene = shadow_scene();
    let first = compose_frame(&scene, &hands);
    // Base regions are dial-only labels.
    assert!(first
        .base_regions
        .iter()
        .all(|r| *r == DitherRegion::Background || *r == DitherRegion::Clock));
    assert!(first.base_regions.contains(&DitherRegion::Clock));
    // Move both hands through times and swap their heights: the dial behind
    // must not move.
    for (hh, mm) in [(0, 0), (3, 15), (9, 45)] {
        let (ha, ma) = analog_clock::angles_for_time(hh, mm, 0);
        for (hh_h, mm_h) in [(0.05, 0.12), (0.12, 0.05)] {
            scene.hour_angle = ha;
            scene.minute_angle = ma;
            scene.hour_h = hh_h;
            scene.minute_h = mm_h;
            let next = compose_frame(&scene, &hands);
            assert_eq!(first.base_gray, next.base_gray, "base moved at {hh}:{mm}");
            assert_eq!(
                first.base_regions, next.base_regions,
                "base labels moved at {hh}:{mm}"
            );
        }
    }
}

#[test]
fn base_row_matches_compose_base_without_hands() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let w = scene.width as usize;
    let c = compose_frame(&scene, &hands);
    for y in 0..scene.height {
        let row = y as usize * w;
        let mut gray = vec![0u8; w];
        let mut regions = vec![DitherRegion::Background; w];
        render_base_row(&scene, y, &mut gray, &mut regions);
        assert_eq!(&gray, &c.base_gray[row..row + w], "base gray drift row {y}");
        assert_eq!(
            &regions,
            &c.base_regions[row..row + w],
            "base region drift row {y}"
        );
    }
}

#[test]
fn mode_names_parse_through_from_str() {
    assert_eq!(
        "removal".parse::<CompositionMode>(),
        Ok(CompositionMode::Removal)
    );
    assert!("nope".parse::<CompositionMode>().is_err());
    assert_eq!(
        format!("{}", UnknownCompositionMode),
        "unknown composition mode (legacy|reference|replacement|removal)"
    );
}

#[test]
fn transparent_hands_leave_no_footprint_and_match_base() {
    let blocks = block_set();
    let clear = vec![0u8; 64];
    let hands = Hands {
        hour: hand(&blocks.ha, &clear, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &clear, &blocks.mn, &blocks.ms),
    };
    let scene = shadow_scene();
    let c = compose_frame(&scene, &hands);
    assert!(c.footprint.iter().all(|f| *f == Footprint::NONE));
    assert_eq!(c.full_gray, c.base_gray);
    // Broken buffers also render the dial alone with no footprint.
    let short = vec![0u8; 4];
    let broken = Hands {
        hour: hand(&short, &short, &short, &short),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    assert!(!broken.validate());
    let c = compose_frame(&scene, &broken);
    assert!(c.footprint.iter().all(|f| *f == Footprint::NONE));
    assert_eq!(c.full_gray, c.base_gray);
}

#[test]
fn opaque_scene_flags_hands_and_shadow_locally() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let c = compose_frame(&scene, &hands);
    let hands_n = c.footprint.iter().filter(|f| f.hand()).count();
    let shadow_n = c.footprint.iter().filter(|f| f.shadow()).count();
    let none_n = c.footprint.iter().filter(|f| f.is_empty()).count();
    assert!(hands_n > 0, "no hand footprint");
    assert!(shadow_n > 0, "no shadow footprint");
    assert!(none_n > 0, "footprint covers the whole frame");
}

#[test]
fn replacement_restores_static_outside_and_dynamic_inside() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    let profiles = uniform(DitherAlgorithm::Gradient);
    let dynamic = pack_bits(&dither_flags(
        w,
        h,
        &c.full_gray,
        &c.full_regions,
        &profiles,
    ));
    let static_bg = pack_bits(&flat_flags(w, h, &c.base_gray, DitherAlgorithm::Gradient));
    let candidates = Candidates {
        static_bg: &static_bg,
        static_clock: None,
        dynamic: &dynamic,
        mask: None,
    };
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Replacement,
        w,
        h,
        &c.base_regions,
        &c.footprint,
        &candidates,
        &mut out,
    )
    .expect("compose");
    let pixels = w as usize * h as usize;
    let mut inside = 0;
    for i in 0..pixels {
        let want_dynamic = !c.footprint[i].is_empty();
        let bit = |packed: &[u8]| packed[i / 8] & (0x80 >> (i % 8)) != 0;
        if want_dynamic {
            inside += 1;
            assert_eq!(out.bits[i], bit(&dynamic), "dynamic not selected at {i}");
        } else {
            assert_eq!(out.bits[i], bit(&static_bg), "static not restored at {i}");
        }
    }
    assert!(inside > 0, "footprint empty: selection untested");
    assert!(inside < pixels, "footprint full: restoration untested");
}

#[test]
fn replacement_zero_affected_preserves_base_exact() {
    // Synthetic: nothing moves, so the composed frame must equal the
    // independently dithered static candidate bit for bit.
    let w = 16u32;
    let h = 8u32;
    let pixels = w as usize * h as usize;
    let base_gray: Vec<u8> = (0..pixels).map(|i| (i * 7 % 256) as u8).collect();
    let base_regions = vec![DitherRegion::Background; pixels];
    let footprint = vec![Footprint::NONE; pixels];
    let profiles = uniform(DitherAlgorithm::Bayer4);
    let full_gray = base_gray.clone();
    let full_regions = base_regions.clone();
    let dynamic = pack_bits(&dither_flags(w, h, &full_gray, &full_regions, &profiles));
    let static_bg = pack_bits(&flat_flags(w, h, &base_gray, DitherAlgorithm::Bayer4));
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Replacement,
        w,
        h,
        &base_regions,
        &footprint,
        &Candidates {
            static_bg: &static_bg,
            static_clock: None,
            dynamic: &dynamic,
            mask: None,
        },
        &mut out,
    )
    .expect("compose");
    assert_eq!(
        out.bits,
        flat_flags(w, h, &base_gray, DitherAlgorithm::Bayer4)
    );
}

#[test]
fn stationary_clock_candidate_owns_furniture() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    let profiles = uniform(DitherAlgorithm::Gradient);
    let dynamic = pack_bits(&dither_flags(
        w,
        h,
        &c.full_gray,
        &c.full_regions,
        &profiles,
    ));
    // Deliberately different static algorithms so the source is observable.
    let static_bg = pack_bits(&flat_flags(w, h, &c.base_gray, DitherAlgorithm::Threshold));
    let static_clock = pack_bits(&flat_flags(w, h, &c.base_gray, DitherAlgorithm::Bayer8));
    let bit = |packed: &[u8], i: usize| packed[i / 8] & (0x80 >> (i % 8)) != 0;
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Replacement,
        w,
        h,
        &c.base_regions,
        &c.footprint,
        &Candidates {
            static_bg: &static_bg,
            static_clock: Some(&static_clock),
            dynamic: &dynamic,
            mask: None,
        },
        &mut out,
    )
    .expect("compose");
    let pixels = w as usize * h as usize;
    let mut saw_clock = 0;
    let mut saw_bg = 0;
    for i in 0..pixels {
        if !c.footprint[i].is_empty() {
            continue;
        }
        match c.base_regions[i] {
            DitherRegion::Clock => {
                saw_clock += 1;
                assert_eq!(
                    out.bits[i],
                    bit(&static_clock, i),
                    "clock mis-sourced at {i}"
                );
            }
            _ => {
                saw_bg += 1;
                assert_eq!(
                    out.bits[i],
                    bit(&static_bg, i),
                    "background mis-sourced at {i}"
                );
            }
        }
    }
    assert!(saw_clock > 0, "no exposed furniture outside the footprint");
    assert!(saw_bg > 0, "no exposed paper outside the footprint");
    // Without its own candidate the furniture falls back to the background.
    let mut fallback = Frame::new(w, h);
    compose_surface(
        CompositionMode::Replacement,
        w,
        h,
        &c.base_regions,
        &c.footprint,
        &Candidates {
            static_bg: &static_bg,
            static_clock: None,
            dynamic: &dynamic,
            mask: None,
        },
        &mut fallback,
    )
    .expect("compose");
    for i in 0..pixels {
        if c.footprint[i].is_empty() {
            assert_eq!(
                fallback.bits[i],
                bit(&static_bg, i),
                "fallback broke at {i}"
            );
        }
    }
}

#[test]
fn removal_identity_plane_restores_static() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    // Identical rows thin nothing: every keep byte is 255 (the dial never
    // encodes true black here, so no guard fires).
    let mut keep = vec![0u8; c.base_gray.len()];
    for (y, row) in keep.chunks_exact_mut(w as usize).enumerate() {
        let off = y * w as usize;
        removal_keep_row(
            &c.base_gray[off..off + w as usize],
            &c.base_gray[off..off + w as usize],
            row,
        );
    }
    assert!(keep.iter().all(|&k| k == 255));
    let mask = pack_bits(&flat_flags(w, h, &keep, DitherAlgorithm::Gradient));
    assert!(
        flat_flags(w, h, &keep, DitherAlgorithm::Gradient)
            .iter()
            .all(|&ink| !ink),
        "full-keep mask must stay all paper"
    );
    let profiles = uniform(DitherAlgorithm::Gradient);
    let dynamic = pack_bits(&dither_flags(
        w,
        h,
        &c.full_gray,
        &c.full_regions,
        &profiles,
    ));
    let static_bg = pack_bits(&flat_flags(w, h, &c.base_gray, DitherAlgorithm::Gradient));
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Removal,
        w,
        h,
        &c.base_regions,
        &vec![Footprint::NONE; c.footprint.len()],
        &Candidates {
            static_bg: &static_bg,
            static_clock: None,
            dynamic: &dynamic,
            mask: Some(&mask),
        },
        &mut out,
    )
    .expect("compose");
    assert_eq!(
        out.bits,
        flat_flags(w, h, &c.base_gray, DitherAlgorithm::Gradient)
    );
}

#[test]
fn removal_only_adds_ink_and_hands_take_dynamic() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    let profiles = uniform(DitherAlgorithm::Gradient);
    let dynamic = pack_bits(&dither_flags(
        w,
        h,
        &c.full_gray,
        &c.full_regions,
        &profiles,
    ));
    // Hostile patterns: striped static, striped mask — removal must still
    // never clear a static ink dot and must pass hands through verbatim.
    let pixels = w as usize * h as usize;
    let static_bits: Vec<bool> = (0..pixels).map(|i| i % 3 == 0).collect();
    let mask_bits: Vec<bool> = (0..pixels).map(|i| i % 2 == 0).collect();
    let static_bg = pack_bits(&static_bits);
    let mask = pack_bits(&mask_bits);
    let candidates = Candidates {
        static_bg: &static_bg,
        static_clock: None,
        dynamic: &dynamic,
        mask: Some(&mask),
    };
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Removal,
        w,
        h,
        &c.base_regions,
        &c.footprint,
        &candidates,
        &mut out,
    )
    .expect("compose");
    let bit = |packed: &[u8], i: usize| packed[i / 8] & (0x80 >> (i % 8)) != 0;
    let mut saw_hand = 0;
    let mut saw_shadow = 0;
    for i in 0..pixels {
        if c.footprint[i].hand() {
            saw_hand += 1;
            assert_eq!(
                out.bits[i],
                bit(&dynamic, i),
                "hand must take dynamic at {i}"
            );
        } else if c.footprint[i].shadow() {
            saw_shadow += 1;
            assert_eq!(
                out.bits[i],
                static_bits[i] || mask_bits[i],
                "shadow must thin, never restore, at {i}"
            );
            assert!(!static_bits[i] || out.bits[i], "removal added white at {i}");
        } else {
            assert_eq!(out.bits[i], static_bits[i], "static moved at {i}");
        }
    }
    assert!(
        saw_hand > 0 && saw_shadow > 0,
        "footprint too small to judge"
    );
}

#[test]
fn faint_coverage_flags_hand_without_hands_label() {
    // Nearly transparent sprites: coverage stays under the half-cover label
    // but the physical footprint must still see the hands.
    let blocks = block_set();
    let faint = vec![64u8; 64];
    let hands = Hands {
        hour: hand(&blocks.ha, &faint, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &faint, &blocks.mn, &blocks.ms),
    };
    let scene = shadow_scene();
    let c = compose_frame(&scene, &hands);
    let mut partial = 0;
    for i in 0..c.footprint.len() {
        if c.footprint[i].hand() && c.full_regions[i] != DitherRegion::Hands {
            partial += 1;
        }
    }
    assert!(partial > 0, "no sub-label hand coverage found");
}

#[test]
fn duplicate_blockers_keep_union_footprint() {
    // Same argument as the regional duplicate-blocker test, at the physical
    // level: identical silhouettes at identical heights give identical
    // visibility, so where the union sees no hand the single blocker sees
    // the same dial — same gray, same (absent) footprint.
    let blocks = block_set();
    let clear = vec![0u8; 64];
    let both = opaque_hands(&blocks);
    let single = Hands {
        hour: hand(&blocks.ha, &clear, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let scene = union_scene();
    let b = compose_frame(&scene, &both);
    let s = compose_frame(&scene, &single);
    let mut dial_pixels = 0;
    let mut umbra = 0;
    for i in 0..b.footprint.len() {
        if b.footprint[i] == Footprint::NONE {
            dial_pixels += 1;
            assert_eq!(
                s.footprint[i],
                Footprint::NONE,
                "union moved a footprint at {i}"
            );
            assert_eq!(
                b.full_gray[i], s.full_gray[i],
                "union moved dial gray at {i}"
            );
        }
        if b.footprint[i].shadow() {
            umbra += 1;
        }
    }
    assert!(dial_pixels > 256, "too little exposed dial: {dial_pixels}");
    assert!(umbra > 0, "no physical umbra found");
}

#[test]
fn flat_matches_uniform_regional_for_every_selector() {
    let algos = [
        DitherAlgorithm::Threshold,
        DitherAlgorithm::Gradient,
        DitherAlgorithm::Bayer4,
        DitherAlgorithm::Bayer8,
        DitherAlgorithm::BlueNoise,
        DitherAlgorithm::FloydSteinberg,
        DitherAlgorithm::Atkinson,
    ];
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    for algo in algos {
        let flat = flat_flags(w, h, &c.full_gray, algo);
        let regional = dither_flags(w, h, &c.full_gray, &c.full_regions, &uniform(algo));
        // Uniform profiles ignore labels, but the region plane here carries
        // mixed legacy labels: equality proves the flat pass truly ignores
        // them, for ordered and diffusion selectors alike.
        assert_eq!(flat, regional, "flat diverged for {algo:?}");
    }
}

#[test]
fn reference_copies_the_blue_noise_candidate() {
    let blocks = block_set();
    let hands = opaque_hands(&blocks);
    let scene = shadow_scene();
    let (w, h) = (scene.width, scene.height);
    let c = compose_frame(&scene, &hands);
    let dynamic = pack_bits(&flat_flags(w, h, &c.full_gray, DitherAlgorithm::BlueNoise));
    let mut out = Frame::new(w, h);
    compose_surface(
        CompositionMode::Reference,
        w,
        h,
        &c.base_regions,
        &c.footprint,
        &Candidates {
            static_bg: &[],
            static_clock: None,
            dynamic: &dynamic,
            mask: None,
        },
        &mut out,
    )
    .expect("compose");
    assert_eq!(
        out.bits,
        flat_flags(w, h, &c.full_gray, DitherAlgorithm::BlueNoise)
    );
}

#[test]
fn packed_convention_is_msb_first_ink() {
    // 1 = ink, MSB first: 0x80 inks only the top-left pixel of an 8x1 row.
    let mut out = Frame::new(8, 1);
    compose_surface(
        CompositionMode::Reference,
        8,
        1,
        &[],
        &[],
        &Candidates {
            static_bg: &[],
            static_clock: None,
            dynamic: &[0x80],
            mask: None,
        },
        &mut out,
    )
    .expect("compose");
    assert!(out.get(0, 0));
    for x in 1..8 {
        assert!(!out.get(x, 0), "paper leaked at {x}");
    }
}

#[test]
fn compose_validates_before_mutation() {
    let w = 8u32;
    let h = 4u32;
    let pixels = w as usize * h as usize;
    let need_bytes = packed_bits_len(w, h).expect("len");
    let regions = vec![DitherRegion::Background; pixels];
    let footprint = vec![Footprint::NONE; pixels];
    let good = vec![0u8; need_bytes];
    let full = Candidates {
        static_bg: &good,
        static_clock: None,
        dynamic: &good,
        mask: Some(&good),
    };
    // A poisoned surface proves failed calls write nothing.
    let attempt = |mode, regs: &[DitherRegion], fp: &[Footprint], cand: &Candidates<'_>| {
        let mut poisoned = Frame::filled(w, h, true);
        let err =
            compose_surface(mode, w, h, regs, fp, cand, &mut poisoned).expect_err("must fail");
        assert!(
            poisoned.bits.iter().all(|&b| b),
            "failed compose mutated the surface"
        );
        err
    };
    assert_eq!(
        attempt(CompositionMode::Legacy, &regions, &footprint, &full),
        analog_clock::ComposeError::NoCandidatesForLegacy
    );
    assert_eq!(
        attempt(
            CompositionMode::Replacement,
            &regions[..pixels - 1],
            &footprint,
            &full
        ),
        analog_clock::ComposeError::RegionsTooShort {
            need: pixels,
            got: pixels - 1
        }
    );
    assert_eq!(
        attempt(
            CompositionMode::Replacement,
            &regions,
            &footprint[..pixels - 1],
            &full
        ),
        analog_clock::ComposeError::FootprintTooShort {
            need: pixels,
            got: pixels - 1
        }
    );
    assert_eq!(
        attempt(
            CompositionMode::Replacement,
            &regions,
            &footprint,
            &Candidates {
                dynamic: &good[..need_bytes - 1],
                ..full
            }
        ),
        analog_clock::ComposeError::BitsTooShort {
            which: "dynamic",
            need: need_bytes,
            got: need_bytes - 1
        }
    );
    assert_eq!(
        attempt(
            CompositionMode::Removal,
            &regions,
            &footprint,
            &Candidates { mask: None, ..full }
        ),
        analog_clock::ComposeError::MissingMask
    );
    assert_eq!(
        attempt(CompositionMode::Replacement, &[], &[], &full),
        analog_clock::ComposeError::RegionsTooShort {
            need: pixels,
            got: 0
        }
    );
    assert_eq!(
        compose_surface(
            CompositionMode::Replacement,
            0,
            h,
            &regions,
            &footprint,
            &full,
            &mut Frame::new(1, 1)
        )
        .expect_err("empty"),
        analog_clock::ComposeError::EmptyFrame
    );
    // Undersized surface fails after plane checks pass.
    let mut small = Frame::new(w - 1, h);
    assert_eq!(
        compose_surface(
            CompositionMode::Replacement,
            w,
            h,
            &regions,
            &footprint,
            &full,
            &mut small
        )
        .expect_err("small surface"),
        analog_clock::ComposeError::SurfaceTooSmall
    );
    // Sanity: the physical helper agrees with the row flags' vocabulary.
    assert_eq!(physical_footprint(0.0, 1.0), Footprint::NONE);
}
