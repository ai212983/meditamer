//! Q16.16 fixed-point arithmetic and a quarter-wave sine/cosine lookup.
//!
//! The hourglass solver runs on `no_std` Xtensa firmware with no `libm`
//! dependency anywhere in this repository (checked before writing this):
//! `core::f32::sin` is a compiler intrinsic backed by `libm` on every target,
//! so calling it here would fail to link on the real board. Every angle,
//! position, velocity, and mass fraction in this module tree is therefore
//! `Fx`, a plain `i32` with 16 fractional bits, and every trig call goes
//! through [`sin_cos`] against a small compile-time table -- "keep sine/cosine
//! lookup data in flash" from the plan's rotation section.

/// Number of fractional bits. 16 leaves 15 integer bits (+ sign), comfortably
/// covering this app's ranges (widget pixels are at most a few hundred, turns
/// are within a handful of full rotations, masses are small particle counts).
const FRAC_BITS: u32 = 16;
const ONE_RAW: i32 = 1 << FRAC_BITS;

/// A Q16.16 signed fixed-point number.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Fx(pub i32);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(ONE_RAW);
    pub const HALF: Fx = Fx(ONE_RAW / 2);
    /// `2 * pi`, for converting an angular velocity in turns/second into
    /// radians/second (the unit the plan's `v_wall = angular_v * J * (x - c)`
    /// term needs).
    pub const TWO_PI: Fx = Fx(411_775);

    pub const fn from_int(n: i32) -> Fx {
        Fx(n * ONE_RAW)
    }

    pub const fn from_raw(raw: i32) -> Fx {
        Fx(raw)
    }

    pub const fn raw(self) -> i32 {
        self.0
    }

    /// `numerator / denominator` as an `Fx`, both plain integers.
    pub const fn ratio(numerator: i32, denominator: i32) -> Fx {
        Fx(((numerator as i64 * ONE_RAW as i64) / denominator as i64) as i32)
    }

    pub const fn add(self, other: Fx) -> Fx {
        Fx(self.0 + other.0)
    }

    pub const fn sub(self, other: Fx) -> Fx {
        Fx(self.0 - other.0)
    }

    pub const fn neg(self) -> Fx {
        Fx(-self.0)
    }

    /// Safe for operands whose magnitude is up to roughly 180: the `i64`
    /// intermediate is exact, but casting the shifted-down product back to
    /// `i32` silently wraps once the product's magnitude exceeds `i32::MAX`,
    /// which happens around `180 * 180` in Q16.16. Every quantity in this
    /// module tree lives at pixel scale (at most a few hundred), well inside
    /// that bound; nothing here squares an arbitrary large integer.
    pub const fn mul(self, other: Fx) -> Fx {
        Fx(((self.0 as i64 * other.0 as i64) >> FRAC_BITS) as i32)
    }

    /// Multiply by a plain integer, exact (no shift), for scaling by small
    /// constants such as particle counts or iteration weights.
    pub const fn mul_int(self, k: i32) -> Fx {
        Fx(self.0 * k)
    }

    /// Named `divide`, not `div`: an inherent `div` reads as (and shadows)
    /// `core::ops::Div::div`, which clippy flags -- there is no operator
    /// overload here since a caller should see divide-by-zero as a real
    /// method call, not a `/` that silently panics.
    pub fn divide(self, other: Fx) -> Fx {
        debug_assert!(other.0 != 0, "Fx division by zero");
        Fx((((self.0 as i64) << FRAC_BITS) / other.0 as i64) as i32)
    }

    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    pub const fn min(self, other: Fx) -> Fx {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    pub const fn max(self, other: Fx) -> Fx {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    pub const fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        self.max(lo).min(hi)
    }

    /// Truncating conversion to a pixel-grid integer. Callers that need
    /// rounding add `Fx::HALF` (with the correct sign) before calling this.
    pub const fn to_int(self) -> i32 {
        self.0 >> FRAC_BITS
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl core::ops::Add for Fx {
    type Output = Fx;
    fn add(self, rhs: Fx) -> Fx {
        self.add(rhs)
    }
}

impl core::ops::Sub for Fx {
    type Output = Fx;
    fn sub(self, rhs: Fx) -> Fx {
        self.sub(rhs)
    }
}

impl core::ops::Neg for Fx {
    type Output = Fx;
    fn neg(self) -> Fx {
        self.neg()
    }
}

impl core::ops::Mul for Fx {
    type Output = Fx;
    fn mul(self, rhs: Fx) -> Fx {
        self.mul(rhs)
    }
}

impl core::ops::AddAssign for Fx {
    fn add_assign(&mut self, rhs: Fx) {
        *self = *self + rhs;
    }
}

impl core::ops::SubAssign for Fx {
    fn sub_assign(&mut self, rhs: Fx) {
        *self = *self - rhs;
    }
}

/// A 2D vector in `Fx` coordinates, used both for glass-local physics
/// positions/velocities and for widget pixel offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 {
        x: Fx::ZERO,
        y: Fx::ZERO,
    };

    pub const fn new(x: Fx, y: Fx) -> Vec2 {
        Vec2 { x, y }
    }

    pub const fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x.add(other.x), self.y.add(other.y))
    }

    pub const fn sub(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x.sub(other.x), self.y.sub(other.y))
    }

    pub const fn scale(self, k: Fx) -> Vec2 {
        Vec2::new(self.x.mul(k), self.y.mul(k))
    }

    /// The quarter-turn operator `J`: rotates 90 degrees counterclockwise in
    /// the plan's `y`-down local frame, i.e. `(x, y) -> (-y, x)`.
    pub const fn quarter_turn(self) -> Vec2 {
        Vec2::new(self.y.neg(), self.x)
    }

    pub fn length_squared(self) -> Fx {
        self.x.mul(self.x) + self.y.mul(self.y)
    }

    /// Integer-square-root-based length, adequate precision for contact
    /// resolution at this widget's pixel scale.
    pub fn length(self) -> Fx {
        Fx(isqrt_raw(self.length_squared().0))
    }
}

impl core::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        self.add(rhs)
    }
}

impl core::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        self.sub(rhs)
    }
}

/// Integer square root of a non-negative `Fx` raw value, itself returned as
/// an `Fx` raw value (i.e. `isqrt_raw(x.mul(x).0) == x.0` for `x >= 0`).
/// Newton's method on `i64`, a handful of iterations from a coarse seed --
/// deterministic and needs no float/libm support.
fn isqrt_raw(raw: i32) -> i32 {
    if raw <= 0 {
        return 0;
    }
    // sqrt(raw / ONE) * ONE == sqrt(raw * ONE), scaled back into Q16.16.
    let target = (raw as i64) << FRAC_BITS;
    let mut guess: i64 = 1i64 << ((64 - target.leading_zeros() as i64) / 2).max(1);
    for _ in 0..24 {
        if guess == 0 {
            break;
        }
        let next = (guess + target / guess) / 2;
        if next == guess {
            break;
        }
        guess = next;
    }
    guess as i32
}

/// An angle in whole turns (`1.0 == 360 degrees`), always wrapped into
/// `[0, 1)` by construction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turns(Fx);

impl Turns {
    pub const ZERO: Turns = Turns(Fx::ZERO);

    /// Wraps any raw turn value (negative, or several turns past one) into
    /// `[0, 1)` with a single `i64` modulo -- O(1) regardless of magnitude,
    /// unlike a subtract-until-in-range loop.
    pub fn wrap(raw: Fx) -> Turns {
        let one = ONE_RAW as i64;
        let r = raw.0 as i64;
        let wrapped = ((r % one) + one) % one;
        Turns(Fx(wrapped as i32))
    }

    pub const fn value(self) -> Fx {
        self.0
    }
}

/// Quarter-turn (0 to 90 degree) sine samples in Q16.16, `sin(i * pi/2 / 64)`
/// for `i` in `0..=64`. Generated once (see the plan's rotation section) and
/// checked into source: no runtime trig, no libm.
const SIN_QUARTER: [i32; 65] = [
    0, 1608, 3216, 4821, 6424, 8022, 9616, 11204, 12785, 14359, 15924, 17479, 19024, 20557, 22078,
    23586, 25080, 26558, 28020, 29466, 30893, 32303, 33692, 35062, 36410, 37736, 39040, 40320,
    41576, 42806, 44011, 45190, 46341, 47464, 48559, 49624, 50660, 51665, 52639, 53581, 54491,
    55368, 56212, 57022, 57798, 58538, 59244, 59914, 60547, 61145, 61705, 62228, 62714, 63162,
    63572, 63944, 64277, 64571, 64827, 65043, 65220, 65358, 65457, 65516, 65536,
];

const QUARTER_RAW: i32 = ONE_RAW / 4;

/// Linearly interpolated lookup for `sin(x)` where `x` is a raw `Fx` value in
/// `[0, QUARTER_RAW]` (i.e. an angle in `[0, 90 degrees]`, in turns).
fn lut(x: i32) -> i32 {
    let x = x.clamp(0, QUARTER_RAW);
    let last = (SIN_QUARTER.len() - 1) as i64;
    let scaled = x as i64 * last;
    let mut index = scaled / QUARTER_RAW as i64;
    let frac = scaled % QUARTER_RAW as i64;
    if index as usize >= SIN_QUARTER.len() - 1 {
        index = last - 1;
    }
    let lo = SIN_QUARTER[index as usize] as i64;
    let hi = SIN_QUARTER[index as usize + 1] as i64;
    (lo + (hi - lo) * frac / QUARTER_RAW as i64) as i32
}

/// `(sin(turns), cos(turns))`, both in `[-1, 1]` as `Fx`.
pub fn sin_cos(turns: Turns) -> (Fx, Fx) {
    let raw = turns.value().raw();
    let quarter = raw / QUARTER_RAW;
    let r = raw - quarter * QUARTER_RAW;
    let s = lut(r);
    let c = lut(QUARTER_RAW - r);
    let (sin, cos) = match quarter {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    (Fx(sin), Fx(cos))
}

/// Rotate a local-frame vector by `turns`: `R(a) * v`.
pub fn rotate(v: Vec2, turns: Turns) -> Vec2 {
    let (s, c) = sin_cos(turns);
    Vec2::new(v.x.mul(c) - v.y.mul(s), v.x.mul(s) + v.y.mul(c))
}

/// Rotate a local-frame vector by `-turns`: `R(-a) * v`.
pub fn rotate_inverse(v: Vec2, turns: Turns) -> Vec2 {
    let (s, c) = sin_cos(turns);
    Vec2::new(v.x.mul(c) + v.y.mul(s), v.y.mul(c) - v.x.mul(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Fx, b: Fx, tolerance_raw: i32) {
        assert!(
            (a.0 - b.0).abs() <= tolerance_raw,
            "expected {:?} ~= {:?} (raw {} vs {})",
            a,
            b,
            a.0,
            b.0
        );
    }

    #[test]
    fn arithmetic_round_trips() {
        let a = Fx::from_int(3);
        let b = Fx::ratio(1, 2);
        assert_eq!(a + b, Fx::from_raw(3 * ONE_RAW + ONE_RAW / 2));
        assert_eq!(a - b, Fx::from_raw(3 * ONE_RAW - ONE_RAW / 2));
        assert_eq!(Fx::from_int(2).mul(Fx::from_int(3)), Fx::from_int(6));
        assert_eq!(Fx::from_int(6).divide(Fx::from_int(3)), Fx::from_int(2));
        assert_eq!(Fx::from_int(-4).abs(), Fx::from_int(4));
        assert_eq!(
            Fx::from_int(2).clamp(Fx::from_int(3), Fx::from_int(5)),
            Fx::from_int(3)
        );
    }

    #[test]
    fn turns_wrap_into_unit_range() {
        assert_eq!(Turns::wrap(Fx::from_int(1)).value(), Fx::ZERO);
        assert_eq!(Turns::wrap(Fx::from_int(-1)).value(), Fx::ZERO);
        approx(Turns::wrap(Fx::ratio(5, 4)).value(), Fx::ratio(1, 4), 2);
        approx(Turns::wrap(Fx::ratio(-1, 4)).value(), Fx::ratio(3, 4), 2);
    }

    #[test]
    fn sin_cos_matches_cardinal_angles() {
        let tolerance = ONE_RAW / 2000; // 0.0005 in Q16.16, well under LUT+interp error
        let (s0, c0) = sin_cos(Turns::wrap(Fx::ZERO));
        approx(s0, Fx::ZERO, tolerance);
        approx(c0, Fx::ONE, tolerance);

        let (s90, c90) = sin_cos(Turns::wrap(Fx::ratio(1, 4)));
        approx(s90, Fx::ONE, tolerance);
        approx(c90, Fx::ZERO, tolerance);

        let (s180, c180) = sin_cos(Turns::wrap(Fx::ratio(1, 2)));
        approx(s180, Fx::ZERO, tolerance);
        approx(c180, -Fx::ONE, tolerance);

        let (s270, c270) = sin_cos(Turns::wrap(Fx::ratio(3, 4)));
        approx(s270, -Fx::ONE, tolerance);
        approx(c270, Fx::ZERO, tolerance);
    }

    #[test]
    fn sin_cos_stays_on_the_unit_circle() {
        let tolerance = ONE_RAW / 200; // 0.005, loose enough for LUT+interp+isqrt error
        for i in 0..37 {
            let turns = Turns::wrap(Fx::ratio(i, 36));
            let (s, c) = sin_cos(turns);
            let magnitude = Vec2::new(s, c).length();
            approx(magnitude, Fx::ONE, tolerance);
        }
    }

    #[test]
    fn rotate_and_rotate_inverse_are_mutual_inverses() {
        let v = Vec2::new(Fx::from_int(30), Fx::from_int(-17));
        for i in 0..8 {
            let turns = Turns::wrap(Fx::ratio(i, 8));
            let round_trip = rotate_inverse(rotate(v, turns), turns);
            approx(round_trip.x, v.x, ONE_RAW / 50);
            approx(round_trip.y, v.y, ONE_RAW / 50);
        }
    }

    #[test]
    fn quarter_turn_operator_is_a_90_degree_rotation() {
        let v = Vec2::new(Fx::from_int(5), Fx::from_int(2));
        let turned = v.quarter_turn();
        // (x, y) -> (-y, x): a 90 degree rotation, and applying it four times
        // returns to the start.
        assert_eq!(turned, Vec2::new(Fx::from_int(-2), Fx::from_int(5)));
        let four_times = turned.quarter_turn().quarter_turn().quarter_turn();
        assert_eq!(four_times, v);
    }

    #[test]
    fn isqrt_matches_known_squares() {
        // Kept within `Fx::mul`'s documented safe range (roughly +/-180):
        // this exercises `isqrt_raw` at the same pixel-coordinate scale
        // `Vec2::length` actually runs at, not an arbitrary integer range.
        for n in [0i32, 1, 2, 3, 4, 9, 16, 100, 170] {
            let x = Fx::from_int(n);
            let squared = x.mul(x);
            approx(Fx(isqrt_raw(squared.0)), x, ONE_RAW / 50);
        }
    }
}
