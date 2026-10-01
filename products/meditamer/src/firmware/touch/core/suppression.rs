/// Drops contact samples after an ownership reset until release is observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContactSuppressionGate {
    armed: bool,
}

impl ContactSuppressionGate {
    pub(crate) const fn new() -> Self {
        Self { armed: false }
    }

    pub(crate) fn arm(&mut self) {
        self.armed = true;
    }

    pub(crate) const fn is_armed(&self) -> bool {
        self.armed
    }

    /// Returns whether the sample belongs to a newly admissible interaction.
    /// The zero-contact sample that ends suppression is itself only a boundary.
    pub(crate) fn admit(&mut self, touch_count: u8) -> bool {
        if !self.is_armed() {
            return true;
        }
        if touch_count == 0 {
            self.armed = false;
        }
        false
    }
}
