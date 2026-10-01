use super::{config::EventEngineConfig, types::EventKind};

#[derive(Clone, Copy, Debug)]
pub(crate) struct EventRegistration {
    pub(crate) kind: EventKind,
    pub(crate) enabled: bool,
}

pub(crate) fn event_registry(config: &EventEngineConfig) -> [EventRegistration; 7] {
    [
        EventRegistration {
            kind: EventKind::DoubleTap,
            enabled: config.triple_tap.enabled,
        },
        EventRegistration {
            kind: EventKind::Pickup,
            enabled: config.optional_events.pickup_enabled,
        },
        EventRegistration {
            kind: EventKind::Placement,
            enabled: config.optional_events.placement_enabled,
        },
        EventRegistration {
            kind: EventKind::StillnessStart,
            enabled: config.optional_events.stillness_start_enabled,
        },
        EventRegistration {
            kind: EventKind::StillnessEnd,
            enabled: config.optional_events.stillness_end_enabled,
        },
        EventRegistration {
            kind: EventKind::NearIntent,
            enabled: config.optional_events.near_intent_enabled,
        },
        EventRegistration {
            kind: EventKind::FarIntent,
            enabled: config.optional_events.far_intent_enabled,
        },
    ]
}
