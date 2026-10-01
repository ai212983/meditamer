//! Bounded UI-owner sleep requests; no retained state crosses deep sleep.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepMode {
    Deep,
    RejectEnvironment,
    RejectBattery,
}

impl SleepMode {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Deep => "DEEP",
            Self::RejectEnvironment => "REJECT_ENV",
            Self::RejectBattery => "REJECT_ADC",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepRequest {
    pub id: u64,
    pub validity_ms: u32,
    pub mode: SleepMode,
}

pub fn parse(line: &[u8]) -> Option<SleepRequest> {
    let mut words = core::str::from_utf8(line).ok()?.split_ascii_whitespace();
    if words.next()? != "OBSSLEEP" {
        return None;
    }
    let id = words.next()?.parse().ok()?;
    let validity_ms = words.next()?.parse().ok()?;
    let mode = match words.next()? {
        "DEEP" => SleepMode::Deep,
        "REJECT_ENV" => SleepMode::RejectEnvironment,
        "REJECT_ADC" => SleepMode::RejectBattery,
        _ => return None,
    };
    if id == 0
        || !(1..=super::fixture::MAX_FIXTURE_VALIDITY_MS).contains(&validity_ms)
        || words.next().is_some()
    {
        return None;
    }
    Some(SleepRequest {
        id,
        validity_ms,
        mode,
    })
}
