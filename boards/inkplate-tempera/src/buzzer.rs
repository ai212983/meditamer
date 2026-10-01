//! Inkplate 4 TEMPERA buzzer driver: nominal pitch mapping, retried pitch
//! programming, and note sequencing with explicit shutdown.
//!
//! Hardware (board revision V1.2.0): U16 is an MCP4017T 10 kOhm rheostat at
//! I2C address `0x2F` driving a TLC555 astable (R59 = 2.2 kOhm, C62 =
//! 100 nF) through a MOSFET push-pull stage into the MLT-7525 transducer.
//! Q13 switches the shared `3V3_BUZZER` rail via active-low `BUZZ_EN`: the
//! rail powers the rheostat, the timer, and the output stage together, so
//! rail-off is not an audio mute and pitch writes require a powered
//! rheostat. Power-on loads default code `0x3F`; a rest followed by a note
//! must restore the requested code after power-up.
//!
//! The nominal oscillator model and its integer math live in the shared
//! [`buzzer`] platform crate so firmware, host tests, and the WAV previewer
//! agree bit-for-bit. This module instantiates that model with the
//! board-specific values (R59 = 2200 Ohm, assumed Rw = 100 Ohm, RAB =
//! 10 kOhm, C62 = 100 nF) and owns the transport and sequencing policy:
//!
//! - [`code_for_frequency`] maps requests to the nearest attainable code in
//!   cents (ties prefer the lower code), saturating out-of-range requests to
//!   the endpoint codes. Zero and negative requests are rejected before any
//!   conversion or division.
//! - [`write_pitch_code`] retries pitch writes and propagates an exhausted
//!   transport failure. Recovery touches only the buzzer path (bus reset
//!   plus a short wait): it never enables the frontlight or any other
//!   subsystem as a side effect.
//! - [`start_note`] powers the rail before programming (never the reverse),
//!   [`retune`] changes pitch while powered without touching the rail, and
//!   every transport failure attempts an explicit shutdown and reports both the
//!   original failure and any shutdown failure.
//!
//! Nominal endpoints are approximately 586.4 and 3136.1 Hz, but RAB
//! tolerance alone is +/-20% before capacitor, resistor, timer, and wiper
//! variation: these are approximations, not calibrated pitch guarantees.

use super::adapters::I2cOps;

/// Master switch for rheostat pitch programming. When disabled, pitch writes
/// are skipped while the rail still powers on: the timer then sounds its
/// power-on default code until bench evidence says otherwise.
pub const ENABLE_BUZZER_PITCH_CONTROL: bool = true;

/// Fixed series resistor R59 in ohms.
pub const R59_OHM: u32 = 2200;

/// Assumed wiper resistance Rw in ohms. Voltage- and code-dependent in
/// reality; this nominal value anchors the model, not a calibration.
pub const WIPER_RW_OHM: u32 = 100;

/// Rheostat full-scale resistance RAB in ohms (+/-20% part tolerance).
pub const RAB_OHM: u32 = 10_000;

/// Nominal timing network instantiated with the board values.
pub const OSCILLATOR: buzzer::model::OscillatorParams = buzzer::model::OscillatorParams {
    r_base_ohm: R59_OHM + WIPER_RW_OHM,
    r_pot_ohm: RAB_OHM,
    code_max: buzzer::model::CODE_MAX as u32,
};

/// Nominal frequency of code 0 in Hz (truncated): the highest pitch.
pub const NOMINAL_FREQ_MAX_HZ: i32 = buzzer::model::freq_hz(&OSCILLATOR, 0) as i32;

/// Nominal frequency of code 127 in Hz (truncated): the lowest pitch.
pub const NOMINAL_FREQ_MIN_HZ: i32 = buzzer::model::freq_hz(&OSCILLATOR, 127) as i32;

/// Rheostat power-on default code: sounds until the first pitch write lands.
pub const DEFAULT_CODE: u8 = 0x3F;

/// Pitch-write attempts before the transport failure is propagated.
pub const PITCH_WRITE_ATTEMPTS: u32 = 4;

/// Wait between pitch-write attempts in milliseconds.
pub const PITCH_RETRY_WAIT_MS: u64 = 2;

/// Rail settle wait before programming a new note in milliseconds. Bench
/// evidence may reduce this; never assume instant reset.
pub const STARTUP_WAIT_MS: u64 = 1;

/// A frequency request with no nearest pitch: zero or negative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuzzerFreqError {
    NonPositive(i32),
}

/// Nearest attainable rheostat code for `freq_hz`.
///
/// Out-of-range requests saturate to the nearest endpoint code; zero and
/// negative requests are rejected before any conversion or division.
pub fn code_for_frequency(freq_hz: i32) -> Result<u8, BuzzerFreqError> {
    if freq_hz <= 0 {
        return Err(BuzzerFreqError::NonPositive(freq_hz));
    }
    Ok(buzzer::model::nearest_code(&OSCILLATOR, freq_hz as u32))
}

/// Nominal frequency in Hz for a raw rheostat code, or `None` when `code`
/// exceeds [`buzzer::model::CODE_MAX`].
pub fn nominal_freq_hz(code: u8) -> Option<u32> {
    if code > buzzer::model::CODE_MAX {
        return None;
    }
    Some(buzzer::model::freq_hz(&OSCILLATOR, code))
}

/// Millisecond wait between pitch-write attempts. Implemented with
/// `embassy_time` on device and scripted in host tests.
#[allow(async_fn_in_trait)]
pub trait PitchWriteDelay {
    async fn wait_ms(&mut self, ms: u64);
}

/// Programs one rheostat code with bounded retries.
///
/// One wire write per attempt; between attempts the bus resets and the
/// caller-provided delay runs. An exhausted failure is returned to the
/// caller -- success is never reported without a completed write, and no
/// unrelated subsystem (notably the frontlight) is touched on any path.
pub async fn write_pitch_code<I: I2cOps>(
    i2c: &mut I,
    addr: u8,
    code: u8,
    delay: &mut impl PitchWriteDelay,
) -> Result<(), I::Error> {
    let payload = [code & 0x7F];
    for attempt in 0..PITCH_WRITE_ATTEMPTS.max(1) {
        match i2c.write(addr, &payload).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if attempt + 1 >= PITCH_WRITE_ATTEMPTS.max(1) {
                    return Err(error);
                }
                let _ = i2c.reset().await;
                delay.wait_ms(PITCH_RETRY_WAIT_MS).await;
            }
        }
    }
    // Unreachable: the loop returns on every path. Retained so a zero
    // `PITCH_WRITE_ATTEMPTS` configuration cannot report success silently.
    debug_assert!(false, "pitch-write loop must return");
    Ok(())
}

/// Note-sequencing failures. Every variant names the operation that failed;
/// transport failures that also fail shutdown carry both errors so neither
/// is masked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuzzerError<E> {
    InvalidFrequency(i32),
    RailEnable(E),
    RailEnableThenShutdown { enable: E, shutdown: E },
    PitchWrite(E),
    PitchWriteThenShutdown { pitch: E, shutdown: E },
    RailShutdown(E),
}

/// Rail and pitch sink for one note. Exactly one owner drives the rail per
/// playback; the implementation below runs on the shared expander cache, and
/// no second private register cache may be introduced beside it.
#[allow(async_fn_in_trait)]
pub trait BuzzerRail {
    type Error;

    /// Switches the shared `3V3_BUZZER` rail (`true` powers the rheostat,
    /// timer, and output stage together).
    async fn set_rail_enabled(&mut self, enabled: bool) -> Result<(), Self::Error>;

    /// Programs one rheostat code; requires a powered rail.
    async fn write_code(&mut self, code: u8) -> Result<(), Self::Error>;

    /// Millisecond wait (rail settle between power-on and programming).
    async fn wait_ms(&mut self, ms: u64);
}

/// Starts a new note: validates the request before touching hardware, powers
/// the rail, waits the startup interval, then programs the code.
///
/// Every transport failure attempts an explicit shutdown: a clean shutdown
/// reports the original failure, a failed shutdown reports both. Returns
/// the selected code on success.
pub async fn start_note<R: BuzzerRail>(
    rail: &mut R,
    freq_hz: i32,
) -> Result<u8, BuzzerError<R::Error>> {
    let code = code_for_frequency(freq_hz).map_err(|_| BuzzerError::InvalidFrequency(freq_hz))?;
    if let Err(enable) = rail.set_rail_enabled(true).await {
        match rail.set_rail_enabled(false).await {
            Ok(()) => return Err(BuzzerError::RailEnable(enable)),
            Err(shutdown) => {
                return Err(BuzzerError::RailEnableThenShutdown { enable, shutdown });
            }
        }
    }
    rail.wait_ms(STARTUP_WAIT_MS).await;
    match rail.write_code(code).await {
        Ok(()) => Ok(code),
        Err(pitch) => match rail.set_rail_enabled(false).await {
            Ok(()) => Err(BuzzerError::PitchWrite(pitch)),
            Err(shutdown) => Err(BuzzerError::PitchWriteThenShutdown { pitch, shutdown }),
        },
    }
}

/// Changes pitch while powered. A pitch failure attempts an explicit
/// shutdown so a failed retune never leaves the rail on: a clean shutdown
/// reports the pitch failure, a failed shutdown reports both. For a new
/// note (power-up plus default-code restore) use [`start_note`].
pub async fn retune<R: BuzzerRail>(
    rail: &mut R,
    freq_hz: i32,
) -> Result<u8, BuzzerError<R::Error>> {
    let code = code_for_frequency(freq_hz).map_err(|_| BuzzerError::InvalidFrequency(freq_hz))?;
    match rail.write_code(code).await {
        Ok(()) => Ok(code),
        Err(pitch) => match rail.set_rail_enabled(false).await {
            Ok(()) => Err(BuzzerError::PitchWrite(pitch)),
            Err(shutdown) => Err(BuzzerError::PitchWriteThenShutdown { pitch, shutdown }),
        },
    }
}

/// Powers the rail off. A failure is reported, never swallowed: an enabled
/// buzzer must not be abandoned silently (in particular not across sleep).
pub async fn end_note<R: BuzzerRail>(rail: &mut R) -> Result<(), BuzzerError<R::Error>> {
    rail.set_rail_enabled(false)
        .await
        .map_err(BuzzerError::RailShutdown)?;
    Ok(())
}

#[cfg(test)]
mod tests;
