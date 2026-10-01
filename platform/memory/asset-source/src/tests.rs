//! Unit tests for the checked access core. A total-length overflow cannot be
//! constructed from real slices (it would need spans summing past
//! `usize::MAX`), so that path is exercised through `checked_end` directly;
//! [`FragmentedSource::new`] accumulates with that same helper.

use super::*;

#[test]
fn slice_read_and_borrow() {
    let bytes = [10u8, 20, 30, 40, 50];
    let src = SliceSource::new(&bytes);
    assert_eq!(src.len(), 5);
    let mut dst = [0u8; 3];
    src.read_exact_at(1, &mut dst).unwrap();
    assert_eq!(dst, [20, 30, 40]);
    assert_eq!(src.borrow_at(1, 3).unwrap(), Some(&bytes[1..4]));
}

#[test]
fn slice_empty_range_at_capacity() {
    let bytes = [1u8, 2];
    let src = SliceSource::new(&bytes);
    let mut dst = [0u8; 0];
    src.read_exact_at(2, &mut dst).unwrap();
    assert_eq!(src.borrow_at(2, 0).unwrap(), Some(&[][..]));
}

#[test]
fn slice_out_of_bounds() {
    let bytes = [1u8, 2, 3];
    let src = SliceSource::new(&bytes);
    let mut dst = [0u8; 2];
    assert_eq!(
        src.read_exact_at(2, &mut dst),
        Err(AccessError::OutOfBounds {
            offset: 2,
            len: 2,
            capacity: 3
        })
    );
    assert_eq!(
        src.borrow_at(3, 1),
        Err(AccessError::OutOfBounds {
            offset: 3,
            len: 1,
            capacity: 3
        })
    );
}

#[test]
fn slice_offset_overflow() {
    let bytes = [1u8, 2, 3];
    let src = SliceSource::new(&bytes);
    let mut dst = [0u8; 1];
    assert_eq!(
        src.read_exact_at(usize::MAX, &mut dst),
        Err(AccessError::Overflow)
    );
    assert_eq!(src.borrow_at(usize::MAX, 1), Err(AccessError::Overflow));
    assert_eq!(
        src.borrow_at(usize::MAX, 0),
        Err(AccessError::OutOfBounds {
            offset: usize::MAX,
            len: 0,
            capacity: 3
        })
    );
}

#[test]
fn fragmented_exact_and_cross_span_read() {
    let a = [1u8, 2, 3];
    let b = [4u8, 5];
    let c = [6u8];
    let spans: [&[u8]; 3] = [&a, &b, &c];
    let src = FragmentedSource::new(&spans).unwrap();
    assert_eq!(src.len(), 6);
    let mut dst = [0u8; 2];
    src.read_exact_at(0, &mut dst).unwrap();
    assert_eq!(dst, [1, 2]);
    let mut dst = [0u8; 4];
    src.read_exact_at(2, &mut dst).unwrap();
    assert_eq!(dst, [3, 4, 5, 6]);
    let mut dst = [0u8; 6];
    src.read_exact_at(0, &mut dst).unwrap();
    assert_eq!(dst, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn fragmented_borrow_single_span_vs_boundary() {
    let a = [1u8, 2, 3];
    let b = [4u8, 5];
    let spans: [&[u8]; 2] = [&a, &b];
    let src = FragmentedSource::new(&spans).unwrap();
    assert_eq!(src.borrow_at(0, 3).unwrap(), Some(&a[..]));
    assert_eq!(src.borrow_at(3, 2).unwrap(), Some(&b[..]));
    assert_eq!(src.borrow_at(2, 2).unwrap(), None);
    assert_eq!(src.borrow_at(1, 4).unwrap(), None);
}

#[test]
fn fragmented_empty_spans_and_empty_ranges() {
    let empty: [u8; 0] = [];
    let a = [1u8, 2];
    let b = [3u8, 4, 5];
    let spans: [&[u8]; 5] = [&empty, &a, &empty, &b, &empty];
    let src = FragmentedSource::new(&spans).unwrap();
    assert_eq!(src.len(), 5);
    let mut dst = [0u8; 5];
    src.read_exact_at(0, &mut dst).unwrap();
    assert_eq!(dst, [1, 2, 3, 4, 5]);
    assert_eq!(src.borrow_at(2, 3).unwrap(), Some(&b[..]));
    assert_eq!(src.borrow_at(1, 2).unwrap(), None);
    assert_eq!(src.borrow_at(0, 0).unwrap(), Some(&[][..]));
    assert_eq!(src.borrow_at(5, 0).unwrap(), Some(&[][..]));
    let mut dst = [0u8; 0];
    src.read_exact_at(5, &mut dst).unwrap();
    let none: [&[u8]; 0] = [];
    let empty_src = FragmentedSource::new(&none).unwrap();
    assert_eq!(empty_src.len(), 0);
    empty_src.read_exact_at(0, &mut dst).unwrap();
    assert_eq!(empty_src.borrow_at(0, 0).unwrap(), Some(&[][..]));
    assert_eq!(
        empty_src.borrow_at(0, 1),
        Err(AccessError::OutOfBounds {
            offset: 0,
            len: 1,
            capacity: 0
        })
    );
}

#[test]
fn fragmented_out_of_bounds_and_overflow() {
    let a = [1u8, 2, 3];
    let spans: [&[u8]; 1] = [&a];
    let src = FragmentedSource::new(&spans).unwrap();
    let mut dst = [0u8; 2];
    assert_eq!(
        src.read_exact_at(2, &mut dst),
        Err(AccessError::OutOfBounds {
            offset: 2,
            len: 2,
            capacity: 3
        })
    );
    assert_eq!(
        src.borrow_at(2, 2),
        Err(AccessError::OutOfBounds {
            offset: 2,
            len: 2,
            capacity: 3
        })
    );
    assert_eq!(
        src.read_exact_at(usize::MAX, &mut dst),
        Err(AccessError::Overflow)
    );
    assert_eq!(src.borrow_at(usize::MAX, 1), Err(AccessError::Overflow));
}

#[test]
fn checked_end_helper_paths() {
    assert_eq!(checked_end(2, 3, 10), Ok(5));
    assert_eq!(checked_end(10, 0, 10), Ok(10));
    assert_eq!(
        checked_end(8, 3, 10),
        Err(AccessError::OutOfBounds {
            offset: 8,
            len: 3,
            capacity: 10
        })
    );
    assert_eq!(
        checked_end(usize::MAX, 1, usize::MAX),
        Err(AccessError::Overflow)
    );
    assert_eq!(checked_end(usize::MAX, 0, usize::MAX), Ok(usize::MAX));

    // Total-length accumulation path used by `FragmentedSource::new`.
    assert_eq!(
        checked_end(usize::MAX - 1, 2, usize::MAX),
        Err(AccessError::Overflow)
    );
}

#[test]
fn is_empty_true_false() {
    let bytes = [1u8, 2, 3];
    let nonempty = SliceSource::new(&bytes);
    assert!(!nonempty.is_empty());
    let empty_bytes: [u8; 0] = [];
    let empty = SliceSource::new(&empty_bytes);
    assert!(empty.is_empty());
    let spans: [&[u8]; 1] = [&bytes];
    let nonempty_frag = FragmentedSource::new(&spans).unwrap();
    assert!(!nonempty_frag.is_empty());
    let no_spans: [&[u8]; 0] = [];
    let empty_frag = FragmentedSource::new(&no_spans).unwrap();
    assert!(empty_frag.is_empty());
}
