use crate::firmware::event_engine::SensorFrame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImuFaultStage {
    Initialization,
    Sampling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImuSuppressionReason {
    Touch,
    Upload,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ImuPipelineInput {
    Recovered {
        now_ms: u64,
    },
    Sample {
        frame: SensorFrame,
        int2: bool,
        power_good: i16,
        discontinuity: bool,
    },
    Fault {
        now_ms: u64,
        stage: ImuFaultStage,
    },
    Suppressed {
        now_ms: u64,
        reason: ImuSuppressionReason,
    },
    Resumed {
        now_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ImuSamplingDemand {
    pub(crate) active_until_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ImuTraceContext {
    pub(crate) battery_percent: i16,
}

impl Default for ImuTraceContext {
    fn default() -> Self {
        Self {
            battery_percent: -1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImuActionSnapshot {
    pub(crate) backlight_trigger: bool,
    pub(crate) day_background_toggle_count: u32,
}
