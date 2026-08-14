use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

use super::types::{ImuPipelineInput, ImuSamplingDemand, ImuTraceContext};

pub(crate) const IMU_INIT_RETRY_MS: u64 = 2_000;
pub(crate) const TAP_TRACE_ENABLED: bool = true;
pub(crate) const TAP_TRACE_SAMPLE_MS: u64 = 25;
pub(crate) const TAP_TRACE_AUX_SAMPLE_MS: u64 = 250;

pub(crate) static IMU_PIPELINE_INPUTS: Channel<CriticalSectionRawMutex, ImuPipelineInput, 8> =
    Channel::new();
pub(crate) static IMU_SAMPLING_DEMAND: Signal<CriticalSectionRawMutex, ImuSamplingDemand> =
    Signal::new();
pub(crate) static IMU_TRACE_CONTEXT: Signal<CriticalSectionRawMutex, ImuTraceContext> =
    Signal::new();
