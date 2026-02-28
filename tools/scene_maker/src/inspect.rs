use std::{fs, io::Read, path::PathBuf};

use crate::{
    cli::{next_value, print_help},
    format::{compression_name, read_header, ChannelDescriptor, StripEntry},
};

pub(crate) fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut bundle: Option<PathBuf> = None;
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bundle" => bundle = Some(PathBuf::from(next_value("--bundle", &mut it)?)),
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown arg for inspect: {arg}")),
        }
    }

    let bundle = bundle.unwrap_or_else(|| PathBuf::from("tools/scene_maker/out/scene.scenebundle"));
    let mut file =
        fs::File::open(&bundle).map_err(|e| format!("open bundle {}: {e}", bundle.display()))?;

    let (width, height, strip_height, strip_count, channel_count) = read_header(&mut file)?;
    println!("bundle: {}", bundle.display());
    println!("size: {}x{}", width, height);
    println!("strip height: {strip_height}, strip count: {strip_count}");
    println!("channels: {channel_count}");

    let mut descs = Vec::with_capacity(channel_count as usize);
    for idx in 0..channel_count {
        let mut b = [0u8; 4];
        file.read_exact(&mut b)
            .map_err(|e| format!("read channel descriptor {idx}: {e}"))?;
        descs.push(ChannelDescriptor {
            id: b[0],
            bits_per_pixel: b[1],
            compression: b[2],
            reserved: b[3],
        });
    }

    let entry_count = (channel_count as usize) * (strip_count as usize);
    let mut entries = Vec::with_capacity(entry_count);
    for idx in 0..entry_count {
        let mut off = [0u8; 8];
        let mut len = [0u8; 4];
        let mut raw = [0u8; 4];
        file.read_exact(&mut off)
            .map_err(|e| format!("read strip entry offset {idx}: {e}"))?;
        file.read_exact(&mut len)
            .map_err(|e| format!("read strip entry len {idx}: {e}"))?;
        file.read_exact(&mut raw)
            .map_err(|e| format!("read strip entry raw len {idx}: {e}"))?;
        entries.push(StripEntry {
            offset: u64::from_le_bytes(off),
            length: u32::from_le_bytes(len),
            raw_length: u32::from_le_bytes(raw),
        });
    }

    for (ch_idx, desc) in descs.iter().enumerate() {
        let mut encoded_total = 0u64;
        let mut raw_total = 0u64;
        for strip_idx in 0..strip_count as usize {
            let entry = entries[ch_idx * (strip_count as usize) + strip_idx];
            encoded_total += entry.length as u64;
            raw_total += entry.raw_length as u64;
        }
        let ratio = if raw_total == 0 {
            1.0
        } else {
            encoded_total as f32 / raw_total as f32
        };
        println!(
            "  channel id={} bpp={} compression={} encoded={} raw={} ratio={:.3}",
            desc.id,
            desc.bits_per_pixel,
            compression_name(desc.compression),
            encoded_total,
            raw_total,
            ratio
        );
    }

    Ok(())
}
