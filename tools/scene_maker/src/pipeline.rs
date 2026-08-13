//! Bundle build pipeline.
//!
//! The stages are split by responsibility -- [`build`] orchestrates, [`channels`]
//! loads and derives the per-channel pixel maps, [`encode`] turns them into
//! compressed strips, and [`output`] writes the bundle and its metadata. The data
//! model they share lives here.

mod build;
mod channels;
mod encode;
mod output;
#[cfg(test)]
mod tests;

use serde::Serialize;

use crate::{
    cli::ChannelTemplate,
    format::{ChannelDescriptor, StripEntry},
};

pub(crate) use build::{run_build, run_inspect};

#[derive(Clone)]
struct ChannelData {
    template: ChannelTemplate,
    source: String,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
struct SceneDims {
    width: usize,
    height: usize,
    total_px: usize,
    strip_count: u16,
}

struct EncodedPayload {
    channel_descriptors: Vec<ChannelDescriptor>,
    entries: Vec<StripEntry>,
    per_channel_encoded: Vec<Vec<Vec<u8>>>,
}

#[derive(Serialize)]
struct Metadata {
    width: u16,
    height: u16,
    strip_height: u16,
    strip_count: u16,
    compression: String,
    bundle_bytes: u64,
    channels: Vec<MetadataChannel>,
}

#[derive(Serialize)]
struct MetadataChannel {
    id: u8,
    name: String,
    source: String,
    min: u8,
    max: u8,
    mean: f32,
}
