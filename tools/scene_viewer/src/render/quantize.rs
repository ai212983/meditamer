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

fn quantize_u8(v: u8, x: i32, y: i32, mode: OutputMode, dither: DitherMode) -> u8 {
    match mode {
        OutputMode::Gray8 => v,
        OutputMode::Mono1 => {
            let threshold = match dither {
                DitherMode::None => 128,
                DitherMode::Bayer4 => bayer4_threshold_u8(x, y),
            };
            if v <= threshold {
                0
            } else {
                255
            }
        }
        OutputMode::Gray3 => {
            let adjusted = dither_adjust(v, x, y, dither, 4);
            quantize_levels(adjusted, 8)
        }
        OutputMode::Gray4 => {
            let adjusted = dither_adjust(v, x, y, dither, 2);
            quantize_levels(adjusted, 16)
        }
    }
}

fn quantize_levels(v: u8, levels: u16) -> u8 {
    if levels <= 1 {
        return v;
    }

    let max = levels - 1;
    let level = ((v as u32 * max as u32 + 127) / 255) as u16;
    ((level as u32 * 255 + (max as u32 / 2)) / max as u32) as u8
}

fn dither_adjust(v: u8, x: i32, y: i32, dither: DitherMode, strength: i16) -> u8 {
    let delta = match dither {
        DitherMode::None => 0,
        DitherMode::Bayer4 => bayer4_value(x, y) as i16 - 8,
    };
    clamp_i16_to_u8(v as i16 + delta * strength)
}

fn bayer4_threshold_u8(x: i32, y: i32) -> u8 {
    (bayer4_value(x, y) << 4) + 8
}

fn bayer4_value(x: i32, y: i32) -> u8 {
    const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

    let xx = x.rem_euclid(4) as usize;
    let yy = y.rem_euclid(4) as usize;
    BAYER4[yy][xx]
}

fn build_tone_lut(curve: ToneCurve) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let x = (i as f32) / 255.0;
        let y = match curve {
            ToneCurve::Linear => x,
            // lift paper whites while preserving dark ink pooling
            ToneCurve::Wash => {
                let lifted = x.powf(0.82);
                (0.82 * lifted) + (0.18 * x * x)
            }
            // stronger contrast for edge-first compositions
            ToneCurve::Filmic => {
                let y = (x * (x * 2.51 + 0.03)) / (x * (x * 2.43 + 0.59) + 0.14);
                y.clamp(0.0, 1.0)
            }
            // keep highlights paper-white and compress mids for an ink-wash look
            ToneCurve::SumiE => {
                let ink = x.powf(0.72);
                let dry = x * x * x;
                let y = (ink * 0.62) + (dry * 0.38);
                y.clamp(0.0, 1.0)
            }
        };
        *entry = ((y.clamp(0.0, 1.0) * 255.0) + 0.5) as u8;
    }
    lut
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
