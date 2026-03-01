use crate::firmware::app_state::{BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode};

use super::super::super::commands::StateSetOperation;
use super::super::util::trim_ascii_whitespace;

pub(in super::super) fn parse_state_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"STATE")
        || trimmed.eq_ignore_ascii_case(b"STATE GET")
        || trimmed.eq_ignore_ascii_case(b"STATE STATUS")
}

pub(in super::super) fn parse_diag_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"DIAG")
        || trimmed.eq_ignore_ascii_case(b"DIAG GET")
        || trimmed.eq_ignore_ascii_case(b"DIAG STATUS")
}

pub(in super::super) fn parse_state_set_command(line: &[u8]) -> Option<StateSetOperation> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"STATE SET";
    if !trimmed
        .get(..cmd.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(cmd))
    {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }
    let kv = &trimmed[i..];
    let mut split = kv.splitn(2, |byte| *byte == b'=');
    let key = split.next()?;
    let value = split.next()?;
    if key.eq_ignore_ascii_case(b"base") {
        if value.eq_ignore_ascii_case(b"DAY") {
            return Some(StateSetOperation::Base(BaseMode::Day));
        }
        if value.eq_ignore_ascii_case(b"TOUCH_WIZARD") {
            return Some(StateSetOperation::Base(BaseMode::TouchWizard));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"day_bg") {
        if value.eq_ignore_ascii_case(b"SUMINAGASHI") {
            return Some(StateSetOperation::DayBackground(DayBackground::Suminagashi));
        }
        if value.eq_ignore_ascii_case(b"SHANSHUI") {
            return Some(StateSetOperation::DayBackground(DayBackground::Shanshui));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"overlay") {
        if value.eq_ignore_ascii_case(b"NONE") {
            return Some(StateSetOperation::Overlay(OverlayMode::None));
        }
        if value.eq_ignore_ascii_case(b"CLOCK") {
            return Some(StateSetOperation::Overlay(OverlayMode::Clock));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"upload") {
        if value.eq_ignore_ascii_case(b"ON") {
            return Some(StateSetOperation::Upload(true));
        }
        if value.eq_ignore_ascii_case(b"OFF") {
            return Some(StateSetOperation::Upload(false));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"assets") {
        if value.eq_ignore_ascii_case(b"ON") {
            return Some(StateSetOperation::AssetReads(true));
        }
        if value.eq_ignore_ascii_case(b"OFF") {
            return Some(StateSetOperation::AssetReads(false));
        }
        return None;
    }
    None
}

pub(in super::super) fn parse_state_diag_command(line: &[u8]) -> Option<(DiagKind, DiagTargets)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"STATE DIAG";
    if !trimmed
        .get(..cmd.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(cmd))
    {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }
    let args = &trimmed[i..];
    let mut kind = None;
    let mut targets = DiagTargets::none();
    for token in args.split(|byte| *byte == b' ') {
        if token.is_empty() {
            continue;
        }
        let mut split = token.splitn(2, |byte| *byte == b'=');
        let key = split.next()?;
        let value = split.next()?;
        if key.eq_ignore_ascii_case(b"kind") {
            if value.eq_ignore_ascii_case(b"NONE") {
                kind = Some(DiagKind::None);
            } else if value.eq_ignore_ascii_case(b"DEBUG") {
                kind = Some(DiagKind::Debug);
            } else if value.eq_ignore_ascii_case(b"TEST") {
                kind = Some(DiagKind::Test);
            } else {
                return None;
            }
        } else if key.eq_ignore_ascii_case(b"targets") {
            let mut bits = 0u8;
            for target in value.split(|byte| *byte == b'|') {
                if target.eq_ignore_ascii_case(b"NONE") {
                    bits = 0;
                    continue;
                }
                if target.eq_ignore_ascii_case(b"SD") {
                    bits |= 1 << 0;
                } else if target.eq_ignore_ascii_case(b"WIFI") {
                    bits |= 1 << 1;
                } else if target.eq_ignore_ascii_case(b"DISPLAY") {
                    bits |= 1 << 2;
                } else if target.eq_ignore_ascii_case(b"TOUCH") {
                    bits |= 1 << 3;
                } else if target.eq_ignore_ascii_case(b"IMU") {
                    bits |= 1 << 4;
                } else if target.is_empty() {
                    continue;
                } else {
                    return None;
                }
            }
            targets = DiagTargets::from_persisted(bits);
        }
    }
    Some((kind?, targets))
}
