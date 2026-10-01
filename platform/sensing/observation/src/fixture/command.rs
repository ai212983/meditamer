//! Shared strict command grammar; products resolve their own provider names.
use super::FixtureRequest;
use crate::{
    periodic::{MAX_INTERVAL_MS, MAX_VALIDITY_MS, MIN_INTERVAL_MS},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureMode {
    Once,
    Periodic { interval_ms: u32 },
    Cancel,
}
impl FixtureMode {
    pub const fn prefix(self) -> &'static str {
        if matches!(self, Self::Once) {
            "OBSFIX"
        } else {
            "OBSPER"
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedCommand<P> {
    pub provider: P,
    pub id: u64,
    pub validity_ms: u32,
    pub mode: FixtureMode,
}
impl<P> ParsedCommand<P> {
    pub fn at(&self, now: Instant) -> FixtureRequest {
        FixtureRequest {
            id: self.id,
            expires_at: now.saturating_add(Duration(self.validity_ms)),
        }
    }
}
pub fn parse_command<P>(
    line: &[u8],
    provider: impl FnOnce(&str) -> Option<P>,
    once_limit: u32,
) -> Option<ParsedCommand<P>> {
    let mut tokens = core::str::from_utf8(line).ok()?.split_ascii_whitespace();
    let prefix = tokens.next()?;
    if prefix != "OBSFIX" && prefix != "OBSPER" {
        return None;
    }
    let provider = provider(tokens.next()?)?;
    let id = decimal(tokens.next()?)?;
    if id == 0 {
        return None;
    }
    let next = tokens.next()?;
    let (mode, validity_ms) = if prefix == "OBSFIX" {
        (FixtureMode::Once, u32::try_from(decimal(next)?).ok()?)
    } else if next == "CANCEL" {
        (FixtureMode::Cancel, 0)
    } else {
        let interval_ms = u32::try_from(decimal(next)?).ok()?;
        if !(MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&interval_ms) {
            return None;
        }
        (
            FixtureMode::Periodic { interval_ms },
            u32::try_from(decimal(tokens.next()?)?).ok()?,
        )
    };
    let valid = match mode {
        FixtureMode::Once => (1..=once_limit).contains(&validity_ms),
        FixtureMode::Periodic { .. } => (1..=MAX_VALIDITY_MS).contains(&validity_ms),
        FixtureMode::Cancel => true,
    };
    if !valid || tokens.next().is_some() {
        return None;
    }
    Some(ParsedCommand {
        provider,
        id,
        validity_ms,
        mode,
    })
}
fn decimal(token: &str) -> Option<u64> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(line: &[u8]) -> Option<ParsedCommand<()>> {
        parse_command(line, |name| (name == "SENSOR").then_some(()), 300_000)
    }
    #[test]
    fn accepts_bounded_periodic_and_original_id_cancellation() {
        let command = parse(b"OBSPER SENSOR 18446744073709551615 300000 900000").unwrap();
        assert_eq!(command.id, u64::MAX);
        assert_eq!(
            command.mode,
            FixtureMode::Periodic {
                interval_ms: 300_000
            }
        );
        assert_eq!(command.at(Instant(7)).expires_at, Instant(900_007));
        assert_eq!(
            parse(b"OBSPER SENSOR 1 CANCEL").unwrap().mode,
            FixtureMode::Cancel
        );
        assert_eq!(
            parse(b"OBSFIX SENSOR 1 300000").unwrap().mode,
            FixtureMode::Once
        );
    }
    #[test]
    fn rejects_invalid_periodic_grammar_without_truncation() {
        for line in [
            "OBSPER SENSOR 0 60000 150000",
            "OBSPER SENSOR 1 59999 150000",
            "OBSPER SENSOR 1 300001 150000",
            "OBSPER SENSOR 1 60000 900001",
            "OBSPER SENSOR 1 60000 0",
            "OBSPER SENSOR 1 60000",
            "OBSPER SENSOR 1 60000 150000 extra",
            "OBSPER SENSOR 1 CANCEL extra",
            "OBSPER SENSOR 18446744073709551616 CANCEL",
            "OBSPER OTHER 1 CANCEL",
            "OBSPER SENSOR +1 CANCEL",
            "OBSFIX SENSOR 1 CANCEL",
        ] {
            assert!(parse(line.as_bytes()).is_none(), "accepted {line}");
        }
        assert!(parse(b"OBSPER SENSOR 1 CANCEL\xff").is_none());
    }
}
