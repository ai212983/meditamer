//! Shared lattice-cell projection for firmware and host previews.
//!
//! Forward-mapping only the centre of a one-pixel lattice cell leaves regular
//! holes when the lattice is rotated. This inverse coverage test rasterizes
//! the cell's unit square instead, while keeping cardinal views one pixel per
//! occupied site.

use super::fixed::{sin_cos, Fx, Turns, Vec2};

/// Per-frame transform from glass-local coordinates to output pixels.
#[derive(Clone, Copy)]
pub struct CellProjection {
    sin: Fx,
    cos: Fx,
    origin_x: Fx,
    origin_y: Fx,
}

impl CellProjection {
    pub fn new(angle: Turns, origin_x: i32, origin_y: i32) -> Self {
        let (sin, cos) = sin_cos(angle);
        Self {
            sin,
            cos,
            origin_x: Fx::from_int(origin_x),
            origin_y: Fx::from_int(origin_y),
        }
    }

    pub fn project(self, local: Vec2) -> (i32, i32) {
        let center = self.projected_center(local);
        (center.x.to_int(), center.y.to_int())
    }

    pub fn cell_pixels(self, local: Vec2) -> CellPixels {
        let center = self.projected_center(local);
        CellPixels {
            sin: self.sin,
            cos: self.cos,
            center,
            base_x: center.x.to_int(),
            base_y: center.y.to_int(),
            candidate: 0,
        }
    }

    fn projected_center(self, local: Vec2) -> Vec2 {
        Vec2::new(
            local.x.mul(self.cos) - local.y.mul(self.sin) + self.origin_x,
            local.x.mul(self.sin) + local.y.mul(self.cos) + self.origin_y,
        )
    }
}

/// The output pixel centres covered by one rotated unit lattice cell.
pub struct CellPixels {
    sin: Fx,
    cos: Fx,
    center: Vec2,
    base_x: i32,
    base_y: i32,
    candidate: u8,
}

impl Iterator for CellPixels {
    type Item = (i32, i32);

    fn next(&mut self) -> Option<Self::Item> {
        while self.candidate < 4 {
            let candidate = self.candidate;
            self.candidate += 1;
            let x = self.base_x + i32::from(candidate & 1);
            let y = self.base_y + i32::from(candidate >> 1);
            let dx = Fx::from_int(x) - self.center.x;
            let dy = Fx::from_int(y) - self.center.y;
            let local_x = dx.mul(self.cos) + dy.mul(self.sin);
            let local_y = dy.mul(self.cos) - dx.mul(self.sin);
            if local_x.abs() <= Fx::HALF && local_y.abs() <= Fx::HALF {
                return Some((x, y));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(usize::from(4 - self.candidate)))
    }
}

impl core::iter::FusedIterator for CellPixels {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinal_cell_occupies_exactly_its_lattice_pixel() {
        let projection = CellProjection::new(Turns::ZERO, 70, 100);
        let mut pixels = projection.cell_pixels(Vec2::new(Fx::from_int(3), Fx::from_int(-4)));
        assert_eq!(pixels.next(), Some((73, 96)));
        assert_eq!(pixels.next(), None);
    }

    #[test]
    fn rotated_dense_lattice_has_no_interior_raster_holes() {
        for degrees in [10, 15, 45, 75] {
            let projection = CellProjection::new(Turns::wrap(Fx::ratio(degrees, 360)), 20, 20);
            let mut pixels = [[false; 41]; 41];
            for y in -15..=15 {
                for x in -15..=15 {
                    for (screen_x, screen_y) in
                        projection.cell_pixels(Vec2::new(Fx::from_int(x), Fx::from_int(y)))
                    {
                        if (0..41).contains(&screen_x) && (0..41).contains(&screen_y) {
                            pixels[screen_y as usize][screen_x as usize] = true;
                        }
                    }
                }
            }
            for row in &pixels[10..=30] {
                assert!(
                    row[10..=30].iter().all(|occupied| *occupied),
                    "dense lattice had a raster hole at {degrees} degrees"
                );
            }
        }
    }
}
