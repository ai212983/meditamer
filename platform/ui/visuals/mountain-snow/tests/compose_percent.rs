//! `compose_row_for_percent` contract: endpoints, interior, errors.

use mountain_snow::composer::{
    compose_row, compose_row_for_percent, ComposerError, Cut, RowInputs,
};

fn inputs<'a>(
    rock: &'a [u8; 8],
    snow: &'a [u8; 8],
    barrier: &'a [u8; 8],
    eligible: &'a [u8; 1],
    noise: &'a [u8; 8],
) -> RowInputs<'a> {
    RowInputs {
        rock,
        snow,
        barrier,
        eligible,
        noise,
    }
}

/// Independent copy of the pre-refactor endpoint formula from
/// `products/meditamer/.../ambient_view/composer.rs`.
fn endpoint_reference(endpoint: &[u8], eligible: &[u8], noise: &[u8]) -> [u8; 1] {
    let mut out = [0u8; 1];
    for x in 0..endpoint.len() {
        let mask = 0x80 >> (x % 8);
        let ink = eligible[x / 8] & mask != 0
            && u32::from(endpoint[x]) * 256 <= u32::from(noise[x]) * 255 + 127;
        if ink {
            out[x / 8] |= mask;
        } else {
            out[x / 8] &= !mask;
        }
    }
    out
}

#[test]
fn zero_dithers_rock_and_ignores_snow_cut_band() {
    let rock = [60u8; 8];
    let snow = [220u8; 8];
    let barrier = [0u8; 8];
    let eligible = [0xFFu8];
    let noise = [250u8; 8];
    let mut out = [0xAAu8];
    compose_row_for_percent(
        &inputs(&rock, &snow, &barrier, &eligible, &noise),
        0,
        Cut {
            level: 0,
            frac: 256,
        },
        64,
        &mut out,
    )
    .expect("valid");
    // Gray 60 vs noise 250: 256*60=15360 <= 255*250+127=63877 -> ink.
    assert_eq!(out, [0xFF]);
    // Same call must ignore a snow-favoring cut/band: still rock.
    let mut out2 = [0u8];
    compose_row_for_percent(
        &inputs(&rock, &snow, &barrier, &eligible, &noise),
        0,
        Cut {
            level: 255,
            frac: 0,
        },
        0,
        &mut out2,
    )
    .expect("valid");
    assert_eq!(out2, [0xFF]);
}

#[test]
fn hundred_dithers_snow() {
    let rock = [60u8; 8];
    let snow = [220u8; 8];
    let barrier = [200u8; 8];
    let eligible = [0xFFu8];
    let noise = [0u8; 8];
    let mut out = [0xFFu8];
    compose_row_for_percent(
        &inputs(&rock, &snow, &barrier, &eligible, &noise),
        100,
        Cut { level: 0, frac: 0 },
        0,
        &mut out,
    )
    .expect("valid");
    // Gray 220 vs noise 0: 256*220=56320 > 127 -> paper.
    assert_eq!(out, [0x00]);
}

#[test]
fn interior_delegates_to_compose_row() {
    let rock = [60u8; 8];
    let snow = [220u8; 8];
    let barrier = [7u8; 8];
    let eligible = [0xFFu8];
    let noise = [200u8; 8];
    let cut = Cut {
        level: 7,
        frac: 128,
    };
    for percent in [1u8, 50, 99] {
        let mut via_helper = [0u8];
        let mut via_direct = [0u8];
        compose_row_for_percent(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            percent,
            cut,
            64,
            &mut via_helper,
        )
        .expect("valid");
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            cut,
            64,
            &mut via_direct,
        )
        .expect("valid");
        assert_eq!(via_helper, via_direct, "percent {percent}");
    }
}

#[test]
fn invalid_percent_rejected_without_touching_output() {
    let row = [0u8; 8];
    let packed = [0u8; 1];
    let good = inputs(&row, &row, &row, &packed, &row);
    let cut = Cut { level: 0, frac: 0 };
    for percent in [101u8, 200, 255] {
        let mut out = [0xA5u8];
        assert_eq!(
            compose_row_for_percent(&good, percent, cut, 64, &mut out),
            Err(ComposerError::InvalidPercent)
        );
        assert_eq!(out, [0xA5], "percent {percent} must not touch output");
    }
}

#[test]
fn ineligible_pixels_stay_paper_on_all_paths() {
    let rock = [0u8; 8]; // black would always ink if eligible
    let snow = [0u8; 8];
    let barrier = [0u8; 8];
    let eligible = [0x00u8];
    let noise = [255u8; 8];
    let cut = Cut {
        level: 0,
        frac: 256,
    };
    for percent in [0u8, 50, 100] {
        let mut out = [0xFFu8];
        compose_row_for_percent(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            percent,
            cut,
            64,
            &mut out,
        )
        .expect("valid");
        assert_eq!(out, [0x00], "percent {percent}");
    }
}

#[test]
fn endpoints_match_pre_refactor_formula_byte_for_byte() {
    // Deterministic sweep: endpoint grays, noise, eligibility patterns.
    let cases: &[[u8; 8]] = &[
        [0, 1, 127, 128, 200, 220, 254, 255],
        [255, 254, 128, 127, 55, 60, 1, 0],
        [60, 60, 60, 60, 60, 60, 60, 60],
        [220, 220, 220, 220, 220, 220, 220, 220],
    ];
    let noises: &[[u8; 8]] = &[
        [0, 0, 0, 0, 0, 0, 0, 0],
        [255, 255, 255, 255, 255, 255, 255, 255],
        [0, 32, 64, 96, 128, 160, 200, 250],
        [250, 200, 160, 128, 96, 64, 32, 0],
    ];
    let eligibles: &[[u8; 1]] = &[[0xFF], [0x00], [0xAA], [0x55], [0xC0]];
    let barrier = [7u8; 8];
    let cut = Cut {
        level: 7,
        frac: 128,
    };
    for endpoint in cases {
        for noise in noises {
            for eligible in eligibles {
                let filler = [9u8; 8];
                // percent 0 uses rock plane: put sweep in rock.
                let mut out = [0u8];
                compose_row_for_percent(
                    &inputs(endpoint, &filler, &barrier, eligible, noise),
                    0,
                    cut,
                    77,
                    &mut out,
                )
                .expect("valid");
                assert_eq!(out, endpoint_reference(endpoint, eligible, noise));
                // percent 100 uses snow plane: put sweep in snow.
                let mut out = [0u8];
                compose_row_for_percent(
                    &inputs(&filler, endpoint, &barrier, eligible, noise),
                    100,
                    cut,
                    77,
                    &mut out,
                )
                .expect("valid");
                assert_eq!(out, endpoint_reference(endpoint, eligible, noise));
            }
        }
    }
}

#[test]
fn endpoint_length_errors_match_compose_row() {
    let row = [0u8; 8];
    let short = [0u8; 7];
    let packed = [0u8; 1];
    let cut = Cut { level: 0, frac: 0 };
    let bad = RowInputs {
        rock: &short,
        snow: &row,
        barrier: &row,
        eligible: &packed,
        noise: &row,
    };
    let mut out = [0u8; 1];
    assert_eq!(
        compose_row_for_percent(&bad, 0, cut, 0, &mut out),
        Err(ComposerError::RowLength)
    );
    assert_eq!(
        compose_row_for_percent(&bad, 100, cut, 0, &mut out),
        Err(ComposerError::RowLength)
    );
    let good = inputs(&row, &row, &row, &packed, &row);
    let mut empty = [0u8; 0];
    assert_eq!(
        compose_row_for_percent(&good, 0, cut, 0, &mut empty),
        Err(ComposerError::PackedLength)
    );
}
