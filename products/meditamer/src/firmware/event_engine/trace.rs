use super::types::{CandidateScore, EngineStateId, RejectReason};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EngineTraceSample {
    pub(crate) now_ms: u64,
    pub(crate) state_id: EngineStateId,
    pub(crate) reject_reason: RejectReason,
    pub(crate) seq_count: u8,
    pub(crate) tap_candidate: u8,
    pub(crate) candidate_source_mask: u8,
    pub(crate) candidate_score: CandidateScore,
    pub(crate) window_ms: u16,
    pub(crate) cooldown_active: u8,
    pub(crate) tap_src: u8,
    pub(crate) jerk_l1: i32,
    pub(crate) motion_veto: u8,
    pub(crate) gyro_l1: i32,
}
