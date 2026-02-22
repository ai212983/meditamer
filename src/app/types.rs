use embassy_time::Instant;
use esp_hal::{gpio::Output, uart::Uart, Async};
use meditamer::{
    inkplate_hal::{InkplateHal, TouchSample},
    platform::{BusyDelay, HalI2c},
};

use crate::{app::store::ModeStore, sd_probe};

pub(crate) type InkplateDriver = InkplateHal<HalI2c<'static>, BusyDelay>;
pub(crate) type SerialUart = Uart<'static, Async>;
pub(crate) type SdProbeDriver = sd_probe::SdCardProbe<'static>;

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    Refresh { uptime_seconds: u32 },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    Touch(TouchEvent),
    ForceRepaint,
    ForceMarbleRepaint,
    SdProbe,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncCommand {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncState {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
    pub(crate) sync_instant: Instant,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchSwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchEventKind {
    Down,
    Move,
    Up,
    Tap,
    LongPress,
    Swipe(TouchSwipeDirection),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TouchEvent {
    pub(crate) kind: TouchEventKind,
    pub(crate) t_ms: u64,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) start_x: u16,
    pub(crate) start_y: u16,
    pub(crate) duration_ms: u16,
    pub(crate) touch_count: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct TapTraceSample {
    pub(crate) t_ms: u64,
    pub(crate) tap_src: u8,
    pub(crate) seq_count: u8,
    pub(crate) tap_candidate: u8,
    pub(crate) cand_src: u8,
    pub(crate) state_id: u8,
    pub(crate) reject_reason: u8,
    pub(crate) candidate_score: u16,
    pub(crate) window_ms: u16,
    pub(crate) cooldown_active: u8,
    pub(crate) jerk_l1: i32,
    pub(crate) motion_veto: u8,
    pub(crate) gyro_l1: i32,
    pub(crate) int1: u8,
    pub(crate) int2: u8,
    pub(crate) power_good: i16,
    pub(crate) battery_percent: i16,
    pub(crate) gx: i16,
    pub(crate) gy: i16,
    pub(crate) gz: i16,
    pub(crate) ax: i16,
    pub(crate) ay: i16,
    pub(crate) az: i16,
}

#[derive(Clone, Copy)]
pub(crate) struct TouchTraceSample {
    pub(crate) t_ms: u64,
    pub(crate) count: u8,
    pub(crate) x0: u16,
    pub(crate) y0: u16,
    pub(crate) x1: u16,
    pub(crate) y1: u16,
    pub(crate) raw: [u8; 8],
}

impl TouchTraceSample {
    pub(crate) fn from_sample(t_ms: u64, sample: TouchSample) -> Self {
        Self {
            t_ms,
            count: sample.touch_count,
            x0: sample.points[0].x,
            y0: sample.points[0].y,
            x1: sample.points[1].x,
            y1: sample.points[1].y,
            raw: sample.raw,
        }
    }
}

pub(crate) struct DisplayContext {
    pub(crate) inkplate: InkplateDriver,
    pub(crate) sd_probe: SdProbeDriver,
    pub(crate) mode_store: ModeStore<'static>,
    pub(crate) _panel_pins: PanelPinHold<'static>,
}

pub(crate) struct PanelPinHold<'d> {
    pub(crate) _cl: Output<'d>,
    pub(crate) _le: Output<'d>,
    pub(crate) _d0: Output<'d>,
    pub(crate) _d1: Output<'d>,
    pub(crate) _d2: Output<'d>,
    pub(crate) _d3: Output<'d>,
    pub(crate) _d4: Output<'d>,
    pub(crate) _d5: Output<'d>,
    pub(crate) _d6: Output<'d>,
    pub(crate) _d7: Output<'d>,
    pub(crate) _ckv: Output<'d>,
    pub(crate) _sph: Output<'d>,
}
