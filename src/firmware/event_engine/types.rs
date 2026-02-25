#[derive(Clone, Copy, Debug, Default)]
pub struct SensorFrame {
    pub now_ms: u64,
    pub tap_src: u8,
    pub int1: bool,
    pub gx: i16,
    pub gy: i16,
    pub gz: i16,
    pub ax: i16,
    pub ay: i16,
    pub az: i16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MotionFeatures {
    pub tap_src: u8,
    pub int1: bool,
    pub tap_axis_mask: u8,
    pub has_axis_tap: bool,
    pub has_single_tap: bool,
    pub has_tap_event: bool,
    pub jerk_l1: i32,
    pub prev_jerk_l1: i32,
    pub jerk_axis: u8,
    pub candidate_axis: u8,
    pub gyro_l1: i32,
    pub gyro_veto_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
#[repr(u8)]
pub enum EventKind {
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
pub struct EventDetected {
    pub kind: EventKind,
    pub confidence: u8,
    pub source_mask: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineAction {
    BacklightTrigger,
    EventDetected(EventDetected),
    CounterReset { reason: RejectReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionBuffer {
    len: usize,
    slots: [Option<EngineAction>; Self::MAX],
}

impl ActionBuffer {
    pub const MAX: usize = 4;

    pub const fn new() -> Self {
        Self {
            len: 0,
            slots: [None; Self::MAX],
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.slots = [None; Self::MAX];
    }

    pub fn push(&mut self, action: EngineAction) {
        if self.len >= Self::MAX {
            return;
        }
        self.slots[self.len] = Some(action);
        self.len += 1;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &EngineAction> {
        self.slots[..self.len].iter().filter_map(Option::as_ref)
    }

    pub fn contains_backlight_trigger(&self) -> bool {
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
pub struct CandidateScore(pub u16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum RejectReason {
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
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum EngineStateId {
    #[default]
    Idle = 0,
    TapSeq1 = 1,
    TapSeq2 = 2,
    TriggeredCooldown = 3,
    SensorFaultBackoff = 4,
}

impl EngineStateId {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}
