//! Row-streaming parity: [`StreamingDither`] must agree bit for bit with
//! whole-frame [`dither_regions`]/[`dither_flat`] on every uniform profile
//! and mixed regional labels, survive narrow/tail frames, and leave no
//! trace on failed or out-of-order rows.

use analog_clock::{
    dither_flat, dither_regions, regional_scratch_len, render_region_row, ClockScene,
    DitherAlgorithm, DitherRegion, HandMaps, Hands, RegionDithers, StreamError, StreamingDither,
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

const ALL_ALGOS: [DitherAlgorithm; 7] = [
    DitherAlgorithm::Threshold,
    DitherAlgorithm::Gradient,
    DitherAlgorithm::Bayer4,
    DitherAlgorithm::Bayer8,
    DitherAlgorithm::BlueNoise,
    DitherAlgorithm::FloydSteinberg,
    DitherAlgorithm::Atkinson,
];

fn uniform(algo: DitherAlgorithm) -> RegionDithers {
    RegionDithers {
        background: algo,
        clock: algo,
        hands: algo,
        shadows: algo,
    }
}

/// Small composed frame: real renderer output (gray + labels), so diffusion
/// and gating run over representative content rather than a toy oracle.
fn composed(width: u32, height: u32) -> (Vec<u8>, Vec<DitherRegion>) {
    let n = 8 * 8;
    let mut normal = vec![0u8; n * 3];
    for px in normal.chunks_exact_mut(3) {
        px[0] = 127;
        px[1] = 127;
        px[2] = 255;
    }
    let hour_albedo = vec![128u8; n * 3];
    let hour_alpha = vec![255u8; n];
    let minute_albedo = vec![200u8; n * 3];
    let minute_alpha = vec![255u8; n];
    let hands = Hands {
        hour: HandMaps {
            width: 8,
            height: 8,
            albedo: &hour_albedo,
            alpha: &hour_alpha,
            normal: &normal,
            spec: &hour_alpha,
            pivot_x: 3.5,
            pivot_y: 6.5,
        },
        minute: HandMaps {
            width: 8,
            height: 8,
            albedo: &minute_albedo,
            alpha: &minute_alpha,
            normal: &normal,
            spec: &minute_alpha,
            pivot_x: 3.5,
            pivot_y: 6.5,
        },
    };
    let mut scene = ClockScene::for_size(width, height);
    scene.hour_angle = 1.1;
    scene.minute_angle = 4.0;
    let pixels = width as usize * height as usize;
    let mut gray = vec![0u8; pixels];
    let mut regions = vec![DitherRegion::Background; pixels];
    for y in 0..height {
        let row = y as usize * width as usize;
        render_region_row(
            &scene,
            &hands,
            y,
            &mut gray[row..row + width as usize],
            &mut regions[row..row + width as usize],
        );
    }
    (gray, regions)
}

fn whole_frame(
    width: u32,
    height: u32,
    gray: &[u8],
    regions: &[DitherRegion],
    profiles: &RegionDithers,
) -> Vec<bool> {
    let mut scratch = vec![0.0f32; regional_scratch_len(width as usize).unwrap()];
    let mut surface = Frame::new(width, height);
    dither_regions(
        width,
        height,
        gray,
        regions,
        profiles,
        &mut scratch,
        &mut surface,
    )
    .expect("whole-frame dither");
    surface.bits
}

/// Stream one row per call with a three-row label lookahead, passing empty
/// tail slices past the bottom — the device's one-row-per-poll shape.
fn streamed(
    width: u32,
    height: u32,
    gray: &[u8],
    regions: &[DitherRegion],
    profiles: RegionDithers,
) -> Vec<bool> {
    let w = width as usize;
    let mut stream = StreamingDither::new(width, height, profiles).expect("geometry");
    // Dirty the scratch first: begin_frame must clear it.
    let mut scratch = vec![1.0f32; regional_scratch_len(w).unwrap()];
    stream.begin_frame(&mut scratch).expect("begin");
    let mut surface = Frame::new(width, height);
    for y in 0..height {
        let row = y as usize * w;
        let next = if y + 1 < height {
            &regions[(y as usize + 1) * w..(y as usize + 2) * w]
        } else {
            &[]
        };
        let next2 = if y + 2 < height {
            &regions[(y as usize + 2) * w..(y as usize + 3) * w]
        } else {
            &[]
        };
        stream
            .process_row(
                y,
                &gray[row..row + w],
                &regions[row..row + w],
                next,
                next2,
                &mut scratch,
                &mut surface,
            )
            .expect("row");
    }
    assert!(stream.is_done());
    stream.finish().expect("finish");
    surface.bits
}

#[test]
fn stream_matches_whole_frame_on_all_uniform_profiles() {
    let (gray, regions) = composed(24, 18);
    for algo in ALL_ALGOS {
        let profiles = uniform(algo);
        assert_eq!(
            streamed(24, 18, &gray, &regions, profiles),
            whole_frame(24, 18, &gray, &regions, &profiles),
            "uniform {algo:?} diverged"
        );
    }
}

#[test]
fn stream_matches_flat_pass_for_every_algorithm() {
    // A uniform Background stream is the flat pass: labels never gate.
    let (gray, regions) = composed(24, 18);
    assert!(regions.contains(&DitherRegion::Hands));
    for algo in ALL_ALGOS {
        let mut scratch = vec![0.0f32; regional_scratch_len(24).unwrap()];
        let mut flat = Frame::new(24, 18);
        dither_flat(24, 18, &gray, algo, &mut scratch, &mut flat).expect("flat");
        assert_eq!(
            streamed(24, 18, &gray, &regions, uniform(algo)),
            flat.bits,
            "flat {algo:?} diverged"
        );
    }
}

#[test]
fn stream_matches_mixed_regional_labels() {
    let (gray, regions) = composed(24, 18);
    let profiles = [
        // Diffusion/diffusion method boundary plus ordered mixes.
        RegionDithers {
            background: DitherAlgorithm::FloydSteinberg,
            clock: DitherAlgorithm::Atkinson,
            hands: DitherAlgorithm::FloydSteinberg,
            shadows: DitherAlgorithm::Atkinson,
        },
        RegionDithers {
            background: DitherAlgorithm::Atkinson,
            clock: DitherAlgorithm::FloydSteinberg,
            hands: DitherAlgorithm::Atkinson,
            shadows: DitherAlgorithm::FloydSteinberg,
        },
        RegionDithers {
            background: DitherAlgorithm::FloydSteinberg,
            clock: DitherAlgorithm::Bayer8,
            hands: DitherAlgorithm::Atkinson,
            shadows: DitherAlgorithm::Threshold,
        },
        RegionDithers {
            background: DitherAlgorithm::BlueNoise,
            clock: DitherAlgorithm::Gradient,
            hands: DitherAlgorithm::Bayer4,
            shadows: DitherAlgorithm::FloydSteinberg,
        },
    ];
    for profiles in profiles {
        assert_eq!(
            streamed(24, 18, &gray, &regions, profiles),
            whole_frame(24, 18, &gray, &regions, &profiles),
            "mixed {profiles:?} diverged"
        );
    }
}

#[test]
fn stream_matches_hand_built_fs_atkinson_boundary() {
    // Synthetic vertical method boundary: every row crosses FS | Atkinson,
    // with a gray ramp driving real error traffic both ways.
    let (w, h) = (16u32, 10u32);
    let mut gray = vec![0u8; w as usize * h as usize];
    let mut regions = vec![DitherRegion::Background; w as usize * h as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            gray[y * w as usize + x] = ((x * 17 + y * 13) % 256) as u8;
            regions[y * w as usize + x] = if x < w as usize / 2 {
                DitherRegion::Background
            } else {
                DitherRegion::Shadows
            };
        }
    }
    let profiles = RegionDithers {
        background: DitherAlgorithm::FloydSteinberg,
        clock: DitherAlgorithm::Threshold,
        hands: DitherAlgorithm::Threshold,
        shadows: DitherAlgorithm::Atkinson,
    };
    assert_eq!(
        streamed(w, h, &gray, &regions, profiles),
        whole_frame(w, h, &gray, &regions, &profiles),
        "FS/Atkinson boundary diverged"
    );
}

#[test]
fn stream_survives_narrow_frames_and_tails() {
    // Widths 1..2 exercise the x +- 2 Atkinson taps against the frame edge;
    // heights 1..2 leave the lookahead rows past the bottom.
    for (w, h) in [(1, 1), (2, 1), (1, 2), (2, 2), (1, 9), (2, 9), (3, 1)] {
        let (gray, regions) = composed(w, h);
        for algo in ALL_ALGOS {
            let profiles = uniform(algo);
            assert_eq!(
                streamed(w, h, &gray, &regions, profiles),
                whole_frame(w, h, &gray, &regions, &profiles),
                "narrow {w}x{h} uniform {algo:?} diverged"
            );
        }
        let mixed = RegionDithers {
            background: DitherAlgorithm::FloydSteinberg,
            clock: DitherAlgorithm::Atkinson,
            hands: DitherAlgorithm::FloydSteinberg,
            shadows: DitherAlgorithm::Atkinson,
        };
        assert_eq!(
            streamed(w, h, &gray, &regions, mixed),
            whole_frame(w, h, &gray, &regions, &mixed),
            "narrow {w}x{h} mixed diverged"
        );
    }
}

#[test]
fn failed_and_out_of_order_rows_corrupt_nothing() {
    let (gray, regions) = composed(12, 8);
    let profiles = RegionDithers {
        background: DitherAlgorithm::Atkinson,
        clock: DitherAlgorithm::FloydSteinberg,
        hands: DitherAlgorithm::Atkinson,
        shadows: DitherAlgorithm::FloydSteinberg,
    };
    let mut stream = StreamingDither::new(12, 8, profiles).expect("geometry");
    let mut scratch = vec![0.0f32; regional_scratch_len(12).unwrap()];
    stream.begin_frame(&mut scratch).expect("begin");
    let mut surface = Frame::new(12, 8);

    // Wrong row first: rejected, cursor unmoved.
    assert_eq!(
        stream.process_row(
            1,
            &gray[12..24],
            &regions[12..24],
            &regions[24..36],
            &regions[36..48],
            &mut scratch,
            &mut surface
        ),
        Err(StreamError::OutOfSequence {
            expected: 0,
            got: 1
        })
    );
    assert_eq!(stream.next_y(), 0);
    assert!(surface.bits.iter().all(|b| !b));
    assert!(scratch.iter().all(|v| *v == 0.0));

    // Short gray window: rejected, nothing written anywhere.
    assert_eq!(
        stream.process_row(
            0,
            &gray[..5],
            &regions[..12],
            &regions[12..24],
            &regions[24..36],
            &mut scratch,
            &mut surface
        ),
        Err(StreamError::GrayTooShort { need: 12, got: 5 })
    );
    assert_eq!(stream.next_y(), 0);
    assert!(surface.bits.iter().all(|b| !b));
    assert!(scratch.iter().all(|v| *v == 0.0));

    // Short future labels on a gated profile: rejected before any write.
    assert_eq!(
        stream.process_row(
            0,
            &gray[..12],
            &regions[..12],
            &[],
            &regions[24..36],
            &mut scratch,
            &mut surface
        ),
        Err(StreamError::RegionsTooShort { need: 12, got: 0 })
    );
    assert_eq!(stream.next_y(), 0);

    // Correct retry still completes to the whole-frame result.
    for y in 0..8u32 {
        let row = y as usize * 12;
        let next = if y + 1 < 8 {
            &regions[(y as usize + 1) * 12..(y as usize + 2) * 12]
        } else {
            &[]
        };
        let next2 = if y + 2 < 8 {
            &regions[(y as usize + 2) * 12..(y as usize + 3) * 12]
        } else {
            &[]
        };
        stream
            .process_row(
                y,
                &gray[row..row + 12],
                &regions[row..row + 12],
                next,
                next2,
                &mut scratch,
                &mut surface,
            )
            .expect("row");
    }
    stream.finish().expect("finish");
    assert_eq!(surface.bits, whole_frame(12, 8, &gray, &regions, &profiles));
    // A finished stream needs a new frame before further rows.
    assert_eq!(
        stream.process_row(
            0,
            &gray[..12],
            &regions[..12],
            &regions[12..24],
            &regions[24..36],
            &mut scratch,
            &mut surface
        ),
        Err(StreamError::NotStarted)
    );
    // Reset + re-arm reproduces the same frame.
    stream.reset();
    assert_eq!(
        stream.process_row(
            0,
            &gray[..12],
            &regions[..12],
            &regions[12..24],
            &regions[24..36],
            &mut scratch,
            &mut surface
        ),
        Err(StreamError::NotStarted)
    );
    stream.begin_frame(&mut scratch).expect("re-begin");
    let mut surface2 = Frame::new(12, 8);
    for y in 0..8u32 {
        let row = y as usize * 12;
        let next = if y + 1 < 8 {
            &regions[(y as usize + 1) * 12..(y as usize + 2) * 12]
        } else {
            &[]
        };
        let next2 = if y + 2 < 8 {
            &regions[(y as usize + 2) * 12..(y as usize + 3) * 12]
        } else {
            &[]
        };
        stream
            .process_row(
                y,
                &gray[row..row + 12],
                &regions[row..row + 12],
                next,
                next2,
                &mut scratch,
                &mut surface2,
            )
            .expect("row");
    }
    stream.finish().expect("finish");
    assert_eq!(
        surface2.bits,
        whole_frame(12, 8, &gray, &regions, &profiles)
    );
}
