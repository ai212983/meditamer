//! The two shaping functions this crate needs that `libm` does not carry.

/// Linear interpolation from `a` to `b`.
#[inline]
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite interpolation between two edges: 0 at or below `edge0`, 1 at or
/// above `edge1`, a smooth ramp between. Returns a step when the span is empty
/// rather than dividing by zero.
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_hits_both_ends() {
        assert_eq!(lerp(2.0, 6.0, 0.0), 2.0);
        assert_eq!(lerp(2.0, 6.0, 1.0), 6.0);
        assert_eq!(lerp(2.0, 6.0, 0.5), 4.0);
    }

    #[test]
    fn smoothstep_is_clamped_and_monotone() {
        assert_eq!(smoothstep(1.0, 2.0, 0.0), 0.0);
        assert_eq!(smoothstep(1.0, 2.0, 3.0), 1.0);
        let mut previous = 0.0;
        for step in 0..=100 {
            let value = smoothstep(0.0, 1.0, step as f32 / 100.0);
            assert!(value >= previous, "dipped at {step}");
            previous = value;
        }
    }

    #[test]
    fn smoothstep_survives_an_empty_span() {
        assert_eq!(smoothstep(1.0, 1.0, 0.9), 0.0);
        assert_eq!(smoothstep(1.0, 1.0, 1.1), 1.0);
    }
}
