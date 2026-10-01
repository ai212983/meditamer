mod acquisition;
mod acquisition_control;

pub use acquisition::environment_acquisition_task;
pub use acquisition_control::{
    resume_environment_acquisition, suspend_environment_acquisition,
    try_request_environment_acquisition_resume,
};

pub(crate) use acquisition_control::control_snapshot;
