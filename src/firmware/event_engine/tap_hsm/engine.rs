use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct EngineOutput {
    pub actions: ActionBuffer,
    pub trace: EngineTraceSample,
}

pub struct EventEngine {
    machine: statig::blocking::StateMachine<TapHsm>,
}

impl Default for EventEngine {
    fn default() -> Self {
        Self::new(active_config())
    }
}

impl EventEngine {
    pub fn new(config: &'static EventEngineConfig) -> Self {
        Self {
            machine: TapHsm::new(config).state_machine(),
        }
    }

    pub fn tick(&mut self, frame: SensorFrame) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::Tick(frame), &mut context);
        self.finish(context)
    }

    pub fn imu_fault(&mut self, now_ms: u64) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::ImuFault { now_ms }, &mut context);
        self.finish(context)
    }

    pub fn imu_recovered(&mut self, now_ms: u64) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::ImuRecovered { now_ms }, &mut context);
        self.finish(context)
    }

    /// Signals that samples were missed while the sensor stayed healthy. Clears
    /// motion history without disturbing the state machine, so a tap sequence
    /// survives scheduler jitter and touch-bus suppression.
    pub fn sampling_gap(&mut self) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::SamplingGap, &mut context);
        self.finish(context)
    }

    fn finish(&self, context: DispatchContext) -> EngineOutput {
        EngineOutput {
            actions: context.actions,
            trace: self.machine.inner().last_trace,
        }
    }
}
