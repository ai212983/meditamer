use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::cli::{next_value, print_help};

const MAGIC: &[u8; 8] = b"SMBNDL1\0";
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 24;

#[derive(Clone, Copy)]
struct ChannelDesc {
    id: u8,
    bits_per_pixel: u8,
    compression: u8,
    _reserved: u8,
}

#[derive(Clone, Copy)]
struct StripEntry {
    offset: u64,
    length: u32,
    raw_length: u32,
}

pub(crate) struct Bundle {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) strip_height: u16,
    pub(crate) strip_count: u16,
    pub(crate) channels: HashMap<u8, Vec<u8>>,
}

pub(crate) fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut bundle = PathBuf::from("tools/scene_maker/out/scene.scenebundle");
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bundle" => bundle = PathBuf::from(next_value("--bundle", &mut it)?),
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown inspect arg '{arg}'")),
        }
    }

    let b = load_bundle(&bundle)?;
    println!("bundle: {}", bundle.display());
    println!("size: {}x{}", b.width, b.height);
    println!("strip_height: {}", b.strip_height);
    println!("strip_count: {}", b.strip_count);
    println!("channels: {}", b.channels.len());

    for (id, data) in &b.channels {
        println!("  id={id} bytes={}", data.len());
    }

    Ok(())
}

pub(crate) fn load_bundle(path: &Path) -> Result<Bundle, String> {
    let bytes = fs::read(path).map_err(|e| format!("read bundle {}: {e}", path.display()))?;
    let mut offset = 0usize;

    let magic = read_bytes(&bytes, &mut offset, 8, "magic")?;
    if magic != MAGIC {
        return Err("invalid bundle magic".to_owned());
    }

    let version = read_u16(&bytes, &mut offset, "version")?;
    if version != VERSION {
        return Err(format!("unsupported bundle version {version}"));
    }

    let header_len = read_u16(&bytes, &mut offset, "header_len")?;
    if header_len != HEADER_LEN {
        return Err(format!("unsupported header length {header_len}"));
    }

    let width = read_u16(&bytes, &mut offset, "width")?;
    let height = read_u16(&bytes, &mut offset, "height")?;
    let strip_height = read_u16(&bytes, &mut offset, "strip_height")?;
    let strip_count = read_u16(&bytes, &mut offset, "strip_count")?;
    let channel_count = read_u16(&bytes, &mut offset, "channel_count")?;
    let _flags = read_u16(&bytes, &mut offset, "flags")?;

    let mut descs = Vec::with_capacity(channel_count as usize);
    for i in 0..channel_count as usize {
        let raw = read_bytes(&bytes, &mut offset, 4, &format!("channel desc {i}"))?;
        descs.push(ChannelDesc {
            id: raw[0],
            bits_per_pixel: raw[1],
            compression: raw[2],
            _reserved: raw[3],
        });
    }

    let entry_count = (channel_count as usize) * (strip_count as usize);
    let mut entries = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let off = read_u64(&bytes, &mut offset, &format!("strip entry {i} offset"))?;
        let len = read_u32(&bytes, &mut offset, &format!("strip entry {i} len"))?;
        let raw_len = read_u32(&bytes, &mut offset, &format!("strip entry {i} raw_len"))?;
        entries.push(StripEntry {
            offset: off,
            length: len,
            raw_length: raw_len,
        });
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let mut channels = HashMap::with_capacity(channel_count as usize);

    for (ch_idx, desc) in descs.iter().enumerate() {
        if desc.bits_per_pixel != 8 {
            return Err(format!(
                "unsupported bits_per_pixel={} for channel id={}",
                desc.bits_per_pixel, desc.id
            ));
        }

        let mut decoded = Vec::with_capacity(width_usize * height_usize);
        for strip_idx in 0..strip_count as usize {
            let entry = entries[ch_idx * (strip_count as usize) + strip_idx];
            let start = entry.offset as usize;
            let end = start + entry.length as usize;
            if end > bytes.len() {
                return Err(format!(
                    "invalid strip bounds for channel id={} strip={strip_idx}",
                    desc.id
                ));
            }

            let payload = &bytes[start..end];
            let expected_rows = strip_rows(
                strip_idx,
                strip_count as usize,
                strip_height as usize,
                height_usize,
            );
            let expected_len = expected_rows * width_usize;
            if entry.raw_length as usize != expected_len {
                return Err(format!(
                    "strip raw_length mismatch channel id={} strip={} expected={} got={}",
                    desc.id, strip_idx, expected_len, entry.raw_length
                ));
            }

            let mut strip = decode_strip(payload, entry.raw_length as usize, desc.compression)
                .map_err(|e| format!("decode channel id={} strip={strip_idx}: {e}", desc.id))?;
            decoded.append(&mut strip);
        }

        if decoded.len() != width_usize * height_usize {
            return Err(format!(
                "decoded channel id={} size mismatch expected={} got={}",
                desc.id,
                width_usize * height_usize,
                decoded.len()
            ));
        }

        channels.insert(desc.id, decoded);
    }

    Ok(Bundle {
        width,
        height,
        strip_height,
        strip_count,
        channels,
    })
}

fn strip_rows(strip_idx: usize, strip_count: usize, strip_height: usize, height: usize) -> usize {
    let y0 = strip_idx * strip_height;
    let y1 = ((strip_idx + 1) * strip_height).min(height);
    if strip_idx >= strip_count || y0 >= height {
        0
    } else {
        y1 - y0
    }
}

fn decode_strip(payload: &[u8], expected_len: usize, compression: u8) -> Result<Vec<u8>, String> {
    match compression {
        0 => {
            if payload.len() != expected_len {
                return Err(format!(
                    "raw strip length mismatch expected={} got={}",
                    expected_len,
                    payload.len()
                ));
            }
            Ok(payload.to_vec())
        }
        1 => rle_decode(payload, expected_len),
        _ => Err(format!("unsupported compression code {compression}")),
    }
}

fn rle_decode(payload: &[u8], expected_len: usize) -> Result<Vec<u8>, String> {
    if payload.len() % 2 != 0 {
        return Err("rle payload must have even length".to_owned());
    }

    let mut out = Vec::with_capacity(expected_len);
    let mut i = 0usize;
    while i < payload.len() {
        let run = payload[i] as usize;
        let value = payload[i + 1];
        i += 2;

        if run == 0 {
            return Err("rle run length 0 is invalid".to_owned());
        }

        for _ in 0..run {
            out.push(value);
        }
    }

    if out.len() != expected_len {
        return Err(format!(
            "rle decoded length mismatch expected={} got={}",
            expected_len,
            out.len()
        ));
    }

    Ok(out)
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    len: usize,
    what: &str,
) -> Result<&'a [u8], String> {
    let start = *offset;
    let end = start + len;
    if end > bytes.len() {
        return Err(format!("unexpected eof while reading {what}"));
    }
    *offset = end;
    Ok(&bytes[start..end])
}

fn read_u16(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u16, String> {
    let raw = read_bytes(bytes, offset, 2, what)?;
    let mut b = [0u8; 2];
    b.copy_from_slice(raw);
    Ok(u16::from_le_bytes(b))
}

fn read_u32(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u32, String> {
    let raw = read_bytes(bytes, offset, 4, what)?;
    let mut b = [0u8; 4];
    b.copy_from_slice(raw);
    Ok(u32::from_le_bytes(b))
}

fn read_u64(bytes: &[u8], offset: &mut usize, what: &str) -> Result<u64, String> {
    let raw = read_bytes(bytes, offset, 8, what)?;
    let mut b = [0u8; 8];
    b.copy_from_slice(raw);
    Ok(u64::from_le_bytes(b))
}
