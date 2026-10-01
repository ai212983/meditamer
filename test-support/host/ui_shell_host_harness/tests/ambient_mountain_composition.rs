//! Scene contract: eligible mountain paper clears underlying ink, transparent
//! edges retain sky/sun, and both coverage endpoints use the source painting.

use mountain_snow::composer::{compose_row_for_percent, cut_from_histogram, RowInputs};
use mountain_snow::pack::{crc32_ieee, parse, Pack, HEADER_LEN, MAGIC};
use ui_shell_host_harness::ambient_composer::{
    bit_at, merge_mountain_overlay, paint_mountain, FRAME_LEN, MOUNTAIN_OVERLAY_FIRST_ROW,
    MOUNTAIN_OVERLAY_LEN, MOUNTAIN_OVERLAY_PLANE_BYTES, MOUNTAIN_OVERLAY_ROWS,
    MOUNTAIN_OVERLAY_ROW_BYTES, WIDTH,
};

fn pack_file(first_row: u16, rows: usize, sections: [&[u8]; 5]) -> Vec<u8> {
    let mut payload = Vec::new();
    for section in sections {
        payload.extend_from_slice(section);
    }
    let mut file = vec![0u8; HEADER_LEN];
    file[..8].copy_from_slice(&MAGIC);
    for (index, value) in [600u16, 600, first_row, rows as u16].iter().enumerate() {
        file[8 + index * 2..10 + index * 2].copy_from_slice(&value.to_le_bytes());
    }
    for (index, section) in sections.iter().enumerate() {
        file[16 + index * 4..20 + index * 4].copy_from_slice(&(section.len() as u32).to_le_bytes());
    }
    file[36..40].copy_from_slice(&crc32_ieee(&payload).to_le_bytes());
    file.extend_from_slice(&payload);
    file
}

fn fixture_pack() -> Vec<u8> {
    let mut rock = vec![255u8; WIDTH]; // paper
    let mut snow = vec![0u8; WIDTH]; // ink
    let mut barrier = vec![0u8; WIDTH];
    let mut eligible = [0u8; WIDTH / 8];
    let noise = vec![255u8; WIDTH];
    eligible[0] = 0xC0; // only x=0,1 opaque; x=2 stays transparent
    barrier[1] = 255;
    rock[2] = 0; // ignored outside eligibility
    snow[2] = 255;
    let sections = [
        &rock[..],
        &snow[..],
        &barrier[..],
        &eligible[..],
        &noise[..],
    ];
    pack_file(331, 1, sections)
}

/// Small deterministic multi-row pack reusing the shared header/payload
/// builder. Patterns vary per row and column so first/interior/last rows
/// exercise distinct histogram, dither, and eligibility paths.
fn fixture_pack_multi(first_row: u16, rows: usize) -> Vec<u8> {
    let mut rock = vec![0u8; rows * WIDTH];
    let mut snow = vec![0u8; rows * WIDTH];
    let mut barrier = vec![0u8; rows * WIDTH];
    let mut eligible = vec![0u8; rows * WIDTH / 8];
    let mut noise = vec![0u8; rows * WIDTH];
    for row in 0..rows {
        for x in 0..WIDTH {
            rock[row * WIDTH + x] = ((x + row * 37) % 256) as u8;
            snow[row * WIDTH + x] = ((x * 3 + row * 11 + 100) % 256) as u8;
            barrier[row * WIDTH + x] = ((x + row * 53) % 256) as u8;
            noise[row * WIDTH + x] = ((x * 7 + row * 91) % 256) as u8;
            if (x + row) % 3 != 0 {
                eligible[row * WIDTH / 8 + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    pack_file(
        first_row,
        rows,
        [
            &rock[..],
            &snow[..],
            &barrier[..],
            &eligible[..],
            &noise[..],
        ],
    )
}

/// Build the streamed overlay for `pack` at `percent` with the same
/// histogram/cut and `compose_row_for_percent` row path the legacy painter
/// uses: ink plane followed by eligibility/opacity plane.
fn build_overlay(pack: &Pack<'_>, percent: u8) -> Vec<u8> {
    let header = pack.header();
    let mut histogram = [0u32; 256];
    let mut eligible_count = 0u32;
    for row in 0..header.rows {
        let barrier = pack.barrier_row(row).unwrap();
        let eligible = pack.eligible_row(row).unwrap();
        for x in 0..WIDTH {
            if eligible[x / 8] & (0x80 >> (x % 8)) != 0 {
                histogram[barrier[x] as usize] += 1;
                eligible_count += 1;
            }
        }
    }
    let target = (eligible_count * u32::from(percent) + 50) / 100;
    let cut = cut_from_histogram(&histogram, target);
    let plane = header.rows * MOUNTAIN_OVERLAY_ROW_BYTES;
    let mut overlay = vec![0u8; 2 * plane];
    for row in 0..header.rows {
        let inputs = RowInputs {
            rock: pack.rock_row(row).unwrap(),
            snow: pack.snow_row(row).unwrap(),
            barrier: pack.barrier_row(row).unwrap(),
            eligible: pack.eligible_row(row).unwrap(),
            noise: pack.noise_row(row).unwrap(),
        };
        let (ink_plane, opacity_plane) = overlay.split_at_mut(plane);
        let ink_row = &mut ink_plane
            [row * MOUNTAIN_OVERLAY_ROW_BYTES..(row + 1) * MOUNTAIN_OVERLAY_ROW_BYTES];
        compose_row_for_percent(&inputs, percent, cut, 64, ink_row).unwrap();
        opacity_plane[row * MOUNTAIN_OVERLAY_ROW_BYTES..(row + 1) * MOUNTAIN_OVERLAY_ROW_BYTES]
            .copy_from_slice(pack.eligible_row(row).unwrap());
    }
    overlay
}

fn nontrivial_base() -> Vec<u8> {
    (0..FRAME_LEN)
        .map(|i| (i.wrapping_mul(31) ^ (i >> 8)) as u8)
        .collect()
}

#[test]
fn mountain_overrides_opaque_sun_and_preserves_transparent_edges() {
    let file = fixture_pack();
    let pack = parse(&file).unwrap();
    let mut frame = vec![0xFFu8; FRAME_LEN]; // sky/sun ink under all three pixels
    assert!(paint_mountain(&mut frame, &pack, 0));
    assert!(!bit_at(&frame, WIDTH / 8, 0, 331));
    assert!(!bit_at(&frame, WIDTH / 8, 1, 331));
    assert!(bit_at(&frame, WIDTH / 8, 2, 331));
    assert!(bit_at(&frame, WIDTH / 8, 0, 330));

    let mut frame = vec![0u8; FRAME_LEN];
    assert!(paint_mountain(&mut frame, &pack, 100));
    assert!(bit_at(&frame, WIDTH / 8, 0, 331));
    assert!(bit_at(&frame, WIDTH / 8, 1, 331));
    assert!(!bit_at(&frame, WIDTH / 8, 2, 331));
}

#[test]
fn overlay_constants_match_frozen_band() {
    assert_eq!(MOUNTAIN_OVERLAY_ROW_BYTES, 75);
    assert_eq!(MOUNTAIN_OVERLAY_ROWS, 269);
    assert_eq!(MOUNTAIN_OVERLAY_FIRST_ROW, 331);
    assert_eq!(MOUNTAIN_OVERLAY_PLANE_BYTES, 20_175);
    assert_eq!(MOUNTAIN_OVERLAY_LEN, 40_350);
}

#[test]
fn streamed_overlay_matches_legacy_paint() {
    let file = fixture_pack_multi(100, 5);
    let pack = parse(&file).unwrap();
    let header = pack.header();
    assert_eq!((header.first_row, header.rows), (100, 5));
    for percent in [0u8, 1, 20, 50, 99, 100] {
        let overlay = build_overlay(&pack, percent);
        let base = nontrivial_base();
        let mut legacy = base.clone();
        let mut streamed = base.clone();
        assert!(paint_mountain(&mut legacy, &pack, percent));
        assert!(merge_mountain_overlay(
            &mut streamed,
            &overlay,
            header.first_row,
            header.rows
        ));
        assert_eq!(legacy, streamed, "percent={percent}");
        // First/interior/last rows agree; rows outside the band are untouched.
        for row in [0, header.rows / 2, header.rows - 1] {
            let start = (header.first_row + row) * MOUNTAIN_OVERLAY_ROW_BYTES;
            assert_eq!(
                legacy[start..start + MOUNTAIN_OVERLAY_ROW_BYTES],
                streamed[start..start + MOUNTAIN_OVERLAY_ROW_BYTES],
                "percent={percent} row={row}"
            );
        }
        assert_eq!(
            &legacy[..header.first_row * MOUNTAIN_OVERLAY_ROW_BYTES],
            &base[..header.first_row * MOUNTAIN_OVERLAY_ROW_BYTES]
        );
        let band_end = (header.first_row + header.rows) * MOUNTAIN_OVERLAY_ROW_BYTES;
        assert_eq!(&legacy[band_end..], &base[band_end..]);
    }
}

#[test]
fn merge_rejects_bad_input_without_mutation() {
    let file = fixture_pack_multi(100, 3);
    let pack = parse(&file).unwrap();
    let overlay = build_overlay(&pack, 50);
    let base = nontrivial_base();
    let check_untouched = |frame: &Vec<u8>| assert_eq!(*frame, base);

    // Frame length.
    let mut frame = base.clone();
    assert!(!merge_mountain_overlay(
        &mut frame[..FRAME_LEN - 1],
        &overlay,
        100,
        3
    ));
    check_untouched(&frame);

    // Overlay length (short, long, empty).
    for bad_len in [0, overlay.len() - 1, overlay.len() + 1] {
        let mut frame = base.clone();
        let bad = vec![0u8; bad_len];
        assert!(!merge_mountain_overlay(&mut frame, &bad, 100, 3));
        check_untouched(&frame);
    }

    // Overlay length mismatched for `rows`.
    let mut frame = base.clone();
    assert!(!merge_mountain_overlay(&mut frame, &overlay, 100, 2));
    check_untouched(&frame);

    // Zero rows and out-of-frame bands.
    for (first_row, rows) in [(100usize, 0usize), (599, 2), (600, 1), (usize::MAX, 1)] {
        let mut frame = base.clone();
        assert!(!merge_mountain_overlay(
            &mut frame, &overlay, first_row, rows
        ));
        check_untouched(&frame);
    }
}
