use esp_hal::gpio::Input;
use meditamer::drivers::inkplate::TouchSample;

pub(crate) type TouchIrqPin = Input<'static>;

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

#[derive(Clone, Copy, Default)]
pub(crate) struct TouchWizardSwipeTraceSample {
    pub(crate) t_ms: u64,
    pub(crate) case_index: u8,
    pub(crate) attempt: u16,
    pub(crate) expected_direction: u8,
    pub(crate) expected_speed: u8,
    pub(crate) verdict: u8,
    pub(crate) classified_direction: u8,
    pub(crate) start_x: u16,
    pub(crate) start_y: u16,
    pub(crate) end_x: u16,
    pub(crate) end_y: u16,
    pub(crate) duration_ms: u16,
    pub(crate) move_count: u16,
    pub(crate) max_travel_px: u16,
    pub(crate) release_debounce_ms: u16,
    pub(crate) dropout_count: u16,
}

#[derive(Clone, Copy)]
pub(crate) enum TouchWizardSessionEvent {
    Start { t_ms: u64 },
    End { t_ms: u64 },
}
