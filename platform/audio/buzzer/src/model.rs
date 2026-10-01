//! Integer-only nominal model of the single-oscillator pitch network.
//!
//! The board's timer runs a nominal 50%-duty astable whose frequency is
//!
//! ```text
//! f(code) = 0.7213 / (R(code) * C)
//! R(code) = R_base + R_pot * code / CODE_MAX
//! ```
//!
//! with board-specific `R_base` (fixed resistor plus assumed wiper
//! resistance), `R_pot` (rheostat full-scale resistance), and capacitance
//! `C` folded into one numerator. All arithmetic here is integer-only so
//! firmware and host agree bit-for-bit; tests derive expectations from
//! independent floating-point math.
//!
//! Pitch resolution is non-uniform: one code step spans more cents at the
//! low-code (high-frequency) end than at the high-code end. Nearest-code
//! selection therefore compares cent distances, not code distances, and
//! resolves exact ties toward the lower code.

/// Numerator of the nominal astable frequency: `0.7213 / 100 nF` in Hz·ohm.
///
/// Boards whose timing capacitor differs from 100 nF scale this numerator;
/// the Inkplate 4 TEMPERA uses C62 = 100 nF directly.
pub const ASTABLE_NUM_HZ_OHM: u32 = 7_213_000;

/// Largest valid pitch code. Codes span `0..=CODE_MAX`.
pub const CODE_MAX: u8 = 127;

/// Board-specific timing-network parameters (ohms and code span).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OscillatorParams {
    /// Fixed series resistance plus assumed wiper resistance (R59 + Rw).
    pub r_base_ohm: u32,
    /// Rheostat full-scale resistance (RAB).
    pub r_pot_ohm: u32,
    /// Largest valid code; the span is `0..=code_max`.
    pub code_max: u32,
}

/// Nominal timing resistance in ohms for `code`.
pub const fn resistance_ohm(params: &OscillatorParams, code: u8) -> u32 {
    params.r_base_ohm + (params.r_pot_ohm * (code as u32)) / params.code_max
}

/// Nominal oscillation frequency in Hz for `code` (truncated, not rounded).
///
/// Monotonic non-increasing in `code`: a larger resistance cannot raise the
/// quotient, and integer division preserves the ordering.
pub const fn freq_hz(params: &OscillatorParams, code: u8) -> u32 {
    ASTABLE_NUM_HZ_OHM / resistance_ohm(params, code)
}

/// Nearest attainable code to `freq_hz` in cents; exact ties prefer the
/// lower code.
///
/// The comparison runs in exact resistance space (see [`nearer_code`]), not
/// on the truncated [`freq_hz`] reports: near a boundary between two codes,
/// a sub-hertz truncation can otherwise flip the choice away from the
/// physically nearer pitch.
///
/// The caller must reject non-positive requests before calling: zero would
/// divide by zero, and a zero/negative request has no nearest pitch. Values
/// above the code-0 frequency saturate to code 0 and values below the
/// code-maximum frequency saturate to the maximum code, which are the
/// nearest attainable pitches by construction.
pub fn nearest_code(params: &OscillatorParams, freq_hz: u32) -> u8 {
    let freq = freq_hz.max(1);
    let r_target = ASTABLE_NUM_HZ_OHM as u64 / freq as u64;
    // Continuous-code estimate; truncation can miss by one, so the window
    // below re-checks both neighbors and one extra step either side.
    let estimate = (r_target as i64 - params.r_base_ohm as i64) * params.code_max as i64
        / params.r_pot_ohm as i64;
    let code_max = params.code_max.min(255) as i64;
    let mut best = estimate.clamp(0, code_max) as u8;
    let mut candidate = (estimate - 2).clamp(0, code_max) as u8;
    let last = (estimate + 2).clamp(0, code_max) as u8;
    while candidate <= last {
        best = nearer_code(params, freq, best, candidate);
        if candidate == last {
            break;
        }
        candidate = candidate.saturating_add(1);
    }
    best
}

/// The nearer of two codes to `freq` in cents; exact ties prefer the lower
/// code.
///
/// Compares `max/min` resistance ratios by cross-multiplication, which orders
/// identically to comparing absolute log frequency ratios without any
/// floating point. Resistances are measured in units of `1/code_max` ohm --
/// `code_max * r_base + r_pot * code` -- which is an exact integer for every
/// code, so unlike the truncated [`freq_hz`] values this comparison cannot
/// flip a near-boundary choice. The target resistance `code_max *
/// ASTABLE_NUM_HZ_OHM / freq` is kept as a rational: both sides are scaled
/// by `freq` before the cross-multiplication, so a floored intermediate
/// never biases a near-boundary choice. Products fit `u128` for the full
/// `u32` request range.
fn nearer_code(params: &OscillatorParams, freq: u32, a: u8, b: u8) -> u8 {
    let r_unit = |code: u8| {
        params.code_max as u64 * params.r_base_ohm as u64
            + params.r_pot_ohm as u64 * u64::from(code)
    };
    let target_num = params.code_max as u128 * ASTABLE_NUM_HZ_OHM as u128;
    let scaled = |code: u8| r_unit(code) as u128 * u128::from(freq);
    let sa = scaled(a);
    let sb = scaled(b);
    let left = sa.max(target_num) * sb.min(target_num);
    let right = sb.max(target_num) * sa.min(target_num);
    if left < right {
        a
    } else if left > right {
        b
    } else {
        a.min(b)
    }
}

#[cfg(test)]
mod tests;
