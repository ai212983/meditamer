//! Codec, size, borrow-layout, and data-polarity tests for the sky/sun pack.
//!
//! Payloads here are synthetic (no PNGs, no canonical artwork): patterned
//! bytes built to valid geometry, plus the real header layout.

use ambient_assets::*;

#[cfg(test)]
extern crate std;

fn err_of(bytes: &[u8]) -> Result<(), Error> {
    decode(bytes).map(|_| ())
}

fn header_for(sun_w: u16, sun_h: u16, sky_len: u32, sun_len: u32, crc: u32) -> std::vec::Vec<u8> {
    let mut h = std::vec![0u8; HEADER_LEN];
    h[0..8].copy_from_slice(&MAGIC);
    h[8..10].copy_from_slice(&(SKY_W as u16).to_le_bytes());
    h[10..12].copy_from_slice(&(SKY_H as u16).to_le_bytes());
    h[12..14].copy_from_slice(&sun_w.to_le_bytes());
    h[14..16].copy_from_slice(&sun_h.to_le_bytes());
    h[16..20].copy_from_slice(&62.5f32.to_le_bytes());
    h[20..24].copy_from_slice(&67.5f32.to_le_bytes());
    h[24..28].copy_from_slice(&sky_len.to_le_bytes());
    h[28..32].copy_from_slice(&sun_len.to_le_bytes());
    h[32..36].copy_from_slice(&crc.to_le_bytes());
    h
}

fn file_for(sun_w: usize, sun_h: usize) -> std::vec::Vec<u8> {
    let stride = sun_w.div_ceil(8);
    let sky: std::vec::Vec<u8> = (0..SKY_LEN).map(|i| (i & 0xFF) as u8).collect();
    let ink: std::vec::Vec<u8> = (0..stride * sun_h).map(|i| (i & 0xFF) as u8).collect();
    let mask: std::vec::Vec<u8> = std::vec![0xFF; stride * sun_h];
    let mut payload = std::vec::Vec::new();
    payload.extend_from_slice(&sky);
    payload.extend_from_slice(&ink);
    payload.extend_from_slice(&mask);
    let mut f = header_for(
        sun_w as u16,
        sun_h as u16,
        SKY_LEN as u32,
        ink.len() as u32,
        crc32(&payload),
    );
    f.extend_from_slice(&payload);
    f
}

#[test]
fn sizes_match_geometry() {
    assert_eq!(SKY_LEN, 45_000);
    assert_eq!(HEADER_LEN, 64);
    assert!(MAX_FILE_LEN >= HEADER_LEN + SKY_LEN + 2 * (183 * 133usize.div_ceil(8)));
}

#[test]
fn crc32_matches_ieee_vector() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn canonical_clip_decodes_with_borrowed_planes() {
    let file = file_for(183, 133);
    let assets = decode(&file).expect("valid pack");
    assert_eq!((assets.sun_w(), assets.sun_h()), (183, 133));
    assert_eq!(assets.sun_stride(), 23);
    assert_eq!(assets.anchor(), (62.5, 67.5));
    assert_eq!(assets.sky().len(), SKY_LEN);
    assert_eq!(assets.sun_ink().len(), 23 * 133);
    assert_eq!(assets.sun_mask().len(), 23 * 133);
    // Polarity: patterned bytes survive verbatim (MSB-first packing is the
    // packer's contract, not the codec's).
    assert_eq!(assets.sky()[1], 1);
    assert!(assets.sun_mask().iter().all(|&b| b == 0xFF));
}

#[test]
fn rejects_short_magic_reserved_and_checksum() {
    let file = file_for(64, 64);
    assert_eq!(err_of(&file[..HEADER_LEN - 1]), Err(Error::BadFileLen));
    let mut bad = file.clone();
    bad[0] ^= 0xFF;
    assert_eq!(err_of(&bad), Err(Error::BadMagic));
    let mut bad = file.clone();
    bad[40] = 1;
    assert_eq!(err_of(&bad), Err(Error::BadReserved));
    let mut bad = file.clone();
    bad[HEADER_LEN] ^= 0xFF;
    assert_eq!(err_of(&bad), Err(Error::BadChecksum));
    // Truncation and over-long files never reach header parsing.
    assert_eq!(err_of(&file[..file.len() - 1]), Err(Error::BadPayloadLen));
    let mut long = file.clone();
    long.extend_from_slice(&[0u8; 8]);
    assert_eq!(err_of(&long), Err(Error::BadPayloadLen));
}

#[test]
fn rejects_bad_geometry() {
    // Zero sun width.
    let sky = std::vec![0u8; SKY_LEN];
    let payload = [&sky[..], &sky[..0], &sky[..0]].concat();
    let file = [
        &header_for(0, 64, SKY_LEN as u32, 0, crc32(&payload))[..],
        &payload,
    ]
    .concat();
    assert_eq!(err_of(&file), Err(Error::BadSunGeometry));
    // Wrong sky geometry: 600 is 0x0258, so 0x59 in the low byte reads 601.
    let mut file = file_for(64, 64);
    file[8] = 0x59;
    assert_eq!(err_of(&file), Err(Error::BadSkyGeometry));
    // Length field disagrees with geometry.
    let mut file = file_for(64, 64);
    file[28] ^= 0xFF;
    assert_eq!(err_of(&file), Err(Error::BadLengthField));
}

#[test]
fn header_only_reports_exact_length_for_both_sun_sizes() {
    for (w, h) in [(183usize, 133usize), (64usize, 64usize)] {
        let file = file_for(w, h);
        assert_eq!(file_len_from_header(&file[..HEADER_LEN]), Ok(file.len()));
        assert_eq!(file_len_from_header(&file), Ok(file.len()));
        assert_eq!(
            decode(&file).unwrap().payload().len(),
            file.len() - HEADER_LEN
        );
    }
}

#[test]
fn header_only_rejects_malformed_overflow_and_over_limit() {
    let file = file_for(64, 64);
    assert_eq!(
        file_len_from_header(&file[..HEADER_LEN - 1]),
        Err(Error::BadFileLen)
    );
    let mut bad = file.clone();
    bad[0] ^= 0xFF;
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadMagic)
    );
    let mut bad = file.clone();
    bad[8] = 0x59;
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadSkyGeometry)
    );
    let mut bad = file.clone();
    bad[12] = 0;
    bad[13] = 0;
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadSunGeometry)
    );
    let mut bad = file.clone();
    bad[12..14].copy_from_slice(&257u16.to_le_bytes());
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadSunGeometry)
    );
    let mut bad = file.clone();
    bad[16..20].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadSunGeometry)
    );
    let mut bad = file.clone();
    bad[28] ^= 0xFF;
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadLengthField)
    );
    let mut bad = file.clone();
    bad[40] = 1;
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadReserved)
    );
    let mut bad = file.clone();
    bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadLengthField)
    );
    let mut bad = file.clone();
    bad[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        file_len_from_header(&bad[..HEADER_LEN]),
        Err(Error::BadLengthField)
    );
}
