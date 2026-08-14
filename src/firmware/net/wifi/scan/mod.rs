use super::*;

use super::scan_candidates::collect_scan_results;

mod stage;

use stage::{run_scan_stage, ScanStage, ScanStageContext};

pub(super) async fn scan_target_candidates(
    controller: &mut WifiController<'_>,
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
