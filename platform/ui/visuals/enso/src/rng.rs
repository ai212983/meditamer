//! A small deterministic PRNG, for drawings that vary from run to run.
//!
//! SplitMix32: one addition and three xor-multiply rounds per draw, no
//! multiword state, and a full period over `u32`. It is not cryptographic and
//! is not trying to be -- what matters here is that a seed reproduces a drawing
//! exactly, on the host and on the device, forever. That is what makes a seed
//! usable as a bug report.

/// The golden-ratio increment SplitMix uses to walk its state.
const GAMMA: u32 = 0x9E37_79B9;

#[derive(Clone, Debug)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
        z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
        z ^ (z >> 16)
    }

    /// A float in `0.0..1.0`. Takes the top 24 bits, which is every bit an
    /// `f32` mantissa can hold, so the result is uniform over representable
    /// values rather than lumpy.
    pub fn unit(&mut self) -> f32 {
        const SCALE: f32 = 1.0 / (1u32 << 24) as f32;
        (self.next_u32() >> 8) as f32 * SCALE
    }

    /// A float in `low..high`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// A float in `0.0..1.0` biased toward the middle -- the average of two
    /// draws, so a triangular rather than a flat distribution.
    ///
    /// This is the one to reach for when sampling a parameter blind.
    /// Under a flat distribution, every parameter independently lands at an
    /// extreme as often as it lands in the middle, so with enough parameters
    /// some run eventually draws the worst value on all of them at once.
    /// Triangular makes that combination rare without narrowing the range.
    pub fn centred(&mut self) -> f32 {
        (self.unit() + self.unit()) * 0.5
    }

    /// One bit, for the genuinely binary choices (a direction, a mirror).
    pub fn flip(&mut self) -> bool {
        self.next_u32() & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_reproduces_its_sequence() {
        let mut first = Rng::new(12345);
        let mut second = Rng::new(12345);
        for _ in 0..64 {
            assert_eq!(first.next_u32(), second.next_u32());
        }
    }

    #[test]
    fn neighbouring_seeds_diverge_immediately() {
        // SplitMix's whole point is that sequential seeds are not correlated,
        // which matters because callers will seed from a counter.
        let first = Rng::new(1).next_u32();
        let second = Rng::new(2).next_u32();
        assert_ne!(first, second);
        assert!(first.abs_diff(second) > 1000);
    }

    #[test]
    fn unit_stays_inside_the_half_open_range() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value), "unit() produced {value}");
        }
    }

    #[test]
    fn range_respects_its_bounds() {
        let mut rng = Rng::new(99);
        for _ in 0..10_000 {
            let value = rng.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&value), "range() produced {value}");
        }
    }

    #[test]
    fn centred_concentrates_around_the_middle() {
        let mut rng = Rng::new(2024);
        let mut middle = 0u32;
        const DRAWS: u32 = 20_000;
        for _ in 0..DRAWS {
            if (0.25..0.75).contains(&rng.centred()) {
                middle += 1;
            }
        }
        // A flat distribution puts 50% in the middle half; triangular puts 75%.
        let fraction = middle as f32 / DRAWS as f32;
        assert!(fraction > 0.70, "only {fraction} landed in the middle half");
    }

    #[test]
    fn flip_is_not_stuck() {
        let mut rng = Rng::new(5);
        let heads = (0..1000).filter(|_| rng.flip()).count();
        assert!((400..600).contains(&heads), "{heads} heads in 1000");
    }
}
