#![allow(dead_code)]

mod types {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum TouchSwipeDirection {
        Left,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum TouchEventKind {
        Down,
        Move,
        Up,
        Tap,
        LongPress,
        Swipe(TouchSwipeDirection),
        Cancel,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct TouchEvent {
        pub(crate) kind: TouchEventKind,
        pub(crate) t_ms: u64,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct TouchActivitySnapshot {
        pub(crate) active: bool,
        pub(crate) last_nonzero_ms: Option<u64>,
    }
}

#[path = "../../../src/firmware/touch/imu_activity.rs"]
mod imu_activity;

use imu_activity::snapshot_for_event;
use types::{TouchEvent, TouchEventKind};

#[test]
fn release_preserves_timestamp_for_post_touch_bus_quiet_window() {
    let snapshot = snapshot_for_event(TouchEvent {
        kind: TouchEventKind::Up,
        t_ms: 1_234,
    });

    assert!(!snapshot.active);
    assert_eq!(snapshot.last_nonzero_ms, Some(1_234));
}

#[test]
fn pressed_contact_remains_active() {
    let snapshot = snapshot_for_event(TouchEvent {
        kind: TouchEventKind::Down,
        t_ms: 42,
    });

    assert!(snapshot.active);
    assert_eq!(snapshot.last_nonzero_ms, Some(42));
}
