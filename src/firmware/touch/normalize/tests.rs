use super::*;

fn sample(
    ms: u64,
    touch_count: u8,
    p0: (u16, u16),
    p1: (u16, u16),
    raw_bit7: bool,
) -> (u64, NormalizedTouchSample) {
    let mut raw = [0u8; 8];
    if raw_bit7 {
        raw[7] = 0x01;
    }
    (
        ms,
        NormalizedTouchSample {
            touch_count,
            points: [
                NormalizedTouchPoint { x: p0.0, y: p0.1 },
                NormalizedTouchPoint { x: p1.0, y: p1.1 },
            ],
            raw,
        },
    )
}

mod part1;
mod part2;
mod part3;
