#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeMode {
    Normal,
    Upload,
}

impl RuntimeMode {
    pub(crate) fn from_persisted(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Upload),
            _ => None,
        }
    }

    pub(crate) fn as_services(self) -> RuntimeServices {
        match self {
            Self::Normal => RuntimeServices::normal(),
            Self::Upload => RuntimeServices::upload_enabled(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeServices {
    bits: u8,
}

impl RuntimeServices {
    const UPLOAD_ENABLED_BIT: u8 = 1 << 0;
    const ASSET_READS_ENABLED_BIT: u8 = 1 << 1;
    const SUPPORTED_BITS: u8 = Self::UPLOAD_ENABLED_BIT | Self::ASSET_READS_ENABLED_BIT;

    pub(crate) const fn normal() -> Self {
        Self::new(false, true)
    }

    pub(crate) const fn upload_enabled() -> Self {
        Self::new(true, true)
    }

    pub(crate) const fn new(upload_enabled: bool, asset_reads_enabled: bool) -> Self {
        let mut bits = 0u8;
        if upload_enabled {
            bits |= Self::UPLOAD_ENABLED_BIT;
        }
        if asset_reads_enabled {
            bits |= Self::ASSET_READS_ENABLED_BIT;
        }
        Self { bits }
    }

    pub(crate) const fn upload_enabled_flag(self) -> bool {
        (self.bits & Self::UPLOAD_ENABLED_BIT) != 0
    }

    pub(crate) const fn asset_reads_enabled_flag(self) -> bool {
        (self.bits & Self::ASSET_READS_ENABLED_BIT) != 0
    }

    pub(crate) fn with_upload_enabled(self, enabled: bool) -> Self {
        Self::new(enabled, self.asset_reads_enabled_flag())
    }

    pub(crate) fn with_asset_reads_enabled(self, enabled: bool) -> Self {
        Self::new(self.upload_enabled_flag(), enabled)
    }

    pub(crate) const fn as_persisted(self) -> u8 {
        self.bits
    }

    pub(crate) fn from_persisted(value: u8) -> Self {
        Self {
            bits: value & Self::SUPPORTED_BITS,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    Clock,
    Suminagashi,
    Shanshui,
}

impl DisplayMode {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Clock => Self::Suminagashi,
            Self::Suminagashi => Self::Shanshui,
            Self::Shanshui => Self::Clock,
        }
    }

    pub(crate) fn as_persisted(self) -> u8 {
        match self {
            Self::Clock => 0,
            Self::Suminagashi => 1,
            Self::Shanshui => 2,
        }
    }

    pub(crate) fn from_persisted(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clock),
            1 => Some(Self::Suminagashi),
            2 => Some(Self::Shanshui),
            _ => None,
        }
    }

    pub(crate) fn toggled_reverse(self) -> Self {
        match self {
            Self::Clock => Self::Shanshui,
            Self::Suminagashi => Self::Clock,
            Self::Shanshui => Self::Suminagashi,
        }
    }
}
