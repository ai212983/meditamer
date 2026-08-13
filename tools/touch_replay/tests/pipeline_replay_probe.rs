#[path = "../../../src/platform/inkplate/touch/protocol.rs"]
mod elan_protocol;
#[path = "../../../src/firmware/touch/core/mod.rs"]
mod touch_core;
#[path = "../../../src/firmware/touch/normalize/mod.rs"]
mod touch_normalize;
#[path = "../../../src/firmware/touch/replay.rs"]
mod touch_replay_probe;

use touch_core::{TouchEngine, TouchEventKind, TouchPoint, TouchSample};
use touch_normalize::{NormalizedTouchPoint, NormalizedTouchSample, TouchPresenceNormalizer};
use touch_replay_probe::{PIPELINE_REPLAY_TAP, PIPELINE_REPLAY_X, PIPELINE_REPLAY_Y};

#[test]
fn replay_is_a_fixed_single_contact_elan_sequence() {
    assert_eq!(
        PIPELINE_REPLAY_TAP.map(|frame| frame.offset_ms),
        [0, 20, 35, 90, 110, 150]
    );
    assert_eq!(
        PIPELINE_REPLAY_TAP.map(|frame| frame.touch_count),
        [1, 1, 1, 1, 0, 0]
    );

    for frame in &PIPELINE_REPLAY_TAP[..4] {
        assert_eq!((frame.x, frame.y), (PIPELINE_REPLAY_X, PIPELINE_REPLAY_Y));
        assert!(elan_protocol::is_touch_report(&frame.raw));
        assert_eq!(elan_protocol::active_slots(&frame.raw), 0x01);
    }
    for frame in &PIPELINE_REPLAY_TAP[4..] {
        assert_eq!((frame.x, frame.y), (0, 0));
        assert!(elan_protocol::is_touch_report(&frame.raw));
        assert_eq!(elan_protocol::active_slots(&frame.raw), 0);
    }
}

#[test]
fn replay_produces_one_tap_for_every_pipeline_ticker_phase() {
    for ticker_phase_ms in 0..8 {
        for ticker_first_on_tie in [false, true] {
            let events = replay_events(ticker_phase_ms, ticker_first_on_tie);
            let kinds: Vec<TouchEventKind> = events.iter().map(|event| event.kind).collect();
            assert_eq!(
                kinds,
                [
                    TouchEventKind::Down,
                    TouchEventKind::Up,
                    TouchEventKind::Tap
                ],
                "ticker_phase_ms={ticker_phase_ms} ticker_first_on_tie={ticker_first_on_tie}"
            );
            assert!(events.iter().all(|event| {
                event.x == PIPELINE_REPLAY_X
                    && event.y == PIPELINE_REPLAY_Y
                    && event.touch_count <= 1
            }));
        }
    }
}

fn replay_events(ticker_phase_ms: u64, ticker_first_on_tie: bool) -> Vec<touch_core::TouchEvent> {
    let mut normalizer = TouchPresenceNormalizer::new();
    let mut engine = TouchEngine::new();
    let mut events = Vec::new();
    let mut replay_index = 0usize;

    for now_ms in 0..=400 {
        let sample_due = PIPELINE_REPLAY_TAP
            .get(replay_index)
            .is_some_and(|frame| frame.offset_ms == now_ms);
        let ticker_due = now_ms >= ticker_phase_ms && (now_ms - ticker_phase_ms).is_multiple_of(8);

        if ticker_first_on_tie && ticker_due {
            advance(&mut normalizer, &mut engine, now_ms, &mut events);
        }
        if sample_due {
            let frame = PIPELINE_REPLAY_TAP[replay_index];
            replay_index += 1;
            sample(
                &mut normalizer,
                &mut engine,
                now_ms,
                frame.touch_count,
                frame.x,
                frame.y,
                frame.raw,
                &mut events,
            );
        }
        if !ticker_first_on_tie && ticker_due {
            advance(&mut normalizer, &mut engine, now_ms, &mut events);
        }
    }

    assert_eq!(replay_index, PIPELINE_REPLAY_TAP.len());
    events
}

#[allow(clippy::too_many_arguments)]
fn sample(
    normalizer: &mut TouchPresenceNormalizer,
    engine: &mut TouchEngine,
    now_ms: u64,
    touch_count: u8,
    x: u16,
    y: u16,
    raw: [u8; 8],
    events: &mut Vec<touch_core::TouchEvent>,
) {
    let normalized = NormalizedTouchSample {
        touch_count,
        points: [
            NormalizedTouchPoint { x, y },
            NormalizedTouchPoint::default(),
        ],
        raw,
    };
    let (count, primary) = normalizer.normalize(now_ms, normalized);
    tick_engine(engine, now_ms, count, primary, events);
}

fn advance(
    normalizer: &mut TouchPresenceNormalizer,
    engine: &mut TouchEngine,
    now_ms: u64,
    events: &mut Vec<touch_core::TouchEvent>,
) {
    let (count, primary) = normalizer.advance(now_ms);
    tick_engine(engine, now_ms, count, primary, events);
}

fn tick_engine(
    engine: &mut TouchEngine,
    now_ms: u64,
    touch_count: u8,
    primary: Option<NormalizedTouchPoint>,
    events: &mut Vec<touch_core::TouchEvent>,
) {
    let sample = TouchSample {
        touch_count,
        points: [
            primary
                .map(|point| TouchPoint {
                    x: point.x,
                    y: point.y,
                })
                .unwrap_or_default(),
            TouchPoint::default(),
        ],
    };
    events.extend(engine.tick(now_ms, sample).events.into_iter().flatten());
}
