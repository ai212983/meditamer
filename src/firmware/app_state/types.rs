#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Initializing,
    Operating,
    DiagnosticsExclusive,
}

impl Phase {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::Initializing => 0,
            Self::Operating => 1,
            Self::DiagnosticsExclusive => 2,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Initializing),
            1 => Some(Self::Operating),
            2 => Some(Self::DiagnosticsExclusive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BaseMode {
    Day,
    TouchWizard,
}

impl BaseMode {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::Day => 0,
            Self::TouchWizard => 1,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Day),
            1 => Some(Self::TouchWizard),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DayBackground {
    Suminagashi,
    Shanshui,
}

impl DayBackground {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::Suminagashi => 0,
            Self::Shanshui => 1,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Suminagashi),
            1 => Some(Self::Shanshui),
            _ => None,
        }
    }

    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::Suminagashi => Self::Shanshui,
            Self::Shanshui => Self::Suminagashi,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OverlayMode {
    None,
    Clock,
}

impl OverlayMode {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Clock => 1,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Clock),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DiagKind {
    None,
    Debug,
    Test,
}

impl DiagKind {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Debug => 1,
            Self::Test => 2,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Debug),
            2 => Some(Self::Test),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DiagTargets {
    bits: u8,
}

impl DiagTargets {
    const SD_BIT: u8 = 1 << 0;
    const WIFI_BIT: u8 = 1 << 1;
    const DISPLAY_BIT: u8 = 1 << 2;
    const TOUCH_BIT: u8 = 1 << 3;
    const IMU_BIT: u8 = 1 << 4;
    const SUPPORTED_MASK: u8 =
        Self::SD_BIT | Self::WIFI_BIT | Self::DISPLAY_BIT | Self::TOUCH_BIT | Self::IMU_BIT;

    pub(crate) const fn none() -> Self {
        Self { bits: 0 }
    }

    pub(crate) const fn from_persisted(bits: u8) -> Self {
        Self {
            bits: bits & Self::SUPPORTED_MASK,
        }
    }

    pub(crate) const fn as_persisted(self) -> u8 {
        self.bits
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ServiceFlags {
    pub(crate) upload_enabled: bool,
    pub(crate) asset_reads_enabled: bool,
}

impl ServiceFlags {
    pub(crate) const fn normal() -> Self {
        Self {
            upload_enabled: false,
            asset_reads_enabled: true,
        }
    }

    pub(crate) const fn as_bits(self) -> u8 {
        (if self.upload_enabled { 1 } else { 0 })
            | (if self.asset_reads_enabled { 1 << 1 } else { 0 })
    }

    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self {
            upload_enabled: (bits & 1) != 0,
            asset_reads_enabled: (bits & (1 << 1)) != 0,
        }
    }
}
