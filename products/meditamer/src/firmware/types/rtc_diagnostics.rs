//! Inkplate RTC failure diagnostics.
//!
//! [`InkplateRtcDiagnostics`] is the [`rtc::driver::RtcDiagnosticSink`] the
//! serial task's RTC driver reports through. Failure-only metadata: the
//! driver calls it solely with the failed operation, register, address, and
//! the original error -- never payload bytes or time values.

use rtc::driver::{RtcDiagnosticSink, RtcStage};

/// Failure-only RTC diagnostic sink for the Inkplate serial task.
#[derive(Clone, Copy, Debug, Default)]
pub struct InkplateRtcDiagnostics;

impl RtcDiagnosticSink for InkplateRtcDiagnostics {
    fn on_i2c_error<E>(&mut self, stage: RtcStage, register: u8, address: u8, error: &E)
    where
        E: core::fmt::Debug,
    {
        console::println!(
            "RTC_DIAG op={} reg=0x{:02x} addr=0x{:02x} err={:?}",
            stage.as_str(),
            register,
            address,
            error
        );
    }
}
