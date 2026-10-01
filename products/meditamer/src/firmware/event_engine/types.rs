#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SensorFrame {
    pub(crate) now_ms: u64,
    pub(crate) tap_src: u8,
    pub(crate) int1: bool,
    pub(crate) gx: i16,
    pub(crate) gy: i16,
    pub(crate) gz: i16,
    pub(crate) ax: i16,
    pub(crate) ay: i16,
    pub(crate) az: i16,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MotionFeatures {
    pub(crate) tap_src: u8,
    pub(crate) int1: bool,
    pub(crate) tap_axis_mask: u8,
    pub(crate) has_axis_tap: bool,
    pub(crate) has_single_tap: bool,
    pub(crate) has_tap_event: bool,
    pub(crate) jerk_l1: i32,
    pub(crate) prev_jerk_l1: i32,
    pub(crate) jerk_axis: u8,
    pub(crate) candidate_axis: u8,
    pub(crate) gyro_l1: i32,
    pub(crate) gyro_veto_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
#[repr(u8)]
pub(crate) enum EventKind {
    #[default]
    DoubleTap = 1,
    Pickup = 2,
    Placement = 3,
    StillnessStart = 4,
    StillnessEnd = 5,
    NearIntent = 6,
    FarIntent = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub(crate) struct EventDetected {
    pub(crate) kind: EventKind,
    pub(crate) confidence: u8,
    pub(crate) source_mask: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EngineAction {
    BacklightTrigger,
    EventDetected(EventDetected),
    CounterReset { reason: RejectReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionBuffer {
    len: usize,
    slots: [Option<EngineAction>; Self::MAX],
}

impl ActionBuffer {
    pub(crate) const MAX: usize = 4;

    pub(crate) const fn new() -> Self {
        Self {
            len: 0,
            slots: [None; Self::MAX],
        }
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.slots = [None; Self::MAX];
    }

    pub(crate) fn push(&mut self, action: EngineAction) {
        if self.len >= Self::MAX {
            return;
        }
        self.slots[self.len] = Some(action);
        self.len += 1;
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &EngineAction> {
        self.slots[..self.len].iter().filter_map(Option::as_ref)
    }

    pub(crate) fn contains_backlight_trigger(&self) -> bool {
        self.iter()
            .any(|action| matches!(action, EngineAction::BacklightTrigger))
    }
}

impl Default for ActionBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CandidateScore(pub u16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum RejectReason {
    #[default]
    None = 0,
    CandidateWeak = 1,
    Debounced = 2,
    GyroVeto = 3,
    AxisMismatch = 4,
    GapTooShort = 5,
    GapTooLong = 6,
    CooldownActive = 7,
    SensorFault = 8,
}

impl RejectReason {
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum EngineStateId {
    #[default]
    Idle = 0,
    TapSeq1 = 1,
    TapSeq2 = 2,
    TriggeredCooldown = 3,
    SensorFaultBackoff = 4,
}

impl EngineStateId {
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}
