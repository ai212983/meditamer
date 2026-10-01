//! Host proof for the bounded mountain-streaming source slice.
//!
//! Builds one small deterministic pack (3 stored rows, varying planes),
//! then proves the `source` module agrees byte for byte with the legacy
//! `pack::parse` / `Pack` row accessors over contiguous and fragmented
//! stores, that validation and row reads issue a bounded, exact read
//! pattern, that composer output through [`RowScratch`](mountain_snow::source::RowScratch)
//! matches the legacy row slices, and that every failure mode reports its
//! precise error without publishing or using invalid metadata.

use std::cell::RefCell;

use allocator_api2::alloc::Global;
use asset_source::{AccessError, AssetSource, FragmentedSource, SliceSource};
use mountain_snow::composer::{compose_row, Cut, RowInputs};
use mountain_snow::pack::{self, PackError, HEADER_LEN, WIDTH};
use mountain_snow::source::{validate_source, RowScratch, ValidatedPack, ValidationError};
use psram_store::{ChunkLimits, ChunkStore, SequentialWriter};

const ROWS: usize = 3;
const FIRST_ROW: u16 = 10;
const CRC_SCRATCH: usize = 1000;

/// Deterministic pack with per-row, per-plane variation so a swapped plane
/// or row cannot pass the byte comparisons.
fn pack_bytes() -> Vec<u8> {
    let mut rock = Vec::with_capacity(ROWS * WIDTH);
    let mut snow = Vec::with_capacity(ROWS * WIDTH);
    let mut barrier = Vec::with_capacity(ROWS * WIDTH);
    let mut eligible = Vec::with_capacity(ROWS * WIDTH / 8);
    let mut noise = Vec::with_capacity(ROWS * WIDTH);
    for row in 0..ROWS {
        for x in 0..WIDTH {
            rock.push(((row * 131 + x * 7 + 60) % 256) as u8);
            snow.push(((row * 91 + x * 13 + 200) % 256) as u8);
            // Full 0..=255 sweep across x, offset per row, so one cut
            // exercises below/at/above branches plus intra-level mixing.
            barrier.push(((x * 3 + row * 97) % 256) as u8);
            noise.push(((x * 29 + row * 53 + 7) % 256) as u8);
        }
        for x_step in (0..WIDTH).step_by(8) {
            let mut packed = 0u8;
            for bit in 0..8 {
                // Every third pixel ineligible, staggered per row.
                if (x_step + bit + row) % 3 != 0 {
                    packed |= 0x80 >> bit;
                }
            }
            eligible.push(packed);
        }
    }
    let mut payload = Vec::new();
    payload.extend_from_slice(&rock);
    payload.extend_from_slice(&snow);
    payload.extend_from_slice(&barrier);
    payload.extend_from_slice(&eligible);
    payload.extend_from_slice(&noise);
    let mut header = vec![0u8; HEADER_LEN];
    header[..8].copy_from_slice(&pack::MAGIC);
    for (i, v) in [600u16, 600, FIRST_ROW, ROWS as u16].iter().enumerate() {
        header[8 + 2 * i..10 + 2 * i].copy_from_slice(&v.to_le_bytes());
    }
    for (i, section) in [&rock, &snow, &barrier, &eligible, &noise]
        .iter()
        .enumerate()
    {
        header[16 + 4 * i..20 + 4 * i].copy_from_slice(&(section.len() as u32).to_le_bytes());
    }
    header[36..40].copy_from_slice(&pack::crc32_ieee(&payload).to_le_bytes());
    header.extend_from_slice(&payload);
    header
}

fn check_row(scratch: &RowScratch, pack: &pack::Pack<'_>, row: usize) {
    assert_eq!(
        scratch.rock.as_slice(),
        pack.rock_row(row).expect("row"),
        "rock row {row}"
    );
    assert_eq!(
        scratch.snow.as_slice(),
        pack.snow_row(row).expect("row"),
        "snow row {row}"
    );
    assert_eq!(
        scratch.barrier.as_slice(),
        pack.barrier_row(row).expect("row"),
        "barrier row {row}"
    );
    assert_eq!(
        scratch.eligible.as_slice(),
        pack.eligible_row(row).expect("row"),
        "eligible row {row}"
    );
    assert_eq!(
        scratch.noise.as_slice(),
        pack.noise_row(row).expect("row"),
        "noise row {row}"
    );
}

#[test]
fn row_scratch_is_exactly_2475_zeroed_bytes() {
    assert_eq!(core::mem::size_of::<RowScratch>(), 2475);
    let scratch = RowScratch::new();
    assert_eq!(scratch, RowScratch::default());
    assert!(scratch.rock.iter().all(|&b| b == 0));
    assert!(scratch.snow.iter().all(|&b| b == 0));
    assert!(scratch.barrier.iter().all(|&b| b == 0));
    assert!(scratch.eligible.iter().all(|&b| b == 0));
    assert!(scratch.noise.iter().all(|&b| b == 0));
}

#[test]
fn slice_validation_and_rows_match_legacy_accessors() {
    let file = pack_bytes();
    let source = SliceSource::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&source, &mut crc_buf).expect("valid pack");
    let legacy = pack::parse(&file).expect("valid pack");
    assert_eq!(validated.header(), legacy.header());

    let mut scratch = RowScratch::new();
    for row in 0..ROWS {
        validated
            .read_row(&source, row, &mut scratch)
            .expect("in-band row");
        check_row(&scratch, &legacy, row);
    }
}

#[test]
fn ordered_segmented_store_matches_legacy_rows_without_a_second_crc_pass() {
    let file = pack_bytes();
    let limits = ChunkLimits::new(1024, file.len().div_ceil(1024), file.len());
    let mut store = ChunkStore::try_new_zeroed(file.len(), limits, Global).expect("chunks");
    let mut writer = SequentialWriter::new(&mut store);
    for (offset, bytes) in file.chunks(333).enumerate() {
        writer
            .write_at(offset * 333, bytes)
            .expect("ordered stream");
    }
    writer.finish().expect("exact stream");
    let header: [u8; HEADER_LEN] = file[..HEADER_LEN].try_into().unwrap();
    let crc = pack::crc32_ieee(&file[HEADER_LEN..]);
    let validated = ValidatedPack::from_streamed_crc(&header, file.len(), crc).expect("CRC");
    let legacy = pack::parse(&file).expect("legacy");
    let mut scratch = RowScratch::new();
    for row in 0..ROWS {
        validated
            .read_row(&store, row, &mut scratch)
            .expect("resident row");
        check_row(&scratch, &legacy, row);
    }
    assert_eq!(
        ValidatedPack::from_streamed_crc(&header, file.len() - 1, crc),
        Err(PackError::BadFileLen)
    );
    assert_eq!(
        ValidatedPack::from_streamed_crc(&header, file.len(), crc ^ 1),
        Err(PackError::BadCrc)
    );
}

#[test]
fn fragmented_boundaries_match_legacy_accessors() {
    let file = pack_bytes();
    // Cuts inside the header, at the header/payload seam, inside payload
    // chunks, and inside each row plane, including off-by-one neighbors.
    let mut cuts = [
        1,
        17,
        63,
        HEADER_LEN - 1,
        HEADER_LEN,
        HEADER_LEN + 1,
        HEADER_LEN + 7,
        1000,
        3000,
        6000,
        7488,
    ];
    cuts.sort_unstable();
    let mut spans = Vec::new();
    let mut prev = 0;
    for cut in cuts.into_iter().chain([file.len()]) {
        if cut > prev && cut <= file.len() {
            spans.push(&file[prev..cut]);
            prev = cut;
        }
    }
    let source = FragmentedSource::new(&spans).expect("span totals");
    assert_eq!(source.len(), file.len());

    let mut crc_buf = vec![0u8; 100];
    let validated = validate_source(&source, &mut crc_buf).expect("valid pack");
    let legacy = pack::parse(&file).expect("valid pack");
    assert_eq!(validated.header(), legacy.header());

    let mut scratch = RowScratch::new();
    for row in 0..ROWS {
        validated
            .read_row(&source, row, &mut scratch)
            .expect("in-band row");
        check_row(&scratch, &legacy, row);
    }
}

/// Records every `read_exact_at` and panics on `borrow_at`, proving the
/// source module never takes the borrow fast path.
struct Counting<'a> {
    inner: SliceSource<'a>,
    calls: RefCell<Vec<(usize, usize)>>,
}

impl<'a> Counting<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            inner: SliceSource::new(bytes),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl AssetSource for Counting<'_> {
    type Error = AccessError;

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        self.calls.borrow_mut().push((offset, dst.len()));
        self.inner.read_exact_at(offset, dst)
    }

    fn borrow_at(&self, _offset: usize, _len: usize) -> Result<Option<&[u8]>, AccessError> {
        panic!("source module must not use borrow_at");
    }
}

#[test]
fn validation_reads_header_once_then_bounded_payload_chunks() {
    let file = pack_bytes();
    let source = Counting::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&source, &mut crc_buf).expect("valid pack");

    let calls = source.calls.borrow();
    // Exactly one 64-byte header read, first.
    assert_eq!(calls[0], (0, HEADER_LEN));
    assert_eq!(calls.iter().filter(|&&(o, _)| o == 0).count(), 1);
    // Payload follows as contiguous bounded chunks covering it exactly once.
    let header = validated.header();
    let payload_calls = &calls[1..];
    assert!(!payload_calls.is_empty());
    assert!(payload_calls
        .iter()
        .all(|&(_, len)| len > 0 && len <= CRC_SCRATCH));
    let total: usize = payload_calls.iter().map(|&(_, len)| len).sum();
    assert_eq!(total, header.payload_len());
    let mut offset = HEADER_LEN;
    for &(call_offset, len) in payload_calls {
        assert_eq!(call_offset, offset);
        offset += len;
    }
    assert_eq!(offset, HEADER_LEN + header.payload_len());
}

#[test]
fn read_row_issues_exactly_five_plane_reads() {
    let file = pack_bytes();
    let source = Counting::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&source, &mut crc_buf).expect("valid pack");
    source.calls.borrow_mut().clear();

    let mut scratch = RowScratch::new();
    validated
        .read_row(&source, 1, &mut scratch)
        .expect("in-band row");
    let ranges = validated.header().row_ranges(1).expect("in-band row");
    assert_eq!(
        *source.calls.borrow(),
        [
            (ranges.rock.offset, ranges.rock.len),
            (ranges.snow.offset, ranges.snow.len),
            (ranges.barrier.offset, ranges.barrier.len),
            (ranges.eligible.offset, ranges.eligible.len),
            (ranges.noise.offset, ranges.noise.len),
        ]
    );
}

fn check_composer_equivalence<S: AssetSource<Error = AccessError>>(
    source: &S,
    validated: &ValidatedPack,
    legacy: &pack::Pack<'_>,
    cut: Cut,
    band: u8,
) {
    let mut scratch = RowScratch::new();
    for row in 0..ROWS {
        validated
            .read_row(source, row, &mut scratch)
            .expect("in-band row");
        let legacy_inputs = RowInputs {
            rock: legacy.rock_row(row).expect("row"),
            snow: legacy.snow_row(row).expect("row"),
            barrier: legacy.barrier_row(row).expect("row"),
            eligible: legacy.eligible_row(row).expect("row"),
            noise: legacy.noise_row(row).expect("row"),
        };
        let mut via_scratch = [0u8; WIDTH / 8];
        let mut via_legacy = [0u8; WIDTH / 8];
        compose_row(&scratch.inputs(), cut, band, &mut via_scratch).expect("lengths agree");
        compose_row(&legacy_inputs, cut, band, &mut via_legacy).expect("lengths agree");
        assert_eq!(
            via_scratch, via_legacy,
            "composer row {row} cut {cut:?} band {band}"
        );
    }
}

fn fragmented_for_composer(file: &[u8]) -> (Vec<&[u8]>, usize) {
    // Deterministic cuts inside the header, across the seam, and inside
    // payload planes so row reads cross span boundaries.
    let mid = HEADER_LEN + 13;
    let end = HEADER_LEN + 2000;
    let _ = end;
    (
        vec![
            &file[..mid],
            &file[mid..end.min(file.len())],
            &file[end.min(file.len())..],
        ],
        file.len(),
    )
}

#[test]
fn composer_output_through_row_scratch_matches_legacy_slices() {
    let file = pack_bytes();
    let slice = SliceSource::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&slice, &mut crc_buf).expect("valid pack");
    let legacy = pack::parse(&file).expect("valid pack");

    // Mixed cut exercising below/at/above barrier plus intra-level mixing,
    // with a nonzero blend band.
    let mixed = Cut {
        level: 100,
        frac: 200,
    };
    // Hard cut at the frac boundary: no blend band.
    let hard = Cut {
        level: 100,
        frac: 0,
    };
    check_composer_equivalence(&slice, &validated, &legacy, mixed, 24);
    check_composer_equivalence(&slice, &validated, &legacy, hard, 0);

    let (spans, _) = fragmented_for_composer(&file);
    let fragmented = FragmentedSource::new(&spans).expect("span totals");
    check_composer_equivalence(&fragmented, &validated, &legacy, mixed, 24);
    check_composer_equivalence(&fragmented, &validated, &legacy, hard, 0);

    // Sanity that the comparison is non-degenerate: mixed ink and paper.
    let mut scratch = RowScratch::new();
    validated
        .read_row(&slice, 0, &mut scratch)
        .expect("in-band row");
    let mut first_out = [0u8; WIDTH / 8];
    compose_row(&scratch.inputs(), mixed, 24, &mut first_out).expect("lengths agree");
    assert!(first_out.iter().any(|&b| b != 0));
    assert!(first_out.iter().any(|&b| b != 0xFF));
    // Rows differ, so the loop is not comparing one row three times.
    let mut last_out = [0u8; WIDTH / 8];
    validated
        .read_row(&slice, ROWS - 1, &mut scratch)
        .expect("last row");
    compose_row(&scratch.inputs(), mixed, 24, &mut last_out).expect("lengths agree");
    assert_ne!(first_out, last_out);
}

#[test]
fn bad_crc_is_rejected_without_publishing_metadata() {
    let mut file = pack_bytes();
    file[HEADER_LEN + 5] ^= 0xFF;
    let source = SliceSource::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    assert_eq!(
        validate_source(&source, &mut crc_buf),
        Err(ValidationError::Invalid(PackError::BadCrc))
    );
}

#[test]
fn truncated_and_overlong_sources_report_exact_lengths() {
    let file = pack_bytes();
    let mut crc_buf = vec![0u8; CRC_SCRATCH];

    let short = &file[..file.len() - 10];
    assert_eq!(
        validate_source(&SliceSource::new(short), &mut crc_buf),
        Err(ValidationError::LengthMismatch {
            actual: file.len() - 10,
            expected: file.len(),
        })
    );

    let mut long = file.clone();
    long.push(0x00);
    assert_eq!(
        validate_source(&SliceSource::new(&long), &mut crc_buf),
        Err(ValidationError::LengthMismatch {
            actual: file.len() + 1,
            expected: file.len(),
        })
    );

    // Below the header: the header read itself fails as an access error.
    let empty: &[u8] = &[];
    assert_eq!(
        validate_source(&SliceSource::new(empty), &mut crc_buf),
        Err(ValidationError::Access(AccessError::OutOfBounds {
            offset: 0,
            len: HEADER_LEN,
            capacity: 0,
        }))
    );
}

#[test]
fn empty_crc_scratch_fails_before_touching_the_source() {
    let file = pack_bytes();
    let source = Counting::new(&file);
    let empty: &mut [u8] = &mut [];
    assert_eq!(
        validate_source(&source, empty),
        Err(ValidationError::EmptyScratch)
    );
    assert!(source.calls.borrow().is_empty());
}

fn sentinel_scratch() -> RowScratch {
    let mut scratch = RowScratch::new();
    scratch.rock.fill(0xA5);
    scratch.snow.fill(0xA5);
    scratch.barrier.fill(0xA5);
    scratch.eligible.fill(0xA5);
    scratch.noise.fill(0xA5);
    scratch
}

#[test]
fn bad_rows_fail_without_touching_scratch() {
    let file = pack_bytes();
    let source = SliceSource::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated: ValidatedPack = validate_source(&source, &mut crc_buf).expect("valid pack");

    let mut scratch = sentinel_scratch();
    assert_eq!(
        validated.read_row(&source, ROWS, &mut scratch),
        Err(ValidationError::Invalid(PackError::BadRow))
    );
    assert_eq!(scratch, sentinel_scratch());
    assert_eq!(
        validated.read_row(&source, usize::MAX, &mut scratch),
        Err(ValidationError::Invalid(PackError::BadRow))
    );
    assert_eq!(scratch, sentinel_scratch());
}

#[test]
fn changed_source_length_after_validation_is_not_used() {
    let file = pack_bytes();
    let source = SliceSource::new(&file);
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&source, &mut crc_buf).expect("valid pack");

    // The same metadata against a source that shrank afterwards.
    let shrunk = SliceSource::new(&file[..file.len() - 10]);
    let mut scratch = sentinel_scratch();
    assert_eq!(
        validated.read_row(&shrunk, 0, &mut scratch),
        Err(ValidationError::LengthMismatch {
            actual: file.len() - 10,
            expected: file.len(),
        })
    );
    assert_eq!(scratch, sentinel_scratch());

    // The same metadata against a source that grew afterwards.
    let mut grown = file.clone();
    grown.extend_from_slice(&[0u8; 16]);
    let grown_source = SliceSource::new(&grown);
    let mut scratch = sentinel_scratch();
    assert_eq!(
        validated.read_row(&grown_source, 0, &mut scratch),
        Err(ValidationError::LengthMismatch {
            actual: file.len() + 16,
            expected: file.len(),
        })
    );
    assert_eq!(scratch, sentinel_scratch());
}

/// Deterministic mid-row failure: fails on one plane offset without touching
/// the destination, so earlier planes stay written and later planes stay at
/// their prior bytes.
struct FailMidRow<'a> {
    inner: SliceSource<'a>,
    fail_offset: usize,
}

impl AssetSource for FailMidRow<'_> {
    type Error = AccessError;

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        if offset == self.fail_offset {
            return Err(AccessError::OutOfBounds {
                offset,
                len: dst.len(),
                capacity: self.inner.len(),
            });
        }
        self.inner.read_exact_at(offset, dst)
    }

    fn borrow_at(&self, offset: usize, len: usize) -> Result<Option<&[u8]>, AccessError> {
        self.inner.borrow_at(offset, len)
    }
}

#[test]
fn access_failure_may_leave_earlier_planes_written() {
    let file = pack_bytes();
    let mut crc_buf = vec![0u8; CRC_SCRATCH];
    let validated = validate_source(&SliceSource::new(&file), &mut crc_buf).expect("valid pack");
    let legacy = pack::parse(&file).expect("valid pack");
    let ranges = validated.header().row_ranges(1).expect("in-band row");
    let failing = FailMidRow {
        inner: SliceSource::new(&file),
        fail_offset: ranges.barrier.offset,
    };

    let mut scratch = sentinel_scratch();
    let result = validated.read_row(&failing, 1, &mut scratch);
    assert!(matches!(result, Err(ValidationError::Access(_))));
    // Planes before the failure are already written; planes at/after it are
    // untouched, so the scratch must be discarded, never composed, after Err.
    assert_eq!(scratch.rock.as_slice(), legacy.rock_row(1).expect("row"));
    assert_eq!(scratch.snow.as_slice(), legacy.snow_row(1).expect("row"));
    assert!(scratch.barrier.iter().all(|&b| b == 0xA5));
    assert!(scratch.eligible.iter().all(|&b| b == 0xA5));
    assert!(scratch.noise.iter().all(|&b| b == 0xA5));
    assert_ne!(scratch, sentinel_scratch());
}
