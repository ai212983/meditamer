use super::*;

use super::collect_scan_results;

#[derive(Clone, Copy)]
pub(super) enum ScanStage {
    ActiveBroad,
    ActiveDirected,
    Passive,
    Probe(u8),
}

impl ScanStage {
    pub(super) fn scan_timeout_ms(self, runtime_policy: WifiRuntimePolicy) -> u64 {
        match self {
            ScanStage::ActiveBroad => active_scan_timeout_ms(runtime_policy),
            ScanStage::ActiveDirected => directed_scan_timeout_ms(runtime_policy),
            ScanStage::Passive => passive_scan_timeout_ms(runtime_policy),
            ScanStage::Probe(_) => zero_discovery_probe_timeout_ms(runtime_policy),
        }
    }
}

pub(super) struct ScanStageContext<'a> {
    pub(super) controller: &'a mut WifiController<'static>,
    pub(super) runtime_policy: WifiRuntimePolicy,
    pub(super) target_ssid: &'a str,
    pub(super) candidates: &'a mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    pub(super) saw_nonzero_results: &'a mut bool,
    pub(super) saw_target_candidate: &'a mut bool,
}

pub(super) async fn run_scan_stage(
    stage: ScanStage,
    context: &mut ScanStageContext<'_>,
    timeout: Duration,
) -> Option<ScanOutcome> {
    let timeout_ms = timeout.as_millis();
    let probe_channel = match stage {
        ScanStage::Probe(channel) => i16::from(channel),
        _ => -1,
    };
    let (label, phase, config) = match stage {
        ScanStage::ActiveBroad => (
            "active_broad",
            observability::WifiScanPhase::Active,
            legacy_discovery::active_scan_config(context.runtime_policy)
                .with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::ActiveDirected => (
            "active_directed",
            observability::WifiScanPhase::Active,
            legacy_discovery::directed_active_scan_config(
                context.target_ssid,
                context.runtime_policy,
            )
            .with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::Passive => (
            "passive",
            observability::WifiScanPhase::Passive,
            legacy_discovery::passive_scan_config(context.runtime_policy)
                .with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::Probe(channel) => (
            "probe",
            observability::WifiScanPhase::Active,
            legacy_discovery::channel_active_scan_config(channel, context.runtime_policy)
                .with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
    };
    diag_reassoc!(
        "upload_http: scan_stage begin label={} probe_channel={} timeout_ms={} target_ssid={} active_min_ms={} active_max_ms={} passive_ms={} candidate_count_before={} saw_nonzero_before={} saw_target_before={}",
        label,
        probe_channel,
        timeout_ms,
        context.target_ssid,
        context.runtime_policy.scan_active_min_ms,
        context.runtime_policy.scan_active_max_ms,
        context.runtime_policy.scan_passive_ms,
        context.candidates.len(),
        *context.saw_nonzero_results,
        *context.saw_target_candidate,
    );
    log_radio_mem_diag(match stage {
        ScanStage::ActiveBroad => "scan_active_broad_before",
        ScanStage::ActiveDirected => "scan_active_directed_before",
        ScanStage::Passive => "scan_passive_before",
        ScanStage::Probe(_) => "scan_probe_before",
    });
    let started_at = Instant::now();
    match with_timeout(
        timeout,
        legacy_discovery::scan_with_controller(context.controller, config),
    )
    .await
    {
        Ok(Ok(results)) => {
            log_radio_mem_diag(match stage {
                ScanStage::ActiveBroad => "scan_active_broad_ok",
                ScanStage::ActiveDirected => "scan_active_directed_ok",
                ScanStage::Passive => "scan_passive_ok",
                ScanStage::Probe(_) => "scan_probe_ok",
            });
            *context.saw_nonzero_results |= !results.is_empty();
            if let ScanStage::Probe(channel) = stage {
                diag_reassoc!(
                    "upload_http: scan probe channel={} found={} target_ssid={}",
                    channel,
                    results.len(),
                    context.target_ssid
                );
            } else {
                diag_reassoc!(
                    "upload_http: scan {} found={} target_ssid={}",
                    label,
                    results.len(),
                    context.target_ssid
                );
            }
            collect_scan_results(label, context.target_ssid, &results, context.candidates);
            *context.saw_target_candidate |= !context.candidates.is_empty();
            let elapsed_ms = elapsed_ms_u32(started_at);
            diag_reassoc!(
                "upload_http: scan_stage end label={} probe_channel={} outcome=ok elapsed_ms={} result_count={} candidate_count_after={} saw_nonzero_after={} saw_target_after={}",
                label,
                probe_channel,
                elapsed_ms,
                results.len(),
                context.candidates.len(),
                *context.saw_nonzero_results,
                *context.saw_target_candidate,
            );
            observability::record_wifi_reassoc_scan(
                phase,
                results.len(),
                !context.candidates.is_empty(),
                elapsed_ms,
                context.candidates.first().map(|ap| ap.hint.channel),
            );
            if !context.candidates.is_empty() {
                return Some(ScanOutcome {
                    candidates: context.candidates.clone(),
                    hit_nomem: false,
                    saw_nonzero_results: *context.saw_nonzero_results,
                    saw_target_candidate: *context.saw_target_candidate,
                });
            }
        }
        Ok(Err(err)) => {
            let elapsed_ms = elapsed_ms_u32(started_at);
            diag_reassoc!(
                "upload_http: scan {} err={:?} target_ssid={}",
                label,
                err,
                context.target_ssid
            );
            diag_reassoc!(
                "upload_http: scan_stage end label={} probe_channel={} outcome=err elapsed_ms={} candidate_count_after={} saw_nonzero_after={} saw_target_after={} err={:?}",
                label,
                probe_channel,
                elapsed_ms,
                context.candidates.len(),
                *context.saw_nonzero_results,
                *context.saw_target_candidate,
                err,
            );
            if is_no_mem_wifi_error(&err) {
                diag_reassoc!(
                    "upload_http: scan {} NoMem target_ssid={}",
                    label,
                    context.target_ssid
                );
                log_radio_mem_diag(match stage {
                    ScanStage::ActiveBroad => "scan_active_broad_nomem",
                    ScanStage::ActiveDirected => "scan_active_directed_nomem",
                    ScanStage::Passive => "scan_passive_nomem",
                    ScanStage::Probe(_) => "scan_probe_nomem",
                });
                return Some(ScanOutcome {
                    candidates: context.candidates.clone(),
                    hit_nomem: true,
                    saw_nonzero_results: *context.saw_nonzero_results,
                    saw_target_candidate: *context.saw_target_candidate,
                });
            }
            observability::record_wifi_reassoc_scan(phase, 0, false, elapsed_ms, None);
        }
        Err(_) => {
            let elapsed_ms = elapsed_ms_u32(started_at);
            diag_reassoc!(
                "upload_http: scan {} timeout={}ms target_ssid={}",
                label,
                timeout_ms,
                context.target_ssid
            );
            diag_reassoc!(
                "upload_http: scan_stage end label={} probe_channel={} outcome=timeout elapsed_ms={} timeout_ms={} candidate_count_after={} saw_nonzero_after={} saw_target_after={}",
                label,
                probe_channel,
                elapsed_ms,
                timeout_ms,
                context.candidates.len(),
                *context.saw_nonzero_results,
                *context.saw_target_candidate,
            );
            observability::record_wifi_reassoc_scan(phase, 0, false, elapsed_ms, None);
        }
    }
    None
}
