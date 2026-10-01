//! Shared, allocation-free hourglass geometry helpers.

/// Evaluate one side of a cubic Bezier bulb in integer cell space.
///
/// The control widths are 47% and 97% of the span from throat to rim. That
/// makes the bulb visibly rounded while keeping the one-pixel production
/// lattice's wall slope at or below one cell per row: grains can always slide
/// along it instead of resting on a discrete overhang.
pub const fn cubic_bezier_half_width_cells(
    y: i32,
    throat_half_width: i32,
    rim_half_width: i32,
    half_height: i32,
) -> i32 {
    debug_assert!(half_height > 0);
    debug_assert!(rim_half_width >= throat_half_width);

    let height = half_height as i64;
    let absolute_y = (y as i64).abs();
    let distance = if absolute_y < height {
        absolute_y
    } else {
        height
    };
    let inverse = height - distance;
    let span = (rim_half_width - throat_half_width) as i64;
    let throat = throat_half_width as i64;
    let control_1 = throat + span * 47 / 100;
    let control_2 = throat + span * 97 / 100;
    let rim = rim_half_width as i64;
    let numerator = throat * inverse * inverse * inverse
        + 3 * control_1 * inverse * inverse * distance
        + 3 * control_2 * inverse * distance * distance
        + rim * distance * distance * distance;
    (numerator / (height * height * height)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_profile_meets_throat_and_rim_and_opens_monotonically() {
        const THROAT: i32 = 0;
        const RIM: i32 = 58;
        const HEIGHT: i32 = 85;
        assert_eq!(
            cubic_bezier_half_width_cells(0, THROAT, RIM, HEIGHT),
            THROAT
        );
        assert_eq!(
            cubic_bezier_half_width_cells(HEIGHT, THROAT, RIM, HEIGHT),
            RIM
        );
        assert_eq!(
            cubic_bezier_half_width_cells(-HEIGHT, THROAT, RIM, HEIGHT),
            RIM
        );

        let mut previous = THROAT;
        for y in 1..=HEIGHT {
            let width = cubic_bezier_half_width_cells(y, THROAT, RIM, HEIGHT);
            assert!(width >= previous);
            assert!(
                width - previous <= 1,
                "wall overhang between rows {} and {}",
                y - 1,
                y
            );
            previous = width;
        }
    }

    #[test]
    fn bezier_bulb_is_rounder_and_more_open_than_the_old_linear_taper() {
        const RIM: i32 = 58;
        const HEIGHT: i32 = 85;
        let halfway = HEIGHT / 2;
        let curved = cubic_bezier_half_width_cells(halfway, 0, RIM, HEIGHT);
        let linear = RIM * halfway / HEIGHT;
        assert!(curved > linear);
    }
}
