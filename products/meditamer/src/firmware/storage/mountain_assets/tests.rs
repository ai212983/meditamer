use super::*;

#[test]
fn queue_carries_percent_and_generation_until_taken() {
    let _guard = QUEUE_LOCK.lock().unwrap();
    reset_loader_for_tests();

    note_committed_upload();
    let generation = upload_generation();
    assert!(request_assets(20).is_ok());
    assert_eq!(request_assets(22), Err(AssetReadError::Busy));
    // The first request is still queued exactly once, with metadata.
    let queued = poll_asset_request().expect("request must be queued");
    assert_eq!(queued.percent, 20);
    assert_eq!(queued.generation, generation);
    assert!(poll_asset_request().is_none());
    // No completion yet: still busy.
    assert!(take_assets().is_none());
    assert_eq!(request_assets(22), Err(AssetReadError::Busy));

    // Simulate the SD worker completing with an error: taking it releases
    // the loader for a bounded retry, echoing id/percent/generation.
    complete_asset_read(MountainReadCompletion {
        id: queued.id,
        percent: 20,
        generation,
        result: Err(AssetReadError::Unavailable),
    });
    let taken = take_assets().expect("completion must be present");
    assert_eq!(taken.id, queued.id);
    assert_eq!(taken.percent, 20);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::Unavailable));
    assert!(request_assets(22).is_ok());
    let queued = poll_asset_request().expect("retry must be queued");
    assert_eq!((queued.percent, queued.generation), (22, generation));
    reset_loader_for_tests();
}

#[test]
fn invalid_percent_rejected_before_outstanding() {
    let _guard = QUEUE_LOCK.lock().unwrap();
    reset_loader_for_tests();

    assert_eq!(request_assets(101), Err(AssetReadError::BadHeader));
    assert_eq!(request_assets(u8::MAX), Err(AssetReadError::BadHeader));
    // The guard was never set: a valid request still queues.
    assert!(request_assets(100).is_ok());
    let queued = poll_asset_request().expect("valid request must queue");
    assert_eq!(queued.percent, 100);
    reset_loader_for_tests();
}

#[test]
fn screen_exit_wakes_owner_or_marks_queued_render_inactive() {
    let _guard = QUEUE_LOCK.lock().unwrap();
    reset_loader_for_tests();

    assert!(request_assets(20).is_ok());
    assert!(resident_wanted());
    release_resident_pack();
    assert!(!resident_wanted());
    // The queue was full, so the pending render itself wakes the owner.
    assert!(matches!(
        ASSET_REQUESTS.try_receive(),
        Ok(MountainCommand::Render(_))
    ));

    reset_loader_for_tests();
    assert!(request_assets(20).is_ok());
    let request = poll_asset_request().expect("render");
    complete_asset_read(MountainReadCompletion {
        id: request.id,
        percent: request.percent,
        generation: request.generation,
        result: Err(AssetReadError::Busy),
    });
    take_assets().expect("completion");
    release_resident_pack();
    assert_eq!(
        ASSET_REQUESTS.try_receive(),
        Ok(MountainCommand::ReleaseResident)
    );
    reset_loader_for_tests();
}

#[test]
fn stale_completion_replaced_and_fresh_delivered() {
    let _guard = QUEUE_LOCK.lock().unwrap();
    reset_loader_for_tests();

    let generation = upload_generation();
    complete_asset_read(MountainReadCompletion {
        id: 7,
        percent: 20,
        generation,
        result: Err(AssetReadError::Unavailable),
    });
    complete_asset_read(MountainReadCompletion {
        id: 8,
        percent: 22,
        generation,
        result: Err(AssetReadError::NotFound),
    });
    let taken = take_assets().expect("fresh completion must be present");
    assert_eq!(taken.id, 8);
    assert_eq!(taken.percent, 22);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));
    assert!(take_assets().is_none());
    assert!(request_assets(22).is_ok());
    reset_loader_for_tests();
}

#[test]
fn upload_generation_captured_per_request() {
    let _guard = QUEUE_LOCK.lock().unwrap();
    reset_loader_for_tests();

    let before = upload_generation();
    assert!(request_assets(10).is_ok());
    let first = poll_asset_request().expect("first request");
    assert_eq!(first.generation, before);
    assert!(take_assets().is_none());
    // A commit while the first render is outstanding does not rewrite
    // the queued request; the next queue captures the new generation.
    note_committed_upload();
    assert_eq!(upload_generation(), before.wrapping_add(1));
    complete_asset_read(MountainReadCompletion {
        id: first.id,
        percent: first.percent,
        generation: first.generation,
        result: Err(AssetReadError::Busy),
    });
    let taken = take_assets().expect("completion");
    assert_eq!(taken.generation, before);
    assert!(!request_is_current(taken.generation, upload_generation()));
    assert!(request_assets(10).is_ok());
    let second = poll_asset_request().expect("second request");
    assert_eq!(second.generation, upload_generation());
    assert!(request_is_current(second.generation, upload_generation()));
    reset_loader_for_tests();
}

#[test]
fn stale_completion_decision_drops_superseded_renders() {
    let current = upload_generation();
    assert!(completion_is_usable(current, 20, current, Some(20)));
    // Committed upload supersedes every queued render.
    assert!(!completion_is_usable(
        current,
        20,
        current.wrapping_add(1),
        Some(20)
    ));
    // A moved policy percent supersedes the landed one.
    assert!(!completion_is_usable(current, 20, current, Some(22)));
    // Nothing desired (unavailable): even a fresh render is unusable.
    assert!(!completion_is_usable(current, 20, current, None));
}

#[test]
fn session_reuse_decision_requires_current_matching_generation() {
    let current = upload_generation();
    assert!(session_reusable(Some(current), current, current));
    // A commit invalidates both the request and the cached session.
    let next = current.wrapping_add(1);
    assert!(!session_reusable(Some(current), current, next));
    assert!(!session_reusable(Some(next), current, next));
    // No session yet, or a session from an older generation: validate.
    assert!(!session_reusable(None, current, current));
    assert!(!session_reusable(
        Some(current.wrapping_sub(1)),
        current,
        current
    ));
}

#[test]
fn coverage_target_rounds_like_the_composer() {
    assert_eq!(coverage_target(0, 0), 0);
    assert_eq!(coverage_target(161_400, 0), 0);
    assert_eq!(coverage_target(161_400, 100), 161_400);
    assert_eq!(coverage_target(161_400, 20), 32_280);
    // Half-pixel rounds up: (3 * 50 + 50) / 100 = 2.
    assert_eq!(coverage_target(3, 50), 2);
    assert_eq!(coverage_target(1, 50), 1);
}

#[test]
fn overlay_layout_matches_the_composer_planes() {
    assert_eq!(OVERLAY_ROW_BYTES, 75);
    assert_eq!(OVERLAY_PLANE_BYTES, 20_175);
    assert_eq!(OVERLAY_LEN, 40_350);
    assert_eq!(OVERLAY_PLANE_BYTES, MOUNTAIN_ROWS * OVERLAY_ROW_BYTES);
}

#[test]
fn asset_path_points_at_the_ambient_subtree() {
    assert_eq!(ASSET_PATH, b"/assets/AMBIENT/MOUNTAIN.BIN");
    let (field, len) = asset_path_field();
    assert_eq!(&field[..len as usize], ASSET_PATH);
}

#[test]
fn percent_change_always_rearms_the_loader() {
    let current = upload_generation();
    // 20 -> 22 replaces the retained overlay and rearms, even when the
    // retained overlay still matches the old percent ...
    assert!(transition_requires_rearm(
        Some(20),
        Some(22),
        Some((20, current)),
        current
    ));
    // ... and when nothing is retained.
    assert!(transition_requires_rearm(Some(20), Some(22), None, current));
    // A steady percent never rearms: the budget is untouched.
    assert!(!transition_requires_rearm(
        Some(20),
        Some(20),
        Some((20, current)),
        current
    ));
    assert!(!transition_requires_rearm(
        Some(20),
        Some(20),
        None,
        current
    ));
}

#[test]
fn unavailable_recovery_preserves_a_matching_overlay() {
    let current = upload_generation();
    // None -> same percent with exactly the current generation retained:
    // keep the overlay, queue nothing.
    assert!(!transition_requires_rearm(
        None,
        Some(20),
        Some((20, current)),
        current
    ));
    // A stale percent, a stale generation, or nothing retained rearms.
    assert!(transition_requires_rearm(
        None,
        Some(20),
        Some((22, current)),
        current
    ));
    assert!(transition_requires_rearm(
        None,
        Some(20),
        Some((20, current.wrapping_add(1))),
        current
    ));
    assert!(transition_requires_rearm(None, Some(20), None, current));
}

#[test]
fn losing_desired_percent_rearms_nothing() {
    let current = upload_generation();
    // Some -> None retains the old overlay and queues nothing.
    assert!(!transition_requires_rearm(
        Some(20),
        None,
        Some((20, current)),
        current
    ));
    assert!(!transition_requires_rearm(Some(20), None, None, current));
    assert!(!transition_requires_rearm(None, None, None, current));
}

#[test]
fn core_mutation_predicate_matches_only_the_mountain_path() {
    use sdcard::request::SdCommand;
    use sdcard::SD_WRITE_MAX;

    fn other_field() -> ([u8; SD_PATH_MAX], u8) {
        let mut path = [0u8; SD_PATH_MAX];
        let other = b"/assets/AMBIENT/SKY.BIN";
        path[..other.len()].copy_from_slice(other);
        (path, other.len() as u8)
    }

    let (mountain_path, mountain_len) = asset_path_field();
    let (other_path, other_len) = other_field();
    let data = [0u8; SD_WRITE_MAX];

    // Every core mutation variant touching the exact pack path
    // invalidates the generation (rename on either side).
    assert!(core_mutation_invalidates_generation(&SdCommand::FatWrite {
        path: mountain_path,
        path_len: mountain_len,
        data,
        data_len: 0,
    }));
    assert!(core_mutation_invalidates_generation(
        &SdCommand::FatAppend {
            path: mountain_path,
            path_len: mountain_len,
            data,
            data_len: 0,
        }
    ));
    assert!(core_mutation_invalidates_generation(
        &SdCommand::FatTruncate {
            path: mountain_path,
            path_len: mountain_len,
            size: 0,
        }
    ));
    assert!(core_mutation_invalidates_generation(
        &SdCommand::FatRemove {
            path: mountain_path,
            path_len: mountain_len,
        }
    ));
    assert!(core_mutation_invalidates_generation(
        &SdCommand::FatRename {
            src_path: mountain_path,
            src_path_len: mountain_len,
            dst_path: other_path,
            dst_path_len: other_len,
        }
    ));
    assert!(core_mutation_invalidates_generation(
        &SdCommand::FatRename {
            src_path: other_path,
            src_path_len: other_len,
            dst_path: mountain_path,
            dst_path_len: mountain_len,
        }
    ));

    // Reads and unrelated paths never invalidate, so the caller bumps
    // the generation only after a successful mountain-path mutation.
    assert!(!core_mutation_invalidates_generation(&SdCommand::FatRead {
        path: mountain_path,
        path_len: mountain_len,
    }));
    assert!(!core_mutation_invalidates_generation(&SdCommand::FatStat {
        path: mountain_path,
        path_len: mountain_len,
    }));
    assert!(!core_mutation_invalidates_generation(&SdCommand::Probe));
    assert!(!core_mutation_invalidates_generation(
        &SdCommand::FatWrite {
            path: other_path,
            path_len: other_len,
            data,
            data_len: 0,
        }
    ));
    assert!(!core_mutation_invalidates_generation(
        &SdCommand::FatRemove {
            path: other_path,
            path_len: other_len,
        }
    ));
    assert!(!core_mutation_invalidates_generation(
        &SdCommand::FatRename {
            src_path: other_path,
            src_path_len: other_len,
            dst_path: other_path,
            dst_path_len: other_len,
        }
    ));
}
