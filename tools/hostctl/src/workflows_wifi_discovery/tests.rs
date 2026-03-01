use crate::workflows_wifi_common::{parse_mem_diag_line, parse_scan_done_count, MemDiagKind};

#[test]
fn scan_done_parser_extracts_count() {
    assert_eq!(
        parse_scan_done_count("upload_http: event scan_done status=0 count=2 scan_id=42"),
        Some(2)
    );
    assert_eq!(
        parse_scan_done_count("upload_http: event scan_done status=0 count=0 scan_id=42"),
        Some(0)
    );
    assert_eq!(
        parse_scan_done_count("NET_STATUS {\"state\":\"Ready\"}"),
        None
    );
}

#[test]
fn mem_diag_parser_extracts_radio_sample() {
    let line = "upload_http: radio_mem stage=scan_active_before trigger=none feature=true state=Initialized total=4259840 used=110160 free=4149680 peak=110160 internal_free=59280 external_free=4090400 min_free=4149680 min_internal_free=59280 min_external_free=4090400";
    let sample = parse_mem_diag_line(line).expect("radio sample parses");
    assert_eq!(sample.kind, MemDiagKind::Radio);
    assert_eq!(sample.stage, "scan_active_before");
    assert_eq!(sample.internal_free, 59280);
    assert_eq!(sample.min_internal_free, 59280);
}
