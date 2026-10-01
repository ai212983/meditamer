//! Small buzzer owner for diagnostics without constructing a panel framebuffer.

use crate::{
    adapters::I2cOps,
    buzzer::{self, BuzzerRail, PitchWriteDelay},
    expander::PcalExpander,
    InkplateHalError,
};
use ::buzzer::{
    runner::Actuator,
    score::{ActionKind, RequestedAction},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

pub struct BuzzerDevice<I: 'static> {
    i2c: I,
    expander: &'static Mutex<CriticalSectionRawMutex, PcalExpander<I>>,
}

impl<I: I2cOps> BuzzerDevice<I> {
    pub fn new(i2c: I, expander: &'static Mutex<CriticalSectionRawMutex, PcalExpander<I>>) -> Self {
        Self { i2c, expander }
    }
}

struct Delay;
impl PitchWriteDelay for Delay {
    async fn wait_ms(&mut self, ms: u64) {
        embassy_time::Timer::after_millis(ms).await;
    }
}

impl<I: I2cOps> BuzzerRail for BuzzerDevice<I> {
    type Error = InkplateHalError<I::Error>;
    async fn set_rail_enabled(&mut self, enabled: bool) -> Result<(), Self::Error> {
        self.expander.lock().await.set_buzzer_power(enabled).await
    }
    async fn write_code(&mut self, code: u8) -> Result<(), Self::Error> {
        buzzer::write_pitch_code(&mut self.i2c, 0x2f, code, &mut Delay)
            .await
            .map_err(InkplateHalError::I2c)
    }
    async fn wait_ms(&mut self, ms: u64) {
        embassy_time::Timer::after_millis(ms).await;
    }
}

// The runner awaits shutdown after any apply failure, including an ambiguous
// rail-enable failure. Never abandon its future while a score is playing.
impl<I: I2cOps> Actuator for BuzzerDevice<I> {
    type Error = InkplateHalError<I::Error>;
    async fn apply(&mut self, action: RequestedAction) -> Result<(), Self::Error> {
        match action.kind {
            ActionKind::PowerOnSetCode { code } => {
                self.set_rail_enabled(true).await?;
                self.wait_ms(buzzer::STARTUP_WAIT_MS).await;
                self.write_code(code).await
            }
            ActionKind::SetCode { code } => self.write_code(code).await,
            ActionKind::PowerOff => self.set_rail_enabled(false).await,
        }
    }
    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        self.set_rail_enabled(false).await
    }
}
