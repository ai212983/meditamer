#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirtyArea {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl DirtyArea {
    pub fn union(self, other: Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }
}
