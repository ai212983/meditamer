pub(crate) fn result_matches_request(request_id: u32, result_request_id: u32) -> bool {
    request_id == result_request_id
}

pub(crate) fn next_request_id(current: u32) -> u32 {
    current.wrapping_add(1).max(1)
}
