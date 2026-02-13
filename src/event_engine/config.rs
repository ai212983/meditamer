#[derive(Clone, Copy, Debug)]
pub struct TapThresholdConfig {
    pub jerk_l1_min: i32,
    pub jerk_strong_l1_min: i32,
    pub jerk_seq_cont_min: i32,
    pub prev_jerk_quiet_max: i32,
    pub gyro_l1_swing_max: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct TapWeightConfig {
    pub axis_weight: u16,
    pub single_tap_weight: u16,
    pub int1_weight: u16,
    pub tap_event_weight: u16,
    pub jerk_axis_weight: u16,
    pub jerk_only_weight: u16,
    pub seq_finish_weight: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct TripleTapConfig {
    pub enabled: bool,
    pub min_gap_ms: u64,
    pub max_gap_ms: u64,
    pub last_max_gap_ms: u64,
    pub cooldown_ms: u64,
    pub debounce_ms: u64,
    pub seq_finish_debounce_ms: u64,
    pub gyro_veto_hold_ms: u64,
    pub thresholds: TapThresholdConfig,
    pub weights: TapWeightConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct OptionalEventConfig {
    pub pickup_enabled: bool,
    pub placement_enabled: bool,
    pub stillness_start_enabled: bool,
    pub stillness_end_enabled: bool,
    pub near_intent_enabled: bool,
    pub far_intent_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct EventEngineConfig {
    pub triple_tap: TripleTapConfig,
    pub optional_events: OptionalEventConfig,
}

include!(concat!(env!("OUT_DIR"), "/event_config.rs"));

pub fn active_config() -> &'static EventEngineConfig {
    &EVENT_ENGINE_CONFIG
}
