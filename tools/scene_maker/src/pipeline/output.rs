use std::{fs, io::Write, path::Path};

use super::{ChannelData, EncodedPayload, Metadata, MetadataChannel};
use crate::{
    cli::BuildConfig,
    format::{write_header, BundleHeader, ChannelDescriptor, StripEntry},
};

pub(super) fn write_bundle(
    cfg: &BuildConfig,
    strip_count: u16,
    encoded: &EncodedPayload,
) -> Result<u64, String> {
    let mut out = fs::File::create(&cfg.out_bundle)
        .map_err(|e| format!("create bundle {}: {e}", cfg.out_bundle.display()))?;
    write_bundle_header(
        &mut out,
        cfg,
        strip_count,
        encoded.channel_descriptors.len() as u16,
    )?;
    write_channel_descriptors(&mut out, &encoded.channel_descriptors)?;
    write_strip_entries(&mut out, &encoded.entries)?;
    write_strip_payload(&mut out, &encoded.per_channel_encoded)?;
    out.flush().map_err(|e| format!("flush bundle: {e}"))?;
    bundle_size(&cfg.out_bundle)
}

fn write_bundle_header(
    out: &mut fs::File,
    cfg: &BuildConfig,
    strip_count: u16,
    channel_count: u16,
) -> Result<(), String> {
    write_header(
        out,
        BundleHeader {
            width: cfg.width,
            height: cfg.height,
            strip_height: cfg.strip_height,
            strip_count,
            channel_count,
        },
    )
}

fn write_channel_descriptors(
    out: &mut fs::File,
    channel_descriptors: &[ChannelDescriptor],
) -> Result<(), String> {
    for desc in channel_descriptors {
        out.write_all(&[
            desc.id,
            desc.bits_per_pixel,
            desc.compression,
            desc.reserved,
        ])
        .map_err(|e| format!("write channel descriptor: {e}"))?;
    }
    Ok(())
}

fn write_strip_entries(out: &mut fs::File, entries: &[StripEntry]) -> Result<(), String> {
    for entry in entries {
        out.write_all(&entry.offset.to_le_bytes())
            .map_err(|e| format!("write strip offset: {e}"))?;
        out.write_all(&entry.length.to_le_bytes())
            .map_err(|e| format!("write strip length: {e}"))?;
        out.write_all(&entry.raw_length.to_le_bytes())
            .map_err(|e| format!("write strip raw length: {e}"))?;
    }
    Ok(())
}

fn write_strip_payload(
    out: &mut fs::File,
    per_channel_encoded: &[Vec<Vec<u8>>],
) -> Result<(), String> {
    for channel_strips in per_channel_encoded {
        for strip in channel_strips {
            out.write_all(strip)
                .map_err(|e| format!("write strip payload: {e}"))?;
        }
    }
    Ok(())
}

fn bundle_size(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map_err(|e| format!("read bundle metadata: {e}"))
        .map(|m| m.len())
}

pub(super) fn build_metadata(
    cfg: &BuildConfig,
    strip_count: u16,
    bundle_bytes: u64,
    channels: &[ChannelData],
) -> Metadata {
    Metadata {
        width: cfg.width,
        height: cfg.height,
        strip_height: cfg.strip_height,
        strip_count,
        compression: cfg.compression.as_str().to_owned(),
        bundle_bytes,
        channels: channels
            .iter()
            .map(|ch| {
                let (min, max, mean) = stats(&ch.pixels);
                MetadataChannel {
                    id: ch.template.id as u8,
                    name: ch.template.name.to_owned(),
                    source: ch.source.clone(),
                    min,
                    max,
                    mean,
                }
            })
            .collect(),
    }
}

pub(super) fn write_metadata(cfg: &BuildConfig, meta: &Metadata) -> Result<(), String> {
    let meta_json =
        serde_json::to_string_pretty(meta).map_err(|e| format!("serialize metadata: {e}"))?;
    fs::write(&cfg.metadata_out, meta_json)
        .map_err(|e| format!("write metadata {}: {e}", cfg.metadata_out.display()))
}

fn stats(data: &[u8]) -> (u8, u8, f32) {
    if data.is_empty() {
        return (0, 0, 0.0);
    }
    let mut min = u8::MAX;
    let mut max = u8::MIN;
    let mut sum = 0u64;

    for &v in data {
        min = min.min(v);
        max = max.max(v);
        sum += v as u64;
    }

    (min, max, (sum as f32) / (data.len() as f32))
}
