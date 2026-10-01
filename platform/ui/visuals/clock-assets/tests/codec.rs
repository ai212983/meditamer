//! Codec, size, borrow-layout, and data-polarity tests for the 600-map pack.
//!
//! Payloads here are synthetic (no PNGs, no canonical artwork): patterned
//! bytes built to the exact canonical segment lengths.

use clock_assets::*;

#[cfg(test)]
extern crate std;

fn patterned_payload() -> std::vec::Vec<u8> {
    let mut v = std::vec::Vec::with_capacity(PAYLOAD_LEN);
    for i in 0..PAYLOAD_LEN {
        v.push((i & 0xFF) as u8);
    }
    v
}

fn file_for(payload: &[u8]) -> std::vec::Vec<u8> {
    let header = encode_header(payload).expect("valid payload");
    let mut f = std::vec::Vec::with_capacity(FILE_LEN);
    f.extend_from_slice(&header);
    f.extend_from_slice(payload);
    f
}

#[test]
fn sizes_match_canonical_geometry() {
    assert_eq!(DIAL_LEN, 600 * 600);
    assert_eq!(HOUR_PIXELS, 245 * 810);
    assert_eq!(HOUR_NORMAL_LEN, 245 * 810 * 3);
    assert_eq!(MINUTE_PIXELS, 156 * 1014);
    assert_eq!(MINUTE_NORMAL_LEN, 156 * 1014 * 3);
    assert_eq!(PAYLOAD_LEN, 2_499_804);
    assert_eq!(HEADER_LEN, 32);
    assert_eq!(FILE_LEN, 2_499_836);
    assert_eq!(OFF_MINUTE_SPEC + MINUTE_PIXELS, PAYLOAD_LEN);
    // Segments are contiguous in canonical order.
    assert_eq!(OFF_HOUR_ALBEDO, OFF_DIAL + DIAL_LEN);
    assert_eq!(OFF_HOUR_ALPHA, OFF_HOUR_ALBEDO + HOUR_PIXELS);
    assert_eq!(OFF_HOUR_NORMAL, OFF_HOUR_ALPHA + HOUR_PIXELS);
    assert_eq!(OFF_HOUR_SPEC, OFF_HOUR_NORMAL + HOUR_NORMAL_LEN);
    assert_eq!(OFF_MINUTE_ALBEDO, OFF_HOUR_SPEC + HOUR_PIXELS);
    assert_eq!(OFF_MINUTE_ALPHA, OFF_MINUTE_ALBEDO + MINUTE_PIXELS);
    assert_eq!(OFF_MINUTE_NORMAL, OFF_MINUTE_ALPHA + MINUTE_PIXELS);
    assert_eq!(OFF_MINUTE_SPEC, OFF_MINUTE_NORMAL + MINUTE_NORMAL_LEN);
}

#[test]
fn header_fields_are_magic_len_crc_and_zero_reserved() {
    let payload = patterned_payload();
    let header = encode_header(&payload).expect("encode");
    assert_eq!(&header[0..8], b"MCLKMAP1");
    assert_eq!(
        u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize,
        PAYLOAD_LEN
    );
    assert_eq!(
        u32::from_le_bytes([header[12], header[13], header[14], header[15]]),
        crc32(&payload)
    );
    assert!(header[16..32].iter().all(|&b| b == 0));
}

#[test]
fn encode_rejects_wrong_payload_len() {
    assert_eq!(encode_header(&[0u8; 8]).unwrap_err(), Error::BadPayloadLen);
    let mut v = patterned_payload();
    v.push(0);
    assert_eq!(encode_header(&v).unwrap_err(), Error::BadPayloadLen);
}

#[test]
fn round_trip_borrows_exact_segments_and_pivots() {
    let payload = patterned_payload();
    let file = file_for(&payload);
    let assets = decode(&file).expect("decode");
    assert_eq!(assets.payload().len(), PAYLOAD_LEN);
    assert_eq!(assets.payload(), payload.as_slice());
    let dial = assets.dial();
    assert!(dial.validate());
    assert_eq!((dial.width, dial.height), (600, 600));
    assert_eq!(dial.pixels, &payload[OFF_DIAL..OFF_HOUR_ALBEDO]);
    let hands = assets.hands();
    assert_eq!((hands.hour.width, hands.hour.height), (245, 810));
    assert_eq!((hands.minute.width, hands.minute.height), (156, 1014));
    assert_eq!(hands.hour.albedo, &payload[OFF_HOUR_ALBEDO..OFF_HOUR_ALPHA]);
    assert_eq!(hands.hour.alpha, &payload[OFF_HOUR_ALPHA..OFF_HOUR_NORMAL]);
    assert_eq!(hands.hour.normal, &payload[OFF_HOUR_NORMAL..OFF_HOUR_SPEC]);
    assert_eq!(hands.hour.spec, &payload[OFF_HOUR_SPEC..OFF_MINUTE_ALBEDO]);
    assert_eq!(
        hands.minute.albedo,
        &payload[OFF_MINUTE_ALBEDO..OFF_MINUTE_ALPHA]
    );
    assert_eq!(
        hands.minute.alpha,
        &payload[OFF_MINUTE_ALPHA..OFF_MINUTE_NORMAL]
    );
    assert_eq!(
        hands.minute.normal,
        &payload[OFF_MINUTE_NORMAL..OFF_MINUTE_SPEC]
    );
    assert_eq!(hands.minute.spec, &payload[OFF_MINUTE_SPEC..PAYLOAD_LEN]);
    // Canonical pivots reuse the analog-clock constants.
    assert_eq!(
        (hands.hour.pivot_x, hands.hour.pivot_y),
        analog_clock::HOUR_PIVOT
    );
    assert_eq!(
        (hands.minute.pivot_x, hands.minute.pivot_y),
        analog_clock::MINUTE_PIVOT
    );
    // Sprite geometry lengths hold even before any core length-contract
    // change: gray albedo is one byte per pixel, normals 3x.
    assert_eq!(hands.hour.albedo.len(), HOUR_PIXELS);
    assert_eq!(hands.hour.alpha.len(), HOUR_PIXELS);
    assert_eq!(hands.hour.normal.len(), HOUR_NORMAL_LEN);
    assert_eq!(hands.hour.spec.len(), HOUR_PIXELS);
    assert_eq!(hands.minute.albedo.len(), MINUTE_PIXELS);
    assert_eq!(hands.minute.alpha.len(), MINUTE_PIXELS);
    assert_eq!(hands.minute.normal.len(), MINUTE_NORMAL_LEN);
    assert_eq!(hands.minute.spec.len(), MINUTE_PIXELS);
    // Dial validate passes on the borrowed canonical geometry.
    assert!(assets.dial().validate());
}

#[test]
fn truncated_and_extended_files_rejected() {
    let payload = patterned_payload();
    let file = file_for(&payload);
    assert_eq!(
        decode(&file[..FILE_LEN - 1]).unwrap_err(),
        Error::BadFileLen
    );
    assert_eq!(decode(&file[..100]).unwrap_err(), Error::BadFileLen);
    let mut longer = file.clone();
    longer.push(0);
    assert_eq!(decode(&longer).unwrap_err(), Error::BadFileLen);
    assert_eq!(decode(&[]).unwrap_err(), Error::BadFileLen);
}

#[test]
fn bad_magic_rejected() {
    let payload = patterned_payload();
    let mut file = file_for(&payload);
    file[0] = b'X';
    assert_eq!(decode(&file).unwrap_err(), Error::BadMagic);
}

#[test]
fn nonzero_reserved_rejected() {
    let payload = patterned_payload();
    let mut file = file_for(&payload);
    file[20] = 1;
    assert_eq!(decode(&file).unwrap_err(), Error::BadReserved);
    let mut file = file_for(&payload);
    file[31] = 0xFF;
    assert_eq!(decode(&file).unwrap_err(), Error::BadReserved);
}

#[test]
fn wrong_length_field_rejected() {
    let payload = patterned_payload();
    let mut file = file_for(&payload);
    file[8..12].copy_from_slice(&1234u32.to_le_bytes());
    assert_eq!(decode(&file).unwrap_err(), Error::BadLengthField);
}

#[test]
fn corrupt_payload_rejected_by_crc() {
    let payload = patterned_payload();
    let mut file = file_for(&payload);
    let last = file.len() - 1;
    file[last] ^= 0xFF;
    assert_eq!(decode(&file).unwrap_err(), Error::BadChecksum);
    let mut file = file_for(&payload);
    file[HEADER_LEN] ^= 0x01;
    assert_eq!(decode(&file).unwrap_err(), Error::BadChecksum);
}

#[test]
fn crc32_matches_ieee_reference() {
    // Standard check value for "123456789".
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0x0000_0000);
}

#[test]
fn normal_polarity_data_passes_through_untouched() {
    // The pack stores normal RGB bytes verbatim (green-up OpenGL convention
    // on disk); the core flips green at sample time. The codec must never
    // alter them: plant a top-rim G > 128 and bottom-rim G < 128 pair and
    // check the borrowed bytes are identical.
    let mut payload = patterned_payload();
    payload[OFF_HOUR_NORMAL + 1] = 200; // G > 128
    payload[OFF_HOUR_NORMAL + HOUR_NORMAL_LEN - 2] = 40; // G < 128
    let file = file_for(&payload);
    let assets = decode(&file).expect("decode");
    let hands = assets.hands();
    assert_eq!(hands.hour.normal[1], 200);
    assert_eq!(hands.hour.normal[HOUR_NORMAL_LEN - 2], 40);
}
