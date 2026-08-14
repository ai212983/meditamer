#[path = "../../../src/firmware/storage/upload/sd_bridge/correlation.rs"]
mod correlation;

#[test]
fn late_timed_out_result_cannot_match_the_next_request() {
    let timed_out_request_id = 41;
    let next_request_id = correlation::next_request_id(timed_out_request_id);

    assert!(!correlation::result_matches_request(
        next_request_id,
        timed_out_request_id
    ));
    assert!(correlation::result_matches_request(
        next_request_id,
        next_request_id
    ));
}

#[test]
fn request_ids_skip_zero_when_the_counter_wraps() {
    assert_eq!(correlation::next_request_id(u32::MAX), 1);
}
