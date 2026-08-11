#![allow(dead_code)]

#[path = "../../../src/firmware/touch/lvgl_multitouch.rs"]
mod lvgl_multitouch;

use lvgl_multitouch::{LvglMultitouchFrame, LvglMultitouchTracker, LvglTouchPoint};

fn frame(t_ms: u64, active_mask: u8, points: [(u16, u16); 2]) -> LvglMultitouchFrame {
    LvglMultitouchFrame {
        t_ms,
        active_mask,
        points: points.map(|(x, y)| LvglTouchPoint { x, y }),
    }
}

#[test]
fn stable_slot_ids_are_pressed_on_each_multitouch_report() {
    let mut tracker = LvglMultitouchTracker::default();
    let batch = tracker.update(frame(42, 0x03, [(10, 20), (30, 40)]));
    let updates: Vec<_> = batch.updates.into_iter().flatten().collect();

    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].id, 0);
    assert_eq!(updates[0].point, LvglTouchPoint { x: 10, y: 20 });
    assert!(updates[0].pressed);
    assert_eq!(updates[1].id, 1);
    assert_eq!(updates[1].point, LvglTouchPoint { x: 30, y: 40 });
    assert!(updates[1].pressed);
    assert_eq!(updates[0].timestamp, 42);
}

#[test]
fn released_slot_uses_its_last_active_coordinates() {
    let mut tracker = LvglMultitouchTracker::default();
    tracker.update(frame(10, 0x03, [(100, 200), (300, 400)]));
    let batch = tracker.update(frame(20, 0x01, [(110, 210), (999, 999)]));
    let updates: Vec<_> = batch.updates.into_iter().flatten().collect();

    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].id, 0);
    assert!(updates[0].pressed);
    assert_eq!(updates[1].id, 1);
    assert!(!updates[1].pressed);
    assert_eq!(updates[1].point, LvglTouchPoint { x: 300, y: 400 });
    assert_eq!(updates[1].timestamp, 20);
}

#[test]
fn reset_releases_every_active_slot() {
    let mut tracker = LvglMultitouchTracker::default();
    tracker.update(frame(10, 0x03, [(100, 200), (300, 400)]));
    let batch = tracker.release_all(25);
    let updates: Vec<_> = batch.updates.into_iter().flatten().collect();

    assert_eq!(updates.len(), 2);
    assert!(updates.iter().all(|update| !update.pressed));
    assert_eq!(updates[0].id, 0);
    assert_eq!(updates[1].id, 1);
    assert_eq!(updates[0].timestamp, 25);
    assert_eq!(updates[1].timestamp, 25);
}

#[test]
fn reset_before_any_multitouch_contact_is_empty() {
    let mut tracker = LvglMultitouchTracker::default();

    assert!(tracker.release_all(25).is_empty());
}

#[test]
fn ending_gesture_releases_both_slots_when_one_finger_remains() {
    let mut tracker = LvglMultitouchTracker::default();
    let (_, terminating) = tracker.update_gesture(frame(10, 0x03, [(100, 200), (300, 400)]));
    assert!(!terminating);

    let (batch, terminating) = tracker.update_gesture(frame(20, 0x01, [(110, 210), (999, 999)]));
    let updates: Vec<_> = batch.updates.into_iter().flatten().collect();

    assert!(terminating);
    assert_eq!(updates.len(), 2);
    assert!(updates.iter().all(|update| !update.pressed));
    assert_eq!(updates[0].id, 0);
    assert_eq!(updates[1].id, 1);
}

#[test]
fn repeated_gestures_start_from_two_fresh_contacts() {
    let mut tracker = LvglMultitouchTracker::default();

    for sequence in 0..16 {
        let started_at = sequence * 20;
        let (started, terminating) =
            tracker.update_gesture(frame(started_at, 0x03, [(100, 200), (300, 400)]));
        let presses: Vec<_> = started.updates.into_iter().flatten().collect();
        assert!(!terminating);
        assert_eq!(presses.len(), 2);
        assert!(presses.iter().all(|update| update.pressed));

        let (ended, terminating) = tracker.update_gesture(frame(
            started_at + 10,
            if sequence % 2 == 0 { 0x01 } else { 0x02 },
            [(110, 210), (310, 410)],
        ));
        let releases: Vec<_> = ended.updates.into_iter().flatten().collect();
        assert!(terminating);
        assert_eq!(releases.len(), 2);
        assert!(releases.iter().all(|update| !update.pressed));
    }
}
