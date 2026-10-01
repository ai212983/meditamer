//! Regional dithering contracts: metadata honesty, algorithm behavior,
//! buffer discipline. The gray row path is compared against the legacy
//! renderer it must never drift from; diffusion behavior is checked through
//! real composed frames, not through re-implemented oracles.

use analog_clock::{
    dither_regions, map_for_size, regional_scratch_len, render_gray_row, render_region_row,
    ClockScene, DitherAlgorithm, DitherRegion, HandMaps, Hands, RegionDithers, RegionalDitherError,
    Surface,
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

    fn get(&self, x: u32, y: u32) -> bool {
        self.bits[y as usize * self.width as usize + x as usize]
    }

    fn ink_count(&self) -> u32 {
        self.bits.iter().filter(|b| **b).count() as u32
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
fn shadow_scene() -> ClockScene<'static> {
    let mut scene = ClockScene::for_size(32, 32);
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
/// union and single blockers agree — with short hands, so most of the frame
/// is exposed dial where the comparison is meaningful.
fn union_scene() -> ClockScene<'static> {
    let mut scene = ClockScene::for_size(32, 32);
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

/// Compose a whole frame through the regional row path.
fn compose(scene: &ClockScene, hands: &Hands) -> (Vec<u8>, Vec<DitherRegion>) {
    let w = scene.width as usize;
    let h = scene.height as usize;
    let mut gray = vec![0u8; w * h];
    let mut regions = vec![DitherRegion::Background; w * h];
    for y in 0..scene.height {
        let row = y as usize * w;
        render_region_row(
            scene,
            hands,
            y,
            &mut gray[row..row + w],
            &mut regions[row..row + w],
        );
    }
    (gray, regions)
}

fn uniform(algo: DitherAlgorithm) -> RegionDithers {
    RegionDithers {
        background: algo,
        clock: algo,
        hands: algo,
        shadows: algo,
    }
}

fn dither(
    width: u32,
    height: u32,
    gray: &[u8],
    regions: &[DitherRegion],
    profiles: &RegionDithers,
) -> Frame {
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
    frame
}

#[test]
fn regional_gray_is_byte_identical_to_legacy() {
    let blocks = block_set();
    let hands = Hands {
        hour: hand(&blocks.ha, &blocks.hal, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let scene = shadow_scene();
    let w = scene.width as usize;
    for y in 0..scene.height {
        let mut legacy = vec![0u8; w];
        let mut gray = vec![0u8; w];
        let mut regions = vec![DitherRegion::Background; w];
        render_gray_row(&scene, &hands, y, &mut legacy);
        render_region_row(&scene, &hands, y, &mut gray, &mut regions);
        assert_eq!(gray, legacy, "gray drifted on row {y}");
    }
    // Past-the-frame rows are a no-op for both paths.
    let mut gray = vec![9u8; w];
    let mut regions = vec![DitherRegion::Hands; w];
    render_region_row(&scene, &hands, scene.height + 3, &mut gray, &mut regions);
    assert!(gray.iter().all(|&g| g == 9));
    assert!(regions.iter().all(|&r| r == DitherRegion::Hands));
    // Short slices process the shared prefix, like the legacy row does.
    let mut legacy = vec![0u8; 5];
    let mut gray = vec![0u8; 7];
    let mut narrow = vec![DitherRegion::Background; 5];
    render_gray_row(&scene, &hands, 4, &mut legacy);
    render_region_row(&scene, &hands, 4, &mut gray, &mut narrow);
    assert_eq!(&gray[..5], &legacy[..]);
}

#[test]
fn regional_gray_matches_legacy_without_valid_hands() {
    // Broken buffers render the dial alone through both paths.
    let short = vec![0u8; 4];
    let blocks = block_set();
    let hands = Hands {
        hour: hand(&short, &short, &short, &short),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    assert!(!hands.validate());
    let scene = shadow_scene();
    let w = scene.width as usize;
    for y in [0, scene.height / 2, scene.height - 1] {
        let mut legacy = vec![0u8; w];
        let mut gray = vec![0u8; w];
        let mut regions = vec![DitherRegion::Background; w];
        render_gray_row(&scene, &hands, y, &mut legacy);
        render_region_row(&scene, &hands, y, &mut gray, &mut regions);
        assert_eq!(gray, legacy, "dial-only gray drifted on row {y}");
    }
}

#[test]
fn metadata_identifies_paper_ticks_hands_and_shadow() {
    let blocks = block_set();
    let hands = Hands {
        hour: hand(&blocks.ha, &blocks.hal, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let scene = shadow_scene();
    let (_, regions) = compose(&scene, &hands);
    let mut counts = [0u32; 4];
    for r in &regions {
        counts[*r as usize] += 1;
    }
    assert!(
        counts[DitherRegion::Background as usize] > 0,
        "no paper found"
    );
    assert!(
        counts[DitherRegion::Clock as usize] > 0,
        "no ticks/hub found"
    );
    assert!(counts[DitherRegion::Hands as usize] > 0, "no hands found");
    assert!(
        counts[DitherRegion::Shadows as usize] > 0,
        "no shadow found"
    );
    // Receiver layering: the center is covered by both opaque hands, so the
    // visible top hand owns it — Hands, never Shadows.
    let center = 16 * scene.width as usize + 16;
    assert_eq!(regions[center], DitherRegion::Hands);
}

#[test]
fn duplicate_blockers_preserve_shadow_weighting() {
    // Both opaque (union) versus hour-transparent (single blocker). Wherever
    // the union scene sees no hand, neither does the single scene (its
    // coverage is a subset), so both receivers are the dial — and union
    // visibility equals single-blocker visibility for identical silhouettes
    // at identical heights. The shadow weighting, and hence the label, must
    // match there exactly, including umbra pixels. The assertion is
    // deliberately one-directional: the fringe where the hour hand shows
    // through the translucent minute legitimately reads Hands in one scene
    // and dial in the other.
    let blocks = block_set();
    let clear = vec![0u8; 64];
    let both = Hands {
        hour: hand(&blocks.ha, &blocks.hal, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let single = Hands {
        hour: hand(&blocks.ha, &clear, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let scene = union_scene();
    let (_, regions_both) = compose(&scene, &both);
    let (_, regions_single) = compose(&scene, &single);
    let mut dial_pixels = 0;
    let mut umbra = 0;
    for i in 0..regions_both.len() {
        if regions_both[i] != DitherRegion::Hands {
            dial_pixels += 1;
            assert_eq!(
                regions_single[i], regions_both[i],
                "duplicate blocker moved a dial region at pixel {i}"
            );
            if regions_both[i] == DitherRegion::Shadows {
                umbra += 1;
            }
        }
    }
    assert!(
        dial_pixels > 512,
        "too little exposed dial: {dial_pixels}/1024"
    );
    assert!(umbra > 0, "no dial umbra pixel found");
}

#[test]
fn profiles_never_touch_the_gray_plane() {
    let blocks = block_set();
    let hands = Hands {
        hour: hand(&blocks.ha, &blocks.hal, &blocks.hn, &blocks.hs),
        minute: hand(&blocks.ma, &blocks.mal, &blocks.mn, &blocks.ms),
    };
    let scene = shadow_scene();
    let (gray, regions) = compose(&scene, &hands);
    let before = gray.clone();
    let wild = RegionDithers {
        background: DitherAlgorithm::FloydSteinberg,
        clock: DitherAlgorithm::Threshold,
        hands: DitherAlgorithm::Atkinson,
        shadows: DitherAlgorithm::Bayer8,
    };
    dither(scene.width, scene.height, &gray, &regions, &wild);
    assert_eq!(gray, before, "dithering mutated the gray plane");
}

#[test]
fn uniform_profiles_hold_tone_and_repeat() {
    let (w, h) = (32, 32);
    let gray = vec![128u8; w as usize * h as usize];
    let regions = vec![DitherRegion::Background; w as usize * h as usize];
    let coverage = 1.0 - 128.0 / 255.0;
    for algo in [
        DitherAlgorithm::Gradient,
        DitherAlgorithm::Bayer4,
        DitherAlgorithm::Bayer8,
        DitherAlgorithm::BlueNoise,
        DitherAlgorithm::FloydSteinberg,
    ] {
        let first = dither(w, h, &gray, &regions, &uniform(algo));
        let second = dither(w, h, &gray, &regions, &uniform(algo));
        assert!(first.bits == second.bits, "{algo:?} is not deterministic");
        let fraction = first.ink_count() as f32 / (w * h) as f32;
        assert!(
            (fraction - coverage).abs() < 0.06,
            "{algo:?} rendered {coverage} as {fraction}"
        );
    }
}

#[test]
fn every_mode_is_binary_with_clean_extremes() {
    let algos = [
        DitherAlgorithm::Threshold,
        DitherAlgorithm::Gradient,
        DitherAlgorithm::Bayer4,
        DitherAlgorithm::Bayer8,
        DitherAlgorithm::BlueNoise,
        DitherAlgorithm::FloydSteinberg,
        DitherAlgorithm::Atkinson,
    ];
    let (w, h) = (16, 16);
    let n = w as usize * h as usize;
    let regions = vec![DitherRegion::Hands; n];
    for algo in algos {
        let white = dither(w, h, &vec![255u8; n], &regions, &uniform(algo));
        let black = dither(w, h, &vec![0u8; n], &regions, &uniform(algo));
        assert_eq!(white.ink_count(), 0, "{algo:?} inked full white");
        assert_eq!(
            black.ink_count(),
            n as u32,
            "{algo:?} left paper in full black"
        );
    }
}

#[test]
fn shared_maps_have_balanced_rank_histograms() {
    // 600x600 / 256 = 1406.25 and 400x300 / 256 = 468.75, so each level
    // appears within +-1 of flat by construction of the rank ordering.
    for (w, h, base) in [(600u32, 600u32, 1406u32), (400u32, 300u32, 468u32)] {
        let map = map_for_size(w, h).expect("prototype enables both maps");
        assert_eq!(map.data.len(), w as usize * h as usize);
        let mut counts = [0u32; 256];
        for cell in map.data {
            counts[*cell as usize] += 1;
        }
        assert!(
            counts.iter().all(|&c| c == base || c == base + 1),
            "{w}x{h} histogram out of balance"
        );
    }
}

#[test]
fn diffusion_transports_error_where_threshold_stays_blank() {
    // Uniform 128 sits just below the hard threshold, so plain Threshold
    // inks nothing while Floyd-Steinberg must carry the deficit outward.
    let (w, h) = (24, 16);
    let n = w as usize * h as usize;
    let gray = vec![128u8; n];
    let regions = vec![DitherRegion::Background; n];
    let plain = dither(w, h, &gray, &regions, &uniform(DitherAlgorithm::Threshold));
    assert_eq!(plain.ink_count(), 0);
    let spread = dither(
        w,
        h,
        &gray,
        &regions,
        &uniform(DitherAlgorithm::FloydSteinberg),
    );
    let fraction = spread.ink_count() as f32 / n as f32;
    assert!(
        (fraction - 0.5).abs() < 0.2,
        "no visible error transport: {fraction}"
    );
    assert_ne!(plain.bits, spread.bits);
}

#[test]
fn error_never_bleeds_across_method_boundaries() {
    // Left half diffuses, right half thresholds: the ordered half must match
    // a pure-threshold run pixel for pixel.
    let (w, h) = (24, 16);
    let n = w as usize * h as usize;
    let gray: Vec<u8> = (0..n).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let mut split = vec![DitherRegion::Background; n];
    for y in 0..h as usize {
        for x in w as usize / 2..w as usize {
            split[y * w as usize + x] = DitherRegion::Clock;
        }
    }
    let mixed = RegionDithers {
        background: DitherAlgorithm::FloydSteinberg,
        clock: DitherAlgorithm::Threshold,
        hands: DitherAlgorithm::Threshold,
        shadows: DitherAlgorithm::Threshold,
    };
    let run_mixed = dither(w, h, &gray, &split, &mixed);
    let run_plain = dither(w, h, &gray, &split, &uniform(DitherAlgorithm::Threshold));
    for y in 0..h {
        for x in w / 2..w {
            assert_eq!(
                run_mixed.get(x, y),
                run_plain.get(x, y),
                "diffusion leaked into the ordered half at ({x}, {y})"
            );
        }
    }
}

#[test]
fn semantic_labels_alone_never_gate_diffusion() {
    // Same uniform diffusion profile, two different label planes: identical
    // output proves region labels gate nothing when methods agree.
    let (w, h) = (24, 16);
    let n = w as usize * h as usize;
    let gray: Vec<u8> = (0..n).map(|i| ((i * 11 + 5) % 256) as u8).collect();
    let flat = vec![DitherRegion::Background; n];
    let mut split = flat.clone();
    for y in 0..h as usize {
        for x in w as usize / 2..w as usize {
            split[y * w as usize + x] = DitherRegion::Shadows;
        }
    }
    let profiles = uniform(DitherAlgorithm::FloydSteinberg);
    assert_eq!(
        dither(w, h, &gray, &flat, &profiles).bits,
        dither(w, h, &gray, &split, &profiles).bits
    );
    let atkinson = uniform(DitherAlgorithm::Atkinson);
    assert_eq!(
        dither(w, h, &gray, &flat, &atkinson).bits,
        dither(w, h, &gray, &split, &atkinson).bits
    );
}

#[test]
fn bad_buffers_and_geometry_are_rejected_cleanly() {
    let (w, h) = (8, 8);
    let n = w as usize * h as usize;
    let gray = vec![128u8; n];
    let regions = vec![DitherRegion::Background; n];
    let profiles = RegionDithers::default();
    let scratch = vec![0.0f32; regional_scratch_len(w as usize).expect("scratch")];
    let mut frame = Frame::new(w, h);

    assert_eq!(
        dither_regions(
            0,
            h,
            &gray,
            &regions,
            &profiles,
            &mut scratch.clone(),
            &mut frame
        ),
        Err(RegionalDitherError::EmptyFrame)
    );
    assert_eq!(
        dither_regions(
            w,
            h,
            &gray[..10],
            &regions,
            &profiles,
            &mut scratch.clone(),
            &mut frame
        ),
        Err(RegionalDitherError::GrayTooShort { need: n, got: 10 })
    );
    assert_eq!(
        dither_regions(
            w,
            h,
            &gray,
            &regions[..10],
            &profiles,
            &mut scratch.clone(),
            &mut frame
        ),
        Err(RegionalDitherError::RegionsTooShort { need: n, got: 10 })
    );
    let need = regional_scratch_len(w as usize).expect("scratch");
    assert_eq!(
        dither_regions(
            w,
            h,
            &gray,
            &regions,
            &profiles,
            &mut scratch[..need - 1].to_vec(),
            &mut frame
        ),
        Err(RegionalDitherError::ScratchTooShort {
            need,
            got: need - 1
        })
    );
    let mut small = Frame::new(4, 4);
    assert_eq!(
        dither_regions(
            w,
            h,
            &gray,
            &regions,
            &profiles,
            &mut scratch.clone(),
            &mut small
        ),
        Err(RegionalDitherError::SurfaceTooSmall)
    );
    // u32::MAX squared overflows `usize` on 32-bit targets (FrameTooLarge)
    // but fits on 64-bit (where the empty planes fail first instead): either
    // way an absurd frame is an error, never a wrap-around.
    assert!(dither_regions(u32::MAX, u32::MAX, &[], &[], &profiles, &mut [], &mut frame).is_err());
    assert_eq!(regional_scratch_len(usize::MAX), None);
    // Nothing was written through any of the failures.
    assert_eq!(frame.ink_count(), 0);
}

#[test]
fn scratch_is_cleared_on_every_call() {
    let (w, h) = (24, 16);
    let n = w as usize * h as usize;
    let gray: Vec<u8> = (0..n).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let regions = vec![DitherRegion::Background; n];
    let profiles = uniform(DitherAlgorithm::FloydSteinberg);
    let need = regional_scratch_len(w as usize).expect("scratch");
    let mut garbage: Vec<f32> = (0..need)
        .map(|i| if i % 2 == 0 { 1e30 } else { -1e30 })
        .collect();
    let mut clean = vec![0.0f32; need];
    let mut first = Frame::new(w, h);
    let mut second = Frame::new(w, h);
    dither_regions(w, h, &gray, &regions, &profiles, &mut garbage, &mut first).expect("dither");
    dither_regions(w, h, &gray, &regions, &profiles, &mut clean, &mut second).expect("dither");
    assert_eq!(first.bits, second.bits, "stale scratch leaked into output");
    // And repeating the garbage-seeded call repeats itself.
    let mut garbage2: Vec<f32> = (0..need)
        .map(|i| if i % 2 == 0 { 1e30 } else { -1e30 })
        .collect();
    let mut third = Frame::new(w, h);
    dither_regions(w, h, &gray, &regions, &profiles, &mut garbage2, &mut third).expect("dither");
    assert_eq!(first.bits, third.bits);
}

#[test]
fn blue_noise_native_frames_use_their_exact_size_shared_map() {
    // A flat mid-gray BlueNoise frame must reproduce the shared map's own
    // threshold decisions pixel for pixel, at both native sizes.
    let coverage = 1.0 - 128.0f32 / 255.0;
    for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
        let n = w as usize * h as usize;
        let gray = vec![128u8; n];
        let regions = vec![DitherRegion::Background; n];
        let frame = dither(w, h, &gray, &regions, &uniform(DitherAlgorithm::BlueNoise));
        let map = map_for_size(w, h).expect("prototype enables both maps");
        assert_eq!((map.width, map.height), (w, h));
        for y in 0..h {
            for x in 0..w {
                let expect = coverage > map.threshold(x as i32, y as i32);
                assert_eq!(
                    frame.get(x, y),
                    expect,
                    "map decision mismatch at ({x}, {y}) on {w}x{h}"
                );
            }
        }
    }
}

#[test]
fn blue_noise_400x300_is_not_a_crop_of_the_600_map() {
    // The small map is an independent synthesis: its bytes differ from the
    // large map's top-left window, and so does the frame rendered from it.
    let small = map_for_size(400, 300).expect("400x300 map");
    let big = map_for_size(600, 600).expect("600x600 map");
    let mut same_bytes = 0u32;
    for y in 0..300 {
        for x in 0..400 {
            if small.value(x, y) == big.value(x, y) {
                same_bytes += 1;
            }
        }
    }
    const N: u32 = 400 * 300;
    assert!(
        N - same_bytes > 1000,
        "400x300 map looks like a 600x600 crop ({same_bytes}/{N} bytes equal)"
    );
    let gray = vec![128u8; N as usize];
    let regions = vec![DitherRegion::Background; N as usize];
    let frame = dither(
        400,
        300,
        &gray,
        &regions,
        &uniform(DitherAlgorithm::BlueNoise),
    );
    let coverage = 1.0 - 128.0f32 / 255.0;
    let mut same_bits = 0u32;
    for y in 0..300u32 {
        for x in 0..400u32 {
            if frame.get(x, y) == (coverage > big.threshold(x as i32, y as i32)) {
                same_bits += 1;
            }
        }
    }
    assert!(
        N - same_bits > 1000,
        "400x300 frame matches the 600x600 crop ({same_bits}/{N} bits equal)"
    );
    assert!(
        same_bits > 1000,
        "400x300 frame looks inverted against the 600x600 crop"
    );
}

#[test]
fn blue_noise_rendered_frames_have_no_32_cell_period() {
    // The retired 32x32 tile repeated every 32 px; full-screen maps must not.
    for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
        let n = w as usize * h as usize;
        let gray = vec![128u8; n];
        let regions = vec![DitherRegion::Background; n];
        let frame = dither(w, h, &gray, &regions, &uniform(DitherAlgorithm::BlueNoise));
        let mut hdiff = 0u32;
        for y in 0..h {
            for x in 0..w - 32 {
                if frame.get(x, y) != frame.get(x + 32, y) {
                    hdiff += 1;
                }
            }
        }
        let mut vdiff = 0u32;
        for y in 0..h - 32 {
            for x in 0..w {
                if frame.get(x, y) != frame.get(x, y + 32) {
                    vdiff += 1;
                }
            }
        }
        assert!(
            hdiff > 1000,
            "{w}x{h}: horizontal 32-repeat ({hdiff} differ)"
        );
        assert!(vdiff > 1000, "{w}x{h}: vertical 32-repeat ({vdiff} differ)");
    }
}

#[test]
fn blue_noise_threshold_sampling_ignores_region_labels() {
    // Region labels steer which algorithm runs, never the threshold value:
    // one uniform BlueNoise profile over three different label planes must
    // render bit-identical frames that each match the shared map directly.
    let (w, h) = (400u32, 300u32);
    let n = w as usize * h as usize;
    let gray: Vec<u8> = (0..n).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let flat = vec![DitherRegion::Background; n];
    let hands = vec![DitherRegion::Hands; n];
    let mut split = flat.clone();
    for y in 0..h as usize {
        for x in w as usize / 2..w as usize {
            split[y * w as usize + x] = DitherRegion::Shadows;
        }
    }
    let profiles = uniform(DitherAlgorithm::BlueNoise);
    let from_flat = dither(w, h, &gray, &flat, &profiles);
    let from_hands = dither(w, h, &gray, &hands, &profiles);
    let from_split = dither(w, h, &gray, &split, &profiles);
    assert_eq!(from_flat.bits, from_hands.bits, "label changed sampling");
    assert_eq!(from_flat.bits, from_split.bits, "label changed sampling");
    let map = map_for_size(w, h).expect("prototype enables both maps");
    for y in 0..h {
        for x in 0..w {
            let idx = y as usize * w as usize + x as usize;
            let coverage = 1.0 - f32::from(gray[idx]) * (1.0 / 255.0);
            assert_eq!(
                from_split.get(x, y),
                coverage > map.threshold(x as i32, y as i32),
                "label-plane pixel mismatches the shared map at ({x}, {y})"
            );
        }
    }
}

#[test]
fn dithering_leaves_gray_and_region_planes_immutable() {
    // No algorithm — BlueNoise included — may write back into its inputs.
    let (w, h) = (24u32, 16u32);
    let n = w as usize * h as usize;
    let gray: Vec<u8> = (0..n).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let mut regions = vec![DitherRegion::Background; n];
    for y in 0..h as usize {
        for x in w as usize / 2..w as usize {
            regions[y * w as usize + x] = DitherRegion::Clock;
        }
    }
    for algo in [
        DitherAlgorithm::Threshold,
        DitherAlgorithm::Gradient,
        DitherAlgorithm::Bayer4,
        DitherAlgorithm::Bayer8,
        DitherAlgorithm::BlueNoise,
        DitherAlgorithm::FloydSteinberg,
        DitherAlgorithm::Atkinson,
    ] {
        let gray_before = gray.clone();
        let regions_before = regions.clone();
        dither(w, h, &gray, &regions, &uniform(algo));
        assert_eq!(gray, gray_before, "{algo:?} mutated the gray plane");
        assert_eq!(regions, regions_before, "{algo:?} mutated the region plane");
    }
}

#[test]
fn ordered_non_blue_fields_do_not_consult_the_shared_map() {
    // Constant and exactly-periodic fields at native size: a map-driven
    // field would speckle instead.
    let (w, h) = (600u32, 600u32);
    let n = w as usize * h as usize;
    let gray = vec![128u8; n];
    let regions = vec![DitherRegion::Background; n];
    let plain = dither(w, h, &gray, &regions, &uniform(DitherAlgorithm::Threshold));
    assert_eq!(plain.ink_count(), 0, "threshold field is not constant");
    for (algo, period) in [
        (DitherAlgorithm::Bayer4, 4u32),
        (DitherAlgorithm::Bayer8, 8u32),
    ] {
        let frame = dither(w, h, &gray, &regions, &uniform(algo));
        for y in 0..h {
            for x in 0..w - period {
                assert_eq!(
                    frame.get(x, y),
                    frame.get(x + period, y),
                    "{algo:?} broke horizontal period {period} at ({x}, {y})"
                );
            }
        }
        for y in 0..h - period {
            for x in 0..w {
                assert_eq!(
                    frame.get(x, y),
                    frame.get(x, y + period),
                    "{algo:?} broke vertical period {period} at ({x}, {y})"
                );
            }
        }
    }
}
