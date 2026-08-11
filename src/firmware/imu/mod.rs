pub(crate) mod config;
mod mailbox;
pub(crate) mod metrics;
pub(crate) mod scheduler;
mod tasks;
mod types;

pub(crate) use mailbox::{discard_pending_actions, take_pending_actions};
pub(crate) use tasks::{
    imu_acquisition_task, imu_pipeline_task, resume_imu_acquisition, suspend_imu_acquisition,
    try_request_imu_acquisition_resume,
};
pub(crate) use types::ImuTraceContext;

pub(crate) fn publish_trace_context(context: ImuTraceContext) {
    config::IMU_TRACE_CONTEXT.signal(context);
}
