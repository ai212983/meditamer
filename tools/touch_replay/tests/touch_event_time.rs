#[path = "../../../products/meditamer/src/firmware/touch/event_time.rs"]
mod event_time;
use event_time::EventTime;

#[test]
fn timestamp_preserves_full_range_and_word_boundary() {
    for value in [0, 1, u32::MAX as u64, (u32::MAX as u64) + 1, u64::MAX] {
        assert_eq!(EventTime::new(value).milliseconds(), value);
    }
}

#[test]
fn timestamp_does_not_impose_eight_byte_queue_alignment() {
    assert_eq!(core::mem::size_of::<EventTime>(), 8);
    assert_eq!(core::mem::align_of::<EventTime>(), 4);
}
