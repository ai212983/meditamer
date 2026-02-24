use core::sync::atomic::AtomicBool;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use super::types::{
    TouchEvent, TouchPipelineInput, TouchTraceSample, TouchWizardSessionEvent,
    TouchWizardSwipeTraceSample,
};

pub(crate) const TOUCH_TRACE_ENABLED: bool = true;
pub(crate) const TOUCH_EVENT_TRACE_ENABLED: bool = true;
pub(crate) const TOUCH_WIZARD_TRACE_ENABLED: bool = true;
pub(crate) const TOUCH_CALIBRATION_WIZARD_ENABLED: bool = true;
// Keep touch polling at 8 ms so gesture starts are not missed between idle ticks.
// With the current controller behavior (frequent interleaved zero frames), 16 ms
// idle cadence can collapse fast swipe starts into one-frame taps.
pub(crate) const TOUCH_SAMPLE_IDLE_MS: u64 = 8;
pub(crate) const TOUCH_SAMPLE_ACTIVE_MS: u64 = 8;
pub(crate) const TOUCH_SAMPLE_IDLE_FALLBACK_MS: u64 = 24;
pub(crate) const TOUCH_IRQ_BURST_MS: u64 = 96;
pub(crate) const TOUCH_ZERO_CONFIRM_WINDOW_MS: u64 = 40;
pub(crate) const TOUCH_INIT_RETRY_MS: u64 = 2_000;
pub(crate) const TOUCH_FEEDBACK_ENABLED: bool = true;
pub(crate) const TOUCH_FEEDBACK_RADIUS_PX: i32 = 3;
pub(crate) const TOUCH_FEEDBACK_MIN_REFRESH_MS: u64 = 30;
pub(crate) const TOUCH_MAX_CATCHUP_SAMPLES: u8 = 8;
pub(crate) const TOUCH_IMU_QUIET_WINDOW_MS: u64 = 120;
pub(crate) const TOUCH_WIZARD_TRACE_CAPTURE_TAIL_MS: u64 = 240;

pub(crate) static TOUCH_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TouchTraceSample, 32> =
    Channel::new();
pub(crate) static TOUCH_EVENT_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TouchEvent, 32> =
    Channel::new();
pub(crate) static TOUCH_WIZARD_SWIPE_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchWizardSwipeTraceSample,
    64,
> = Channel::new();
pub(crate) static TOUCH_WIZARD_RAW_TRACE_SAMPLES: Channel<
    CriticalSectionRawMutex,
    TouchTraceSample,
    256,
> = Channel::new();
pub(crate) static TOUCH_WIZARD_SESSION_EVENTS: Channel<
    CriticalSectionRawMutex,
    TouchWizardSessionEvent,
    8,
> = Channel::new();
pub(crate) static TOUCH_PIPELINE_INPUTS: Channel<CriticalSectionRawMutex, TouchPipelineInput, 32> =
    Channel::new();
pub(crate) static TOUCH_PIPELINE_EVENTS: Channel<CriticalSectionRawMutex, TouchEvent, 64> =
    Channel::new();
pub(crate) static TOUCH_IRQ_LOW: AtomicBool = AtomicBool::new(false);
