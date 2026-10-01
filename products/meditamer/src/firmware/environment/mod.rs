//! The BME688 provider (ADR-0018, typed observation subscriptions plan,
//! Phase 4): an independent acquisition task plus the display adapter that
//! reads its published state. Mirrors `crate::firmware::imu`'s module shape
//! (`config`/`tasks`/`types`) since it joins the same `panel_bus`
//! suspension protocol that module established.

mod config;
mod driver;
pub(crate) mod requests;
mod tasks;
mod types;

pub(crate) use config::{
    ENVIRONMENT_DEMAND, ENVIRONMENT_REQUESTS, ENVIRONMENT_SAMPLE_INTERVAL_S, ENVIRONMENT_STATE,
    SUBSCRIPTION_CAPACITY,
};
pub(crate) use requests::RequestAdmissionError;
pub use tasks::{
    environment_acquisition_task, resume_environment_acquisition, suspend_environment_acquisition,
    try_request_environment_acquisition_resume,
};
pub(crate) use types::{
    ambient_home_subscription, EnvironmentFields, EnvironmentSnapshot, EnvironmentStateSnapshot,
    ENVIRONMENT_PROVIDER_ID, FIELDS,
};

pub(crate) use tasks::control_snapshot;
