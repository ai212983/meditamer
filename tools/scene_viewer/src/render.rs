//! Bundle rendering.
//!
//! [`flow`] drives a render end to end; [`quantize`] maps tone to the device's
//! grey levels; [`stylize`] adds the relight, ink, and paper texture passes. The
//! channel ids and 8-bit blend primitives all three share live here.

mod flow;
mod quantize;
mod stylize;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::cli::parse_render_args;

const CH_ALBEDO: u8 = 1;
const CH_LIGHT: u8 = 2;
const CH_AO: u8 = 3;
const CH_DEPTH: u8 = 4;
const CH_EDGE: u8 = 5;
const CH_MASK: u8 = 6;
const CH_STROKE: u8 = 7;
const CH_NORMAL_X: u8 = 8;
const CH_NORMAL_Y: u8 = 9;

pub(crate) fn run_render<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let cfg = parse_render_args(args)?;
    flow::run_render_with_config(cfg)
}

fn get_channel_or_default<'a>(
    channels: &'a HashMap<u8, Vec<u8>>,
    id: u8,
    len: usize,
    default_value: u8,
) -> Result<&'a [u8], String> {
    if let Some(ch) = channels.get(&id) {
        if ch.len() != len {
            return Err(format!(
                "channel id={id} length mismatch expected={} got={}",
                len,
                ch.len()
            ));
        }
        Ok(ch)
    } else {
        // Return a leaked backing buffer to keep interface simple and no allocations in the hot loop.
        let boxed = vec![default_value; len].into_boxed_slice();
        Ok(Box::leak(boxed))
    }
}

fn mul8(a: u8, b: u8) -> u8 {
    (((a as u16 * b as u16) + 128) >> 8) as u8
}

fn mix_u8(a: u8, b: u8, t: u8) -> u8 {
    ((((a as u16) * (255 - t) as u16) + ((b as u16) * t as u16) + 128) >> 8) as u8
}

fn clamp_i16_to_u8(v: i16) -> u8 {
    v.clamp(0, 255) as u8
}
