pub(crate) mod config;
mod mailbox;
pub(crate) mod metrics;
pub(crate) mod scheduler;
pub mod tasks;
pub(crate) mod timing;
mod types;

pub(crate) use mailbox::take_pending_actions;
pub use tasks::{
    imu_acquisition_task, imu_pipeline_task, resume_imu_acquisition, suspend_imu_acquisition,
    try_request_imu_acquisition_resume,
};
pub(crate) use types::ImuTraceContext;

pub(crate) fn publish_trace_context(context: ImuTraceContext) {
    config::IMU_TRACE_CONTEXT.signal(context);
}

/// A finite diagnostic demand uses the ordinary adaptive scheduler and bus path.
/// It changes neither the configured rate nor touch/panel suppression policy.
pub(crate) fn request_qualification_window() {
    config::IMU_SAMPLING_DEMAND.signal(types::ImuSamplingDemand {
        active_until_ms: embassy_time::Instant::now()
            .as_millis()
            .saturating_add(30_000),
    });
}
