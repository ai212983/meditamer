//! Product-level frame data and labels: what a render task needs from
//! [`super::model::HourglassModel`] each frame, without naming LVGL or any
//! hardware type. The device-neutral frame renderer consumes the pixel data;
//! a target widget consumes the formatted time and status.

use super::fixed::{Turns, Vec2};
use super::model::{HourglassModel, ParticlePositions, SessionState};
use super::tracers::TRACER_CAPACITY;

/// Everything the widget needs to draw one frame, captured at the render
/// task's own cadence (independently of the physics tick rate -- capturing
/// this is read-only and never advances the model).
#[derive(Clone, Copy)]
pub struct FrameData<'a> {
    pub angle: Turns,
    pub tracer_positions: &'a [Vec2; TRACER_CAPACITY],
    pub tracer_active_mask: u8,
    pub state: SessionState,
    /// Mass- and angle-derived ETA. `None` means the current orientation has
    /// no finite flow rate (paused/settling).
    pub remaining_seconds: Option<u32>,
    model: &'a HourglassModel,
}

impl<'a> FrameData<'a> {
    pub fn capture(model: &'a HourglassModel) -> FrameData<'a> {
        FrameData {
            angle: model.angle(),
            tracer_positions: model.tracer_positions(),
            tracer_active_mask: model.tracer_active_mask(),
            state: model.state(),
            remaining_seconds: model.remaining_seconds(),
            model,
        }
    }

    pub fn positions(&self) -> ParticlePositions<'a> {
        self.model.positions()
    }
}

/// `MM:SS` from a seconds count, or `--:--` for `None` (paused/settling, no
/// defined source chamber), NUL-terminated; returns the length including the
/// NUL. Same hand-rolled shape as `medinote::presentation`'s other
/// formatters -- no `core::fmt` machinery for two fixed-width fields.
pub fn format_remaining(buffer: &mut [u8; 8], remaining_seconds: Option<u32>) -> usize {
    let Some(total_seconds) = remaining_seconds else {
        buffer[..6].copy_from_slice(b"--:--\0");
        return 6;
    };
    let capped = total_seconds.min(99 * 60 + 59);
    let minutes = capped / 60;
    let seconds = capped % 60;
    buffer[0] = b'0' + (minutes / 10) as u8;
    buffer[1] = b'0' + (minutes % 10) as u8;
    buffer[2] = b':';
    buffer[3] = b'0' + (seconds / 10) as u8;
    buffer[4] = b'0' + (seconds % 10) as u8;
    buffer[5] = 0;
    6
}

/// A short, fixed status word for each [`SessionState`].
pub fn status_label(state: SessionState) -> &'static core::ffi::CStr {
    match state {
        SessionState::Ready => c"Ready",
        SessionState::Running => c"Running",
        SessionState::Paused => c"Paused",
        SessionState::Complete => c"Complete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "hourglass-pbd"))]
    use crate::fixed::Fx;
    #[cfg(not(feature = "hourglass-pbd"))]
    use crate::model::PHYSICS_HZ;
    #[cfg(not(feature = "hourglass-pbd"))]
    use crate::rotation::{RotationCommand, RotationCommandKind, TimestampUs};

    #[cfg(not(feature = "hourglass-pbd"))]
    fn hash_u32(mut hash: u64, value: u32) -> u64 {
        for byte in value.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    #[cfg(not(feature = "hourglass-pbd"))]
    fn frame_fingerprint(frame: &FrameData<'_>) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325;
        hash = hash_u32(hash, frame.angle.value().raw() as u32);
        hash = hash_u32(hash, frame.state as u32);
        hash = hash_u32(hash, frame.remaining_seconds.unwrap_or(u32::MAX));
        for position in frame.positions() {
            hash = hash_u32(hash, position.x.raw() as u32);
            hash = hash_u32(hash, position.y.raw() as u32);
        }
        for position in frame.tracer_positions {
            hash = hash_u32(hash, position.x.raw() as u32);
            hash = hash_u32(hash, position.y.raw() as u32);
        }
        hash_u32(hash, u32::from(frame.tracer_active_mask))
    }

    fn as_str(buffer: &[u8], len: usize) -> &str {
        core::str::from_utf8(&buffer[..len - 1]).unwrap()
    }

    #[test]
    fn format_remaining_renders_minutes_and_seconds() {
        let mut buf = [0u8; 8];
        let len = format_remaining(&mut buf, Some(125));
        assert_eq!(as_str(&buf, len), "02:05");
    }

    #[test]
    fn format_remaining_renders_placeholder_when_undefined() {
        let mut buf = [0u8; 8];
        let len = format_remaining(&mut buf, None);
        assert_eq!(as_str(&buf, len), "--:--");
    }

    #[test]
    fn format_remaining_caps_at_99_minutes_59_seconds() {
        let mut buf = [0u8; 8];
        let len = format_remaining(&mut buf, Some(999_999));
        assert_eq!(as_str(&buf, len), "99:59");
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels = [
            status_label(SessionState::Ready),
            status_label(SessionState::Running),
            status_label(SessionState::Paused),
            status_label(SessionState::Complete),
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn capture_reflects_the_model_it_was_taken_from() {
        let model = HourglassModel::new();
        let frame = FrameData::capture(&model);
        assert_eq!(frame.state, SessionState::Ready);
        assert_eq!(frame.angle.value(), super::super::fixed::Fx::ZERO);
        assert!(frame.remaining_seconds.is_some());
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn upright_progress_frame_matches_golden_fingerprint() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        for _ in 0..PHYSICS_HZ * 10 {
            model.tick();
        }
        assert_eq!(
            frame_fingerprint(&FrameData::capture(&model)),
            0x7435_0b94_f9a4_2a5d
        );
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn rotation_trace_frame_matches_golden_fingerprint() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        for _ in 0..45 {
            model.tick();
        }
        model
            .apply_command(RotationCommand {
                kind: RotationCommandKind::RotateBy(Fx::ratio(1, 4)),
                at: TimestampUs(1_500_000),
            })
            .unwrap();
        for _ in 0..60 {
            model.tick();
        }
        model
            .apply_command(RotationCommand {
                kind: RotationCommandKind::RotateBy(Fx::ratio(1, 4)),
                at: TimestampUs(3_500_000),
            })
            .unwrap();
        for _ in 0..90 {
            model.tick();
        }
        #[cfg(not(feature = "hourglass-block-gravity"))]
        let expected = 0x538c_7748_84a9_9e06;
        #[cfg(feature = "hourglass-block-gravity")]
        let expected = 0xf63a_0d5b_cfb2_3996;
        assert_eq!(frame_fingerprint(&FrameData::capture(&model)), expected);
    }
}
