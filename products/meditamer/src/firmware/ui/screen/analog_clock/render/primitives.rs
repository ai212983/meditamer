//! Decoded clock maps and compact streaming output primitives.

use super::*;
use analog_clock::{DialMap, DitherAlgorithm, Hands, RegionDithers, Surface};

/// Borrowed map view valid only for the current render step.
pub(super) struct DecodedMaps<'a> {
    pub(super) hands: Hands<'a>,
    pub(super) scene: ClockScene<'a>,
}

pub(super) fn decode_shared(bytes: &AssetBuffers) -> Option<DecodedMaps<'_>> {
    let assets = bytes.borrowed()?;
    let dial: DialMap<'_> = assets.dial();
    if !dial.validate() {
        return None;
    }
    let hands = assets.hands();
    let mut scene = ClockScene::for_size(CLOCK_WIDTH, CLOCK_HEIGHT);
    scene.dial = Some(dial);
    // Fixed studio setup for the Durer dial: default light/source pivots,
    // 8 height-ordered area samples, minute hand above hour.
    scene.hand_darkness = 0.85;
    Some(DecodedMaps { hands, scene })
}

pub(super) fn base_profiles() -> RegionDithers {
    RegionDithers {
        background: DitherAlgorithm::Atkinson,
        clock: DitherAlgorithm::Atkinson,
        hands: DitherAlgorithm::Atkinson,
        shadows: DitherAlgorithm::Atkinson,
    }
}

pub(super) fn frame_profiles() -> RegionDithers {
    RegionDithers {
        background: DitherAlgorithm::Atkinson,
        clock: DitherAlgorithm::Bayer8,
        hands: DitherAlgorithm::FloydSteinberg,
        shadows: DitherAlgorithm::FloydSteinberg,
    }
}

/// Row-major MSB-first bit writer over a caller buffer, used as the
/// [`Surface`] for one streaming pass. Sized for the full frame because the
/// core validates surface geometry per row.
pub(super) struct BitWriter<'a> {
    pub(super) bits: &'a mut [u8],
}

impl Surface for BitWriter<'_> {
    fn width(&self) -> i32 {
        CLOCK_WIDTH as i32
    }
    fn height(&self) -> i32 {
        CLOCK_HEIGHT as i32
    }
    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x < 0 || y < 0 || x >= CLOCK_WIDTH as i32 || y >= CLOCK_HEIGHT as i32 {
            return;
        }
        let bit = (y as usize) * (CLOCK_WIDTH as usize) + (x as usize);
        let byte = &mut self.bits[bit / 8];
        let mask = 0x80 >> (bit % 8);
        if ink {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

pub(super) fn packed_bit(bits: &[u8], x: u32, y: u32) -> bool {
    let bit = (y as usize) * (CLOCK_WIDTH as usize) + (x as usize);
    bits.get(bit / 8)
        .is_some_and(|byte| byte & (0x80 >> (bit % 8)) != 0)
}
