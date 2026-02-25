#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeMode {
    Normal,
    Upload,
}

impl RuntimeMode {
    pub(crate) fn as_persisted(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Upload => 1,
        }
    }

    pub(crate) fn from_persisted(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Upload),
            _ => None,
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
