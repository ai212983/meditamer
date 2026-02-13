pub mod config;
pub mod features;
pub mod registry;
pub mod tap_hsm;
pub mod trace;
pub mod types;

pub use tap_hsm::{EngineOutput, EventEngine};
pub use trace::EngineTraceSample;
pub use types::{
    ActionBuffer, CandidateScore, EngineAction, EngineStateId, EventDetected, EventKind,
    MotionFeatures, RejectReason, SensorFrame,
};
