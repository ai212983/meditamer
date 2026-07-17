use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

use super::types::{
    TouchActivitySnapshot, TouchEvent, TouchPipelineInput, TouchStatus, TouchTraceSample,
    TouchWizardSessionEvent, TouchWizardSwipeTraceSample,
};

pub(crate) const TOUCH_TRACE_ENABLED: bool = true;
pub(crate) const TOUCH_EVENT_TRACE_ENABLED: bool = false;
pub(crate) const TOUCH_WIZARD_TRACE_ENABLED: bool = false;
pub(crate) const TOUCH_CALIBRATION_WIZARD_ENABLED: bool = false;
pub(crate) const GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED: bool = false;
// Keep touch polling at 8 ms so gesture starts are not missed between idle ticks.
// With the current controller behavior (frequent interleaved zero frames), 16 ms
// idle cadence can collapse fast swipe starts into one-frame taps.
pub(crate) const TOUCH_SAMPLE_ACTIVE_MS: u64 = 8;
pub(crate) const TOUCH_INIT_RETRY_MS: u64 = 2_000;
pub(crate) const TOUCH_FEEDBACK_ENABLED: bool = true;
pub(crate) const TOUCH_FEEDBACK_RADIUS_PX: i32 = 3;
pub(crate) const TOUCH_FEEDBACK_MIN_REFRESH_MS: u64 = 30;
pub(crate) const TOUCH_IMU_QUIET_WINDOW_MS: u64 = 120;

const TOUCH_TRACE_SAMPLES_CAP: usize = if TOUCH_TRACE_ENABLED { 8 } else { 1 };
const TOUCH_EVENT_TRACE_SAMPLES_CAP: usize = if TOUCH_EVENT_TRACE_ENABLED { 16 } else { 1 };
const TOUCH_WIZARD_SWIPE_TRACE_SAMPLES_CAP: usize = if TOUCH_WIZARD_TRACE_ENABLED { 16 } else { 1 };
const TOUCH_WIZARD_RAW_TRACE_SAMPLES_CAP: usize = if TOUCH_WIZARD_TRACE_ENABLED { 64 } else { 1 };

pub(crate) static TOUCH_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchTraceSample,
    TOUCH_TRACE_SAMPLES_CAP,
> = Channel::new();
pub(crate) static TOUCH_EVENT_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchEvent,
    TOUCH_EVENT_TRACE_SAMPLES_CAP,
> = Channel::new();
pub(crate) static TOUCH_WIZARD_SWIPE_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchWizardSwipeTraceSample,
    TOUCH_WIZARD_SWIPE_TRACE_SAMPLES_CAP,
> = Channel::new();
pub(crate) static TOUCH_WIZARD_RAW_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchTraceSample,
    TOUCH_WIZARD_RAW_TRACE_SAMPLES_CAP,
> = Channel::new();
pub(crate) static TOUCH_WIZARD_SESSION_EVENTS: Channel<
    CriticalSectionRawMutex,
    TouchWizardSessionEvent,
    4,
> = Channel::new();
pub(crate) static TOUCH_PIPELINE_INPUTS: Channel<CriticalSectionRawMutex, TouchPipelineInput, 32> =
    Channel::new();
pub(crate) static TOUCH_PIPELINE_EVENTS: Channel<CriticalSectionRawMutex, TouchEvent, 64> =
    Channel::new();
pub(crate) static TOUCH_IMU_ACTIVITY: Signal<CriticalSectionRawMutex, TouchActivitySnapshot> =
    Signal::new();
pub(crate) static TOUCH_IMU_STATUS: Signal<CriticalSectionRawMutex, TouchStatus> = Signal::new();
