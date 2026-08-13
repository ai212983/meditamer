use crate::platform::inkplate::TouchSample;
use esp_hal::gpio::Input;

pub(crate) type Gpio36InputPin = Input<'static>;

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
    pub(crate) contact_x: u16,
    pub(crate) contact_y: u16,
    pub(crate) start_x: u16,
    pub(crate) start_y: u16,
    pub(crate) duration_ms: u16,
    pub(crate) touch_count: u8,
    pub(crate) move_count: u16,
    pub(crate) max_travel_px: u16,
    pub(crate) release_debounce_ms: u16,
    pub(crate) dropout_count: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct TouchSampleFrame {
    pub(crate) t_ms: u64,
    pub(crate) sample: TouchSample,
}

#[derive(Clone, Copy)]
pub(crate) enum TouchPipelineInput {
    Sample(TouchSampleFrame),
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchStatus {
    Initializing,
    Ready { x_res: u16, y_res: u16 },
    Fault,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TouchActivitySnapshot {
    pub(crate) active: bool,
    pub(crate) last_nonzero_ms: Option<u64>,
}

#[derive(Clone, Copy)]
#[cfg(not(feature = "wifi-debug-slim-app"))]
pub(crate) struct TouchTraceSample {
    pub(crate) t_ms: u64,
    pub(crate) count: u8,
    pub(crate) x0: u16,
    pub(crate) y0: u16,
    pub(crate) x1: u16,
    pub(crate) y1: u16,
    pub(crate) raw: [u8; 8],
}

#[cfg(not(feature = "wifi-debug-slim-app"))]
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
