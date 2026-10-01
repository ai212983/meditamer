//! Independent floating-point cross-checks of the integer pitch model.
extern crate std;

use super::*;

const INKPLATE: OscillatorParams = OscillatorParams {
    r_base_ohm: 2300,
    r_pot_ohm: 10_000,
    code_max: 127,
};

fn reference_freq_hz(code: u8) -> f64 {
    0.7213 / ((2300.0 + 10_000.0 * f64::from(code) / 127.0) * 100e-9)
}

fn reference_nearest(freq: f64) -> u8 {
    let mut best = 0u8;
    let mut best_cents = f64::INFINITY;
    for code in 0..=127u8 {
        let cents = (freq / reference_freq_hz(code)).log2().abs() * 1200.0;
        // Strict improvement only: the first (lowest) code wins exact ties.
        if cents < best_cents {
            best_cents = cents;
            best = code;
        }
    }
    best
}

#[test]
fn endpoints_match_plan_values() {
    // Plan endpoints: approximately 586.4 and 3136.1 Hz.
    assert_eq!(freq_hz(&INKPLATE, 0), 3136);
    assert_eq!(freq_hz(&INKPLATE, 127), 586);
    assert!((reference_freq_hz(0) - 3136.1).abs() < 0.2);
    assert!((reference_freq_hz(127) - 586.4).abs() < 0.2);
}

#[test]
fn mapping_is_monotonic_non_increasing() {
    for code in 0..127u8 {
        assert!(
            freq_hz(&INKPLATE, code) >= freq_hz(&INKPLATE, code + 1),
            "code {code} -> {} then {}",
            freq_hz(&INKPLATE, code),
            freq_hz(&INKPLATE, code + 1),
        );
    }
}

#[test]
fn nearest_code_matches_reference_sweep() {
    // Plan check: the nearest code to 1000 Hz is 62.
    assert_eq!(nearest_code(&INKPLATE, 1000), 62);
    // Near-boundary regression: 2049 Hz is closer to code 16 in exact
    // cents; a floored target resistance picked 15.
    assert_eq!(nearest_code(&INKPLATE, 2049), 16);
    assert_eq!(reference_nearest(2049.0), 16);
    let mut freq = 100u32;
    while freq <= 4000 {
        assert_eq!(
            nearest_code(&INKPLATE, freq),
            reference_nearest(f64::from(freq)),
            "freq {freq} Hz",
        );
        freq += 7;
    }
}

#[test]
fn nearest_code_matches_reference_every_integer_hz() {
    // Exhaustive integer-Hz check across the attainable band and its
    // saturation skirts: the integer cross-products must agree with the
    // floating-point cents oracle at every point, not just sampled steps.
    let mut freq = 1u32;
    while freq <= 5000 {
        assert_eq!(
            nearest_code(&INKPLATE, freq),
            reference_nearest(f64::from(freq)),
            "freq {freq} Hz",
        );
        freq += 1;
    }
}

#[test]
fn nearest_code_saturates_out_of_range() {
    assert_eq!(nearest_code(&INKPLATE, 5000), 0);
    assert_eq!(nearest_code(&INKPLATE, u32::MAX), 0);
    assert_eq!(nearest_code(&INKPLATE, 400), 127);
    assert_eq!(nearest_code(&INKPLATE, 1), 127);
}

#[test]
fn nearest_code_error_stays_within_half_step() {
    // Every in-range request must land within half the bracketing code step
    // in cents: the request sits between two adjacent attainable pitches, and
    // the nearer of the two is at most half their span away. The relevant
    // neighbor is on the request's side of the chosen pitch, not always the
    // next code up.
    fn span(a: u8, b: u8) -> f64 {
        (reference_freq_hz(a) / reference_freq_hz(b)).log2().abs() * 1200.0 / 2.0
    }
    let mut freq = 590u32;
    while freq <= 3130 {
        let code = nearest_code(&INKPLATE, freq);
        let actual = (f64::from(freq) / reference_freq_hz(code)).log2().abs() * 1200.0;
        let bound = if f64::from(freq) >= reference_freq_hz(code) {
            if code == 0 {
                span(0, 1)
            } else {
                span(code - 1, code)
            }
        } else if code == 127 {
            span(126, 127)
        } else {
            span(code, code + 1)
        };
        assert!(
            actual <= bound + 1e-6,
            "freq {freq} Hz code {code}: {actual} cents > half step {bound}",
        );
        freq += 13;
    }
}
