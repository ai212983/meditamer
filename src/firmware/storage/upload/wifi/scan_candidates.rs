use super::*;

pub(super) fn collect_scan_results(
    label: &str,
    target_ssid: &str,
    results: &[AccessPointInfo],
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
) {
    if results.is_empty() {
        telemetry::record_wifi_scan(0, false);
        diag_reassoc!(
            "upload_http: scan {} found=0 target_ssid={}",
            label,
            target_ssid
        );
        return;
    }

    diag_reassoc!(
        "upload_http: scan {} found={} target_ssid={}",
        label,
        results.len(),
        target_ssid
    );

    for ap in results.iter() {
        diag_reassoc!(
            "upload_http: scan ap ssid={} channel={} bssid={} rssi={} auth={:?}",
            ap.ssid.as_str(),
            ap.channel,
            format_bssid(ap.bssid),
            ap.signal_strength,
            ap.auth_method
        );
        if ap.ssid.as_str() == target_ssid {
            insert_or_update_candidate(
                candidates,
                TargetApCandidate {
                    hint: TargetApHint {
                        channel: ap.channel,
                        bssid: ap.bssid,
                    },
                    rssi: ap.signal_strength,
                },
            );
        }
    }

    if let Some(ap) = candidates.first() {
        diag_reassoc!(
            "upload_http: scan target_ssid={} found_channel={} found_bssid={} via={} candidates={}",
            target_ssid,
            ap.hint.channel,
            format_bssid(ap.hint.bssid),
            label,
            candidates.len(),
        );
    }
    telemetry::record_wifi_scan(results.len(), !candidates.is_empty());
}

fn insert_or_update_candidate(
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    candidate: TargetApCandidate,
) {
    if let Some(existing_idx) = candidates
        .iter()
        .position(|item| item.hint.bssid == candidate.hint.bssid)
    {
        if candidate.rssi > candidates[existing_idx].rssi {
            candidates[existing_idx] = candidate;
            sort_candidates_by_signal(candidates);
        }
        return;
    }
    if candidates.len() < WIFI_AP_CANDIDATE_MAX {
        let _ = candidates.push(candidate);
        sort_candidates_by_signal(candidates);
        return;
    }
    if let Some((weakest_idx, weakest)) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, item)| item.rssi)
    {
        if candidate.rssi > weakest.rssi {
            candidates[weakest_idx] = candidate;
            sort_candidates_by_signal(candidates);
        }
    }
}

fn sort_candidates_by_signal(
    candidates: &mut heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
) {
    if candidates.len() < 2 {
        return;
    }
    let mut i = 1usize;
    while i < candidates.len() {
        let mut j = i;
        while j > 0 && candidates[j].rssi > candidates[j - 1].rssi {
            candidates.swap(j, j - 1);
            j -= 1;
        }
        i += 1;
    }
}

pub(super) fn rotate_to_next_candidate(
    candidates: &heapless::Vec<TargetApCandidate, WIFI_AP_CANDIDATE_MAX>,
    current_bssid: Option<[u8; 6]>,
    candidate_idx: &mut usize,
) -> Option<TargetApCandidate> {
    if candidates.is_empty() {
        return None;
    }
    if let Some(current_bssid) = current_bssid {
        if let Some(position) = candidates
            .iter()
            .position(|candidate| candidate.hint.bssid == current_bssid)
        {
            *candidate_idx = position;
        }
    } else {
        *candidate_idx = 0;
        return candidates.get(*candidate_idx).copied();
    }
    if candidates.len() > 1 {
        *candidate_idx = (*candidate_idx + 1) % candidates.len();
    } else {
        *candidate_idx = 0;
    }
    candidates.get(*candidate_idx).copied()
}
