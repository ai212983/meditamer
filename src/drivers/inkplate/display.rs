mod async_impl;

use super::{LUTB, LUTW};

fn partial_waveform_byte(previous: u8, current: u8, upper_nibble: bool) -> u8 {
    let black_to_white = previous & !current;
    let white_to_black = !previous & current;
    let shift = if upper_nibble { 4 } else { 0 };
    LUTW[((black_to_white >> shift) & 0x0F) as usize]
        & LUTB[((white_to_black >> shift) & 0x0F) as usize]
}
