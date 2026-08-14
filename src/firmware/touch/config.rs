use core::sync::atomic::{AtomicBool, AtomicU8};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

#[cfg(not(feature = "wifi-debug-slim-app"))]
use super::types::TouchTraceSample;
use super::{
    lvgl_multitouch::LvglMultitouchFrame,
    types::{TouchActivitySnapshot, TouchEvent, TouchPipelineInput, TouchStatus},
};

#[cfg(not(feature = "wifi-debug-slim-app"))]
pub(crate) const TOUCH_TRACE_ENABLED: bool = false;
pub(crate) const TOUCH_EVENT_TRACE_ENABLED: bool = false;
pub(crate) const GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED: bool = false;
// Keep touch polling at 8 ms so gesture starts are not missed between idle ticks.
// With the current controller behavior (frequent interleaved zero frames), 16 ms
// idle cadence can collapse fast swipe starts into one-frame taps.
pub(crate) const TOUCH_SAMPLE_ACTIVE_MS: u64 = 8;
pub(crate) const TOUCH_INIT_RETRY_MS: u64 = 2_000;
pub(crate) const TOUCH_IMU_QUIET_WINDOW_MS: u64 = 120;

#[cfg(not(feature = "wifi-debug-slim-app"))]
const TOUCH_TRACE_SAMPLES_CAP: usize = if TOUCH_TRACE_ENABLED { 8 } else { 1 };
const TOUCH_EVENT_TRACE_SAMPLES_CAP: usize = if TOUCH_EVENT_TRACE_ENABLED { 16 } else { 1 };

#[cfg(not(feature = "wifi-debug-slim-app"))]
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
pub(crate) static TOUCH_PIPELINE_INPUTS: Channel<CriticalSectionRawMutex, TouchPipelineInput, 32> =
    Channel::new();
pub(crate) static TOUCH_PIPELINE_EVENTS: Channel<CriticalSectionRawMutex, TouchEvent, 64> =
    Channel::new();
pub(crate) static TOUCH_LVGL_MULTITOUCH_FRAMES: Channel<
    CriticalSectionRawMutex,
    LvglMultitouchFrame,
    64,
> = Channel::new();
pub(crate) static TOUCH_LVGL_MULTITOUCH_RESET: AtomicBool = AtomicBool::new(false);
pub(crate) static TOUCH_CONTROLLER_ACTIVE_SLOTS: AtomicU8 = AtomicU8::new(0);
pub(crate) static TOUCH_IMU_ACTIVITY: Signal<CriticalSectionRawMutex, TouchActivitySnapshot> =
    Signal::new();
pub(crate) static TOUCH_IMU_STATUS: Signal<CriticalSectionRawMutex, TouchStatus> = Signal::new();
