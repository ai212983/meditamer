use super::*;

use super::scan_candidates::collect_scan_results;

pub(super) async fn scan_target_candidates(
    controller: &mut WifiController<'static>,
    target_ssid: &str,
    runtime_policy: WifiRuntimePolicy,
    force_full_channel_probe: bool,
) -> ScanOutcome {
    let mut candidates = heapless::Vec::<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>::new();
    let probe_timeout_ms = zero_discovery_probe_timeout_ms(runtime_policy);
    let probe_timeout = Duration::from_millis(probe_timeout_ms);
    let mut any_nonzero_results = false;
    let mut saw_target_candidate = false;
    let mut scan_context = ScanStageContext {
        controller,
        runtime_policy,
        target_ssid,
        candidates: &mut candidates,
        saw_nonzero_results: &mut any_nonzero_results,
        saw_target_candidate: &mut saw_target_candidate,
    };

    for stage in [
        ScanStage::ActiveBroad,
        ScanStage::ActiveDirected,
        ScanStage::Passive,
    ] {
        let timeout_ms = stage.scan_timeout_ms(runtime_policy);
        let timeout = Duration::from_millis(timeout_ms);
        if let Some(outcome) = run_scan_stage(stage, &mut scan_context, timeout).await {
            return outcome;
        }
    }

    // In AP-dense environments, unrelated SSIDs can keep scan counts non-zero while
    // the target SSID is still absent. Probe fallback should key on target visibility.
    if !*scan_context.saw_target_candidate {
        let probe_channels: &[u8] = if force_full_channel_probe {
            &WIFI_CHANNEL_PROBE_SEQUENCE
        } else {
            &WIFI_ZERO_DISCOVERY_SCAN_PROBE_CHANNELS
        };
        diag_reassoc!(
            "upload_http: scan zero_result_fallback start channels={:?} full_channel_probe={} target_ssid={} probe_timeout_ms={}",
            probe_channels,
            force_full_channel_probe,
            target_ssid,
            probe_timeout_ms,
        );
        for channel in probe_channels.iter().copied() {
            if let Some(outcome) =
                run_scan_stage(ScanStage::Probe(channel), &mut scan_context, probe_timeout).await
            {
                return outcome;
            }
        }
    }

    let saw_nonzero_results = *scan_context.saw_nonzero_results;
    let saw_target_candidate = *scan_context.saw_target_candidate;
    let candidates = core::mem::take(scan_context.candidates);
    if let Some(outcome) =
        scan_stage_outcome_if_available(target_ssid, &candidates, saw_nonzero_results)
    {
        return outcome;
    }

    diag_reassoc!("upload_http: scan target_ssid={} found=0", target_ssid);
    ScanOutcome {
        candidates,
        hit_nomem: false,
        saw_nonzero_results,
        saw_target_candidate,
    }
}

#[derive(Clone, Copy)]
enum ScanStage {
    ActiveBroad,
    ActiveDirected,
    Passive,
    Probe(u8),
}

impl ScanStage {
    fn scan_timeout_ms(self, runtime_policy: WifiRuntimePolicy) -> u64 {
        match self {
            ScanStage::ActiveBroad => active_scan_timeout_ms(runtime_policy),
            ScanStage::ActiveDirected => directed_scan_timeout_ms(runtime_policy),
            ScanStage::Passive => passive_scan_timeout_ms(runtime_policy),
            ScanStage::Probe(_) => zero_discovery_probe_timeout_ms(runtime_policy),
        }
    }
}

struct ScanStageContext<'a> {
    controller: &'a mut WifiController<'static>,
    runtime_policy: WifiRuntimePolicy,
    target_ssid: &'a str,
    candidates: &'a mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    saw_nonzero_results: &'a mut bool,
    saw_target_candidate: &'a mut bool,
}

async fn run_scan_stage(
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
            telemetry::WifiScanPhase::Active,
            driver::active_scan_config(context.runtime_policy).with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::ActiveDirected => (
            "active_directed",
            telemetry::WifiScanPhase::Active,
            driver::directed_active_scan_config(context.target_ssid, context.runtime_policy)
                .with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::Passive => (
            "passive",
            telemetry::WifiScanPhase::Passive,
            driver::passive_scan_config(context.runtime_policy).with_max(WIFI_SCAN_DIAG_MAX_APS),
        ),
        ScanStage::Probe(channel) => (
            "probe",
            telemetry::WifiScanPhase::Active,
            driver::channel_active_scan_config(channel, context.runtime_policy)
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
        wifi_scan_with_config_async(context.controller, config),
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
            telemetry::record_wifi_reassoc_scan(
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
            telemetry::record_wifi_reassoc_scan(phase, 0, false, elapsed_ms, None);
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
            telemetry::record_wifi_reassoc_scan(phase, 0, false, elapsed_ms, None);
        }
    }
    None
}

fn scan_stage_outcome_if_available(
    target_ssid: &str,
    candidates: &heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    saw_nonzero_results: bool,
) -> Option<ScanOutcome> {
    if candidates.is_empty() {
        return None;
    }
    diag_reassoc!(
        "upload_http: scan target_ssid={} candidate_count={} top_channel={} top_bssid={}",
        target_ssid,
        candidates.len(),
        candidates.first().map(|ap| ap.hint.channel).unwrap_or(0),
        format_bssid_opt(candidates.first().map(|ap| ap.hint.bssid)),
    );
    Some(ScanOutcome {
        candidates: candidates.clone(),
        hit_nomem: false,
        saw_nonzero_results,
        saw_target_candidate: true,
    })
}
