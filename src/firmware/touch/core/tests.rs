use super::*;

fn sample1(x: u16, y: u16) -> TouchSample {
    TouchSample {
        touch_count: 1,
        points: [TouchPoint { x, y }, TouchPoint::default()],
    }
}

fn sample2(x0: u16, y0: u16, x1: u16, y1: u16) -> TouchSample {
    TouchSample {
        touch_count: 2,
        points: [TouchPoint { x: x0, y: y0 }, TouchPoint { x: x1, y: y1 }],
    }
}

fn sample0() -> TouchSample {
    TouchSample {
        touch_count: 0,
        points: [TouchPoint::default(), TouchPoint::default()],
    }
}

fn drain_kinds(output: TouchEngineOutput, out: &mut std::vec::Vec<TouchEventKind>) {
    for event in output.events.into_iter().flatten() {
        out.push(event.kind);
    }
}

#[test]
fn tap_emits_down_up_tap() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(100, 120)), &mut events);
    drain_kinds(engine.tick(20, sample1(100, 120)), &mut events);
    drain_kinds(engine.tick(35, sample1(101, 120)), &mut events);
    drain_kinds(engine.tick(90, sample1(101, 121)), &mut events);
    drain_kinds(engine.tick(110, sample0()), &mut events);
    drain_kinds(engine.tick(150, sample0()), &mut events);

    assert_eq!(
        events,
        std::vec![
            TouchEventKind::Down,
            TouchEventKind::Up,
            TouchEventKind::Tap
        ]
    );
}

#[test]
fn long_press_emits_without_tap() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(200, 200)), &mut events);
    drain_kinds(engine.tick(35, sample1(200, 200)), &mut events);
    drain_kinds(engine.tick(760, sample1(201, 200)), &mut events);
    drain_kinds(engine.tick(800, sample0()), &mut events);
    drain_kinds(engine.tick(840, sample0()), &mut events);

    assert_eq!(
        events,
        std::vec![
            TouchEventKind::Down,
            TouchEventKind::LongPress,
            TouchEventKind::Up
        ]
    );
}

#[test]
fn swipe_right_emits_move_up_swipe() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(50, 100)), &mut events);
    drain_kinds(engine.tick(35, sample1(50, 100)), &mut events);
    drain_kinds(engine.tick(80, sample1(90, 103)), &mut events);
    drain_kinds(engine.tick(120, sample1(180, 108)), &mut events);
    drain_kinds(engine.tick(150, sample0()), &mut events);
    drain_kinds(engine.tick(190, sample0()), &mut events);
    drain_kinds(engine.tick(230, sample0()), &mut events);

    assert_eq!(events[0], TouchEventKind::Down);
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Move)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn multitouch_cancels_current_interaction() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(120, 120)), &mut events);
    drain_kinds(engine.tick(35, sample1(120, 120)), &mut events);
    drain_kinds(engine.tick(60, sample2(121, 121, 220, 220)), &mut events);
    drain_kinds(engine.tick(100, sample0()), &mut events);
    drain_kinds(engine.tick(160, sample0()), &mut events);

    assert_eq!(events[0], TouchEventKind::Down);
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Cancel)));
    assert!(!events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
}

#[test]
fn brief_bounce_is_ignored_by_down_debounce() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(20, 20)), &mut events);
    drain_kinds(engine.tick(10, sample0()), &mut events);
    drain_kinds(engine.tick(60, sample0()), &mut events);

    assert!(events.is_empty());
}

#[test]
fn down_origin_is_anchored_after_debounce() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // First sample is noisy; stabilized point appears by debounce boundary.
    let _ = engine.tick(0, sample1(40, 40));
    let output = engine.tick(35, sample1(100, 120));
    for event in output.events.into_iter().flatten() {
        events.push(event);
    }
    let _ = engine.tick(80, sample0());
    let output = engine.tick(120, sample0());
    for event in output.events.into_iter().flatten() {
        events.push(event);
    }

    let down = events
        .iter()
        .find(|ev| matches!(ev.kind, TouchEventKind::Down))
        .expect("missing down event");
    assert_eq!(down.start_x, 100);
    assert_eq!(down.start_y, 120);

    let up = events
        .iter()
        .find(|ev| matches!(ev.kind, TouchEventKind::Up))
        .expect("missing up event");
    assert_eq!(up.start_x, 100);
    assert_eq!(up.start_y, 120);

    assert!(events
        .iter()
        .any(|ev| matches!(ev.kind, TouchEventKind::Tap)));
}

#[test]
fn jitter_drag_still_emits_tap_when_release_is_near() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Jitter briefly crosses drag threshold but total release travel remains tap-like.
    drain_kinds(engine.tick(0, sample1(200, 200)), &mut events);
    drain_kinds(engine.tick(35, sample1(200, 200)), &mut events);
    drain_kinds(engine.tick(70, sample1(212, 205)), &mut events);
    drain_kinds(engine.tick(95, sample0()), &mut events);
    drain_kinds(engine.tick(130, sample0()), &mut events);

    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
}

#[test]
fn short_press_release_during_down_debounce_emits_tap() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Press starts, then touch count flickers to zero before a stable second count=1 sample.
    // Engine should still produce a real tap interaction.
    drain_kinds(engine.tick(0, sample1(180, 220)), &mut events);
    drain_kinds(engine.tick(8, sample0()), &mut events);
    drain_kinds(engine.tick(16, sample0()), &mut events);
    drain_kinds(engine.tick(40, sample0()), &mut events);

    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Tap)));
}

#[test]
fn fast_swipe_during_down_debounce_is_still_detected() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Finger moves quickly before debounce-down promotes to Pressed.
    drain_kinds(engine.tick(0, sample1(50, 100)), &mut events);
    drain_kinds(engine.tick(8, sample1(120, 102)), &mut events);
    drain_kinds(engine.tick(16, sample0()), &mut events);
    drain_kinds(engine.tick(40, sample0()), &mut events);

    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn pre_debounce_fast_motion_is_preserved_at_down_promotion() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    // Motion happens before debounce promotion and press remains active at
    // the promotion sample. Engine should preserve early path and classify swipe.
    drain_kinds(engine.tick(0, sample1(70, 220)), &mut events);
    drain_kinds(engine.tick(8, sample1(150, 222)), &mut events);
    drain_kinds(engine.tick(16, sample1(240, 224)), &mut events);
    drain_kinds(engine.tick(24, sample0()), &mut events);
    drain_kinds(engine.tick(80, sample0()), &mut events);

    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Down)));
    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn drag_flicker_does_not_split_swipe_into_two_touches() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(40, 120)), &mut events);
    drain_kinds(engine.tick(16, sample1(40, 120)), &mut events);
    drain_kinds(engine.tick(24, sample1(90, 121)), &mut events);
    // Brief count=0 drop while finger is still moving.
    drain_kinds(engine.tick(32, sample0()), &mut events);
    drain_kinds(engine.tick(40, sample0()), &mut events);
    // Recover touch before drag debounce window expires.
    drain_kinds(engine.tick(48, sample1(165, 123)), &mut events);
    drain_kinds(engine.tick(56, sample0()), &mut events);
    drain_kinds(engine.tick(96, sample0()), &mut events);
    drain_kinds(engine.tick(128, sample0()), &mut events);

    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
    assert_eq!(
        events
            .iter()
            .filter(|k| matches!(k, TouchEventKind::Down))
            .count(),
        1
    );
}

#[test]
fn swipe_detected_even_if_release_returns_near_start() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(60, 120)), &mut events);
    drain_kinds(engine.tick(16, sample1(60, 120)), &mut events);
    drain_kinds(engine.tick(32, sample1(180, 121)), &mut events);
    // Finger jitters back before lift.
    drain_kinds(engine.tick(48, sample1(90, 122)), &mut events);
    drain_kinds(engine.tick(64, sample0()), &mut events);
    drain_kinds(engine.tick(120, sample0()), &mut events);
    drain_kinds(engine.tick(136, sample0()), &mut events);

    assert!(events
        .iter()
        .any(|k| matches!(k, TouchEventKind::Swipe(TouchSwipeDirection::Right))));
}

#[test]
fn recontact_after_release_gap_emits_up_for_previous_interaction() {
    let mut engine = TouchEngine::new();
    let mut events = std::vec::Vec::new();

    drain_kinds(engine.tick(0, sample1(100, 100)), &mut events);
    drain_kinds(engine.tick(20, sample1(100, 100)), &mut events);
    // Enter release debounce.
    drain_kinds(engine.tick(40, sample0()), &mut events);
    // Re-contact well after continuity recovery window; old press must
    // finalize with Up before a new interaction starts.
    drain_kinds(engine.tick(160, sample1(200, 200)), &mut events);

    assert!(events.iter().any(|k| matches!(k, TouchEventKind::Up)));
}

#[cfg(test)]
mod part2;
