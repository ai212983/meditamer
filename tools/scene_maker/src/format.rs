use std::io::{Read, Write};

use crate::cli::Compression;

const MAGIC: &[u8; 8] = b"SMBNDL1\0";
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 24;
const CHANNEL_DESC_LEN: usize = 4;
const STRIP_ENTRY_LEN: usize = 16;

#[derive(Clone, Copy)]
pub(crate) struct ChannelDescriptor {
    pub(crate) id: u8,
    pub(crate) bits_per_pixel: u8,
    pub(crate) compression: u8,
    pub(crate) reserved: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct StripEntry {
    pub(crate) offset: u64,
    pub(crate) length: u32,
    pub(crate) raw_length: u32,
}

pub(crate) fn payload_start(channel_count: usize, strip_count: usize) -> usize {
    let header_bytes = HEADER_LEN as usize;
    let channel_desc_bytes = channel_count * CHANNEL_DESC_LEN;
    let strip_entry_bytes = channel_count * strip_count * STRIP_ENTRY_LEN;
    header_bytes + channel_desc_bytes + strip_entry_bytes
}

pub(crate) fn encode_strip(raw: &[u8], compression: Compression) -> Vec<u8> {
    match compression {
        Compression::None => raw.to_vec(),
        Compression::Rle => rle_encode(raw),
    }
}

pub(crate) fn decode_len_hint(strip: &[u8], compression: Compression) -> Option<usize> {
    match compression {
        Compression::None => Some(strip.len()),
        Compression::Rle => {
            if !strip.len().is_multiple_of(2) {
                return None;
            }
            let mut len = 0usize;
            let mut i = 0usize;
            while i < strip.len() {
                len += strip[i] as usize;
                i += 2;
            }
            Some(len)
        }
    }
}

pub(crate) fn raw_len_from_strip(strip: &[u8], compression: Compression) -> usize {
    match compression {
        Compression::None => strip.len(),
        Compression::Rle => strip
            .chunks_exact(2)
            .map(|pair| pair[0] as usize)
            .sum::<usize>(),
    }
}

fn rle_encode(raw: &[u8]) -> Vec<u8> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(raw.len() / 2);
    let mut i = 0;
    while i < raw.len() {
        let value = raw[i];
        let mut run = 1usize;
        while i + run < raw.len() && raw[i + run] == value && run < 255 {
            run += 1;
        }
        out.push(run as u8);
        out.push(value);
        i += run;
    }
    out
}

pub(crate) fn write_header<W: Write>(
    mut out: W,
    width: u16,
    height: u16,
    strip_height: u16,
    strip_count: u16,
    channel_count: u16,
) -> Result<(), String> {
    out.write_all(MAGIC)
        .map_err(|e| format!("write header magic: {e}"))?;
    out.write_all(&VERSION.to_le_bytes())
        .map_err(|e| format!("write header version: {e}"))?;
    out.write_all(&HEADER_LEN.to_le_bytes())
        .map_err(|e| format!("write header len: {e}"))?;
    out.write_all(&width.to_le_bytes())
        .map_err(|e| format!("write header width: {e}"))?;
    out.write_all(&height.to_le_bytes())
        .map_err(|e| format!("write header height: {e}"))?;
    out.write_all(&strip_height.to_le_bytes())
        .map_err(|e| format!("write header strip height: {e}"))?;
    out.write_all(&strip_count.to_le_bytes())
        .map_err(|e| format!("write header strip count: {e}"))?;
    out.write_all(&channel_count.to_le_bytes())
        .map_err(|e| format!("write header channel count: {e}"))?;
    out.write_all(&0u16.to_le_bytes())
        .map_err(|e| format!("write header flags: {e}"))?;
    Ok(())
}

pub(crate) fn read_header<R: Read>(mut r: R) -> Result<(u16, u16, u16, u16, u16), String> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)
        .map_err(|e| format!("read header magic: {e}"))?;
    if &magic != MAGIC {
        return Err("invalid magic; not a scene bundle".to_owned());
    }

    let version = read_u16(&mut r, "version")?;
    if version != VERSION {
        return Err(format!("unsupported bundle version {version}"));
    }

    let header_len = read_u16(&mut r, "header_len")?;
    if header_len as usize != HEADER_LEN as usize {
        return Err(format!("unsupported header length {header_len}"));
    }

    let width = read_u16(&mut r, "width")?;
    let height = read_u16(&mut r, "height")?;
    let strip_height = read_u16(&mut r, "strip_height")?;
    let strip_count = read_u16(&mut r, "strip_count")?;
    let channel_count = read_u16(&mut r, "channel_count")?;
    let _flags = read_u16(&mut r, "flags")?;

    Ok((width, height, strip_height, strip_count, channel_count))
}

fn read_u16<R: Read>(r: &mut R, what: &str) -> Result<u16, String> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)
        .map_err(|e| format!("read {what}: {e}"))?;
    Ok(u16::from_le_bytes(buf))
}

pub(crate) fn compression_name(code: u8) -> &'static str {
    match code {
        0 => "none",
        1 => "rle",
        _ => "unknown",
    }
}
