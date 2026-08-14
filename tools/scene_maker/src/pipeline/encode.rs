use super::{ChannelData, EncodedPayload, SceneDims};
use crate::{
    cli::{BuildConfig, Compression},
    format::{
        decode_len_hint, encode_strip, payload_start, raw_len_from_strip, ChannelDescriptor,
        StripEntry,
    },
};

pub(super) fn encode_channels(
    cfg: &BuildConfig,
    dims: SceneDims,
    channels: &[ChannelData],
) -> EncodedPayload {
    let compression = cfg.compression;
    let channel_descriptors: Vec<ChannelDescriptor> = channels
        .iter()
        .map(|ch| ChannelDescriptor {
            id: ch.template.id as u8,
            bits_per_pixel: 8,
            compression: compression.as_u8(),
            reserved: 0,
        })
        .collect();

    let per_channel_encoded = encode_channel_strips(cfg, dims, channels, compression);
    let entries = build_strip_entries(
        &per_channel_encoded,
        compression,
        channel_descriptors.len(),
        dims.strip_count as usize,
    );

    EncodedPayload {
        channel_descriptors,
        entries,
        per_channel_encoded,
    }
}

fn encode_channel_strips(
    cfg: &BuildConfig,
    dims: SceneDims,
    channels: &[ChannelData],
    compression: Compression,
) -> Vec<Vec<Vec<u8>>> {
    let mut per_channel_encoded: Vec<Vec<Vec<u8>>> = Vec::with_capacity(channels.len());
    for ch in channels {
        let mut channel_strips = Vec::with_capacity(dims.strip_count as usize);
        for strip_idx in 0..dims.strip_count as usize {
            let y0 = strip_idx * cfg.strip_height as usize;
            let y1 = ((strip_idx + 1) * cfg.strip_height as usize).min(dims.height);
            let rows = y1 - y0;
            let start = y0 * dims.width;
            let end = start + rows * dims.width;
            let raw = &ch.pixels[start..end];
            channel_strips.push(encode_strip(raw, compression));
        }
        per_channel_encoded.push(channel_strips);
    }
    per_channel_encoded
}

fn build_strip_entries(
    per_channel_encoded: &[Vec<Vec<u8>>],
    compression: Compression,
    channel_count: usize,
    strip_count: usize,
) -> Vec<StripEntry> {
    let mut entries: Vec<StripEntry> = Vec::with_capacity(channel_count * strip_count);
    let mut payload_offset = payload_start(channel_count, strip_count) as u64;
    for channel_strips in per_channel_encoded {
        for strip in channel_strips {
            let length = strip.len() as u32;
            let raw_length = decode_len_hint(strip, compression)
                .unwrap_or_else(|| raw_len_from_strip(strip, compression))
                as u32;
            entries.push(StripEntry {
                offset: payload_offset,
                length,
                raw_length,
            });
            payload_offset += length as u64;
        }
    }
    entries
}
