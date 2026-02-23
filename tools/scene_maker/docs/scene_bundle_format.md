# Scene Bundle Format (`.scenebundle`)

Version: `1`

## Layout
1. Header (`24` bytes)
2. Channel descriptors (`4` bytes each)
3. Strip directory entries (`16` bytes each)
4. Payload bytes (channel-major, strip-major)

## Header (little-endian)
- `magic[8]`: `SMBNDL1\0`
- `version u16`
- `header_len u16` (currently `24`)
- `width u16`
- `height u16`
- `strip_height u16`
- `strip_count u16`
- `channel_count u16`
- `flags u16` (reserved)

## Channel descriptor (`4` bytes)
- `id u8`
  - `1`: albedo
  - `2`: light
  - `3`: ao
  - `4`: depth
  - `5`: edge
  - `6`: mask
  - `7`: stroke
- `bits_per_pixel u8` (`8`)
- `compression u8`
  - `0`: raw
  - `1`: RLE (run,value pairs)
- `reserved u8`

## Strip entry (`16` bytes)
- `offset u64` (absolute file offset)
- `length u32` (encoded length)
- `raw_length u32` (decoded bytes)

Entries are stored in channel-major order, then strip index order.

## Compression
RLE encoding is byte-oriented and stores repeated bytes as `(run_len, value)` pairs where `run_len` is `1..255`.
