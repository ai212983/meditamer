#[derive(Clone, Copy, Debug)]
pub(crate) struct Admission {
    epoch: u32,
    transition_epoch: Option<u32>,
    contact_present: Option<bool>,
    suppress_until_release: bool,
    presented: bool,
    pending_resets: u32,
}

impl Admission {
    pub(crate) const fn new() -> Self {
        Self {
            epoch: 1,
            transition_epoch: None,
            contact_present: None,
            suppress_until_release: false,
            presented: false,
            pending_resets: 0,
        }
    }
    fn next(value: u32) -> u32 {
        let value = value.wrapping_add(1);
        if value == 0 {
            1
        } else {
            value
        }
    }
    pub(crate) fn begin_transition(&mut self) -> u32 {
        self.epoch = Self::next(self.epoch);
        self.transition_epoch = Some(self.epoch);
        self.presented = false;
        self.suppress_until_release = true;
        self.epoch
    }
    pub(crate) fn presentation_complete(&mut self, epoch: u32) -> Option<u32> {
        if self.transition_epoch != Some(epoch) {
            return None;
        }
        self.presented = true;
        self.try_complete()
    }
    pub(crate) fn reset_requested(&mut self) {
        self.pending_resets = self
            .pending_resets
            .checked_add(1)
            .expect("reset count overflow");
    }
    pub(crate) fn resets_completed(&mut self, count: u32) -> Option<u32> {
        self.pending_resets = self
            .pending_resets
            .checked_sub(count)
            .expect("unrequested reset");
        self.try_complete()
    }
    fn try_complete(&mut self) -> Option<u32> {
        if !self.presented || self.pending_resets != 0 {
            return None;
        }
        let completed = self.transition_epoch.take()?;
        self.epoch = Self::next(self.epoch);
        self.presented = false;
        self.suppress_until_release = self.contact_present != Some(false);
        Some(completed)
    }
    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch
    }
    pub(crate) fn accepts(&self, epoch: u32) -> bool {
        self.transition_epoch.is_none() && !self.suppress_until_release && self.epoch == epoch
    }
    pub(crate) fn observe(&mut self, read_epoch: u32, count: u8) -> bool {
        if read_epoch != self.epoch {
            // A read spanning a boundary cannot introduce a press, but its
            // observed contact must not reappear as fresh on the next poll.
            // Acquisition is serial: only a subsequent authoritative release
            // can establish that this contact has ended. A stale zero never
            // clears a newer hold.
            if count > 0 {
                self.contact_present = Some(true);
                self.suppress_until_release = true;
            }
            return false;
        }
        self.contact_present = Some(count > 0);
        if self.transition_epoch.is_some() {
            return false;
        }
        if self.suppress_until_release {
            if count == 0 {
                self.suppress_until_release = false;
            }
            return false;
        }
        self.epoch == read_epoch
    }
}
