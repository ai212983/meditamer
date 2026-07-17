pub(crate) mod config;
mod face_down;
mod mailbox;
pub(crate) mod metrics;
pub(crate) mod scheduler;
mod tasks;
mod types;

pub(crate) use mailbox::{discard_pending_actions, take_pending_actions};
pub(crate) use tasks::{imu_acquisition_task, imu_pipeline_task};
pub(crate) use types::ImuTraceContext;

pub(crate) fn publish_trace_context(context: ImuTraceContext) {
    config::IMU_TRACE_CONTEXT.signal(context);
}
