use super::*;
fn no_move_release_recontact_continues_same_swipe() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Press stabilizes.
    drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
    // Brief dropout starts release candidate.
    drain_kinds(engine.tick(40, sample0()), &mut events);
    // Re-contact shortly after debounce window, now far enough from down to
    // indicate same swipe continuation rather than a new tap.
    drain_kinds(engine.tick(76, sample1(170, 102)), &mut events);
    drain_kinds(engine.tick(92, sample1(230, 103)), &mut events);
    drain_kinds(engine.tick(120, sample0()), &mut events);
    drain_kinds(engine.tick(180, sample0()), &mut events);

    assert_eq!(
        events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn no_move_release_large_late_recontact_still_continues_swipe() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Stabilize press, then drop to zero before any drag sample is observed.
    drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(36, sample0()), &mut events);
    // Re-contact arrives after debounce window with a long rightward jump.
    drain_kinds(engine.tick(160, sample1(430, 104)), &mut events);
    drain_kinds(engine.tick(176, sample1(520, 106)), &mut events);
    drain_kinds(engine.tick(208, sample0()), &mut events);
    drain_kinds(engine.tick(320, sample0()), &mut events);

    assert_eq!(
        events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn no_move_release_recontact_beyond_hard_max_starts_new_interaction() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(36, sample0()), &mut events);
    // Too far from original down point to be treated as same interaction.
    drain_kinds(engine.tick(160, sample1(760, 100)), &mut events);
    drain_kinds(engine.tick(192, sample1(760, 100)), &mut events);

    let down_count = events
        .iter()
        .filter(|k| matches!(k, TouchEventKind::Down))
        .count();
    assert!(down_count >= 2);
}

#[test]
fn tiny_jitter_no_move_release_uses_extended_debounce() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Stabilize press with tiny jitter that should still count as "no motion".
    let _ = engine.tick(0, sample1(200, 220));
    for event in engine
        .tick(16, sample1(201, 220))
        .events
        .into_iter()
        .flatten()
    {
        events.push(event);
    }
    // Enter release debounce; no re-contact.
    for event in engine.tick(24, sample0()).events.into_iter().flatten() {
        events.push(event);
    }
    // Should not finalize yet because early no-move debounce should be extended.
    for event in engine.tick(120, sample0()).events.into_iter().flatten() {
        events.push(event);
    }
    assert!(!events
        .iter()
        .any(|ev| matches!(ev.kind, TouchEventKind::Up)));

    // Past extended window, release must finalize with the extended debounce value.
    for event in engine.tick(144, sample0()).events.into_iter().flatten() {
        events.push(event);
    }
    let up = events
        .iter()
        .find(|ev| matches!(ev.kind, TouchEventKind::Up))
        .expect("missing up event");
    assert_eq!(
        up.release_debounce_ms,
        TOUCH_DEBOUNCE_UP_NO_MOVE_EARLY_MS as u16
    );
}

#[test]
fn sparse_start_reports_still_recover_into_single_swipe() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Touch starts, then panel emits zeros before next coordinate update.
    drain_kinds(engine.tick(0, sample1(100, 120)), &mut events);
    drain_kinds(engine.tick(16, sample1(100, 120)), &mut events);
    drain_kinds(engine.tick(32, sample0()), &mut events);
    drain_kinds(engine.tick(64, sample0()), &mut events);
    drain_kinds(engine.tick(96, sample0()), &mut events);
    // Sparse re-contact still belongs to same physical gesture.
    drain_kinds(engine.tick(128, sample1(185, 122)), &mut events);
    drain_kinds(engine.tick(160, sample1(245, 123)), &mut events);
    drain_kinds(engine.tick(176, sample0()), &mut events);
    drain_kinds(engine.tick(320, sample0()), &mut events);

    assert_eq!(
        events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn slow_drag_still_emits_swipe_when_travel_is_clear() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(80, 160)), &mut events);
    drain_kinds(engine.tick(20, sample1(80, 160)), &mut events);
    // Slow but clear rightward drag lasting longer than swipe max duration.
    drain_kinds(engine.tick(700, sample1(170, 162)), &mut events);
    drain_kinds(engine.tick(1_220, sample1(245, 164)), &mut events);
    drain_kinds(engine.tick(1_260, sample0()), &mut events);
    drain_kinds(engine.tick(1_360, sample0()), &mut events);

    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn post_swipe_retouch_near_release_is_suppressed() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Complete a normal swipe.
    drain_kinds(engine.tick(0, sample1(120, 220)), &mut events);
    drain_kinds(engine.tick(16, sample1(120, 220)), &mut events);
    drain_kinds(engine.tick(64, sample1(220, 222)), &mut events);
    drain_kinds(engine.tick(96, sample0()), &mut events);
    drain_kinds(engine.tick(136, sample0()), &mut events);

    let down_before = events
        .iter()
        .filter(|k| matches!(k, TouchEventKind::Down))
        .count();

    // Controller reports a short follow-up contact near release point.
    drain_kinds(engine.tick(176, sample1(223, 223)), &mut events);
    drain_kinds(engine.tick(200, sample0()), &mut events);
    drain_kinds(engine.tick(240, sample0()), &mut events);

    let down_after = events
        .iter()
        .filter(|k| matches!(k, TouchEventKind::Down))
        .count();

    assert_eq!(down_before, down_after);
    assert!(!events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn post_swipe_new_touch_far_from_release_starts_new_interaction() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(80, 180)), &mut events);
    drain_kinds(engine.tick(16, sample1(80, 180)), &mut events);
    drain_kinds(engine.tick(64, sample1(180, 182)), &mut events);
    drain_kinds(engine.tick(96, sample0()), &mut events);
    drain_kinds(engine.tick(136, sample0()), &mut events);

    // New touch far away should not be suppressed by post-swipe guard.
    drain_kinds(engine.tick(176, sample1(300, 300)), &mut events);
    drain_kinds(engine.tick(208, sample1(300, 300)), &mut events);

    let down_count = events
        .iter()
        .filter(|k| matches!(k, TouchEventKind::Down))
        .count();
    assert!(down_count >= 2);
}
