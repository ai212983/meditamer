use crate::firmware::event_engine::types::{CandidateScore, EngineStateId, RejectReason};

#[derive(Clone, Copy, Debug, Default)]
pub struct EngineTraceSample {
    pub now_ms: u64,
    pub state_id: EngineStateId,
    pub reject_reason: RejectReason,
    pub seq_count: u8,
    pub tap_candidate: u8,
    pub candidate_source_mask: u8,
    pub candidate_score: CandidateScore,
    pub window_ms: u16,
    pub cooldown_active: u8,
    pub tap_src: u8,
    pub jerk_l1: i32,
    pub motion_veto: u8,
    pub gyro_l1: i32,
}
