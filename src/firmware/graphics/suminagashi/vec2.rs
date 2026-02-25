use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct Vec2Fx {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2Fx {
    #[inline]
    pub const fn new(x: Fx, y: Fx) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn dot(self, other: Self) -> Fx {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn norm2(self) -> Fx {
        self.dot(self)
    }

    #[inline]
    pub fn norm(self) -> Fx {
        let n2 = self.norm2();
        if n2 <= FX_ZERO {
            FX_ZERO
        } else {
            sqrt_fx(n2)
        }
    }
}

impl core::ops::Add for Vec2Fx {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl core::ops::Sub for Vec2Fx {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl core::ops::Mul<Fx> for Vec2Fx {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Fx) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl core::ops::Div<Fx> for Vec2Fx {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Fx) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}
