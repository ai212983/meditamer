#[derive(Clone, Copy, Debug)]
pub(crate) struct TapThresholdConfig {
    pub(crate) jerk_l1_min: i32,
    pub(crate) jerk_strong_l1_min: i32,
    pub(crate) jerk_seq_cont_min: i32,
    pub(crate) prev_jerk_quiet_max: i32,
    pub(crate) gyro_l1_swing_max: i32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TapWeightConfig {
    pub(crate) axis_weight: u16,
    pub(crate) single_tap_weight: u16,
    pub(crate) int1_weight: u16,
    pub(crate) tap_event_weight: u16,
    pub(crate) jerk_axis_weight: u16,
    pub(crate) jerk_only_weight: u16,
    pub(crate) seq_finish_weight: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TripleTapConfig {
    pub(crate) enabled: bool,
    pub(crate) min_gap_ms: u64,
    pub(crate) max_gap_ms: u64,
    pub(crate) last_max_gap_ms: u64,
    pub(crate) cooldown_ms: u64,
    pub(crate) debounce_ms: u64,
    pub(crate) seq_finish_debounce_ms: u64,
    pub(crate) gyro_veto_hold_ms: u64,
    pub(crate) thresholds: TapThresholdConfig,
    pub(crate) weights: TapWeightConfig,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OptionalEventConfig {
    pub(crate) pickup_enabled: bool,
    pub(crate) placement_enabled: bool,
    pub(crate) stillness_start_enabled: bool,
    pub(crate) stillness_end_enabled: bool,
    pub(crate) near_intent_enabled: bool,
    pub(crate) far_intent_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EventEngineConfig {
    pub(crate) imu_sampling: ImuSamplingConfig,
    pub(crate) triple_tap: TripleTapConfig,
    pub(crate) optional_events: OptionalEventConfig,
}

include!(concat!(env!("OUT_DIR"), "/event_config.rs"));

pub(crate) fn active_config() -> &'static EventEngineConfig {
    &EVENT_ENGINE_CONFIG
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImuSamplingConfig {
    pub(crate) sensor_odr_hz: u16,
    pub(crate) idle_hz: u16,
    pub(crate) active_hz: u16,
    pub(crate) active_hold_ms: u64,
}
