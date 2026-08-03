use super::types::{TouchActivitySnapshot, TouchEvent, TouchEventKind};

pub(crate) const fn snapshot_for_event(event: TouchEvent) -> TouchActivitySnapshot {
    let active = matches!(
        event.kind,
        TouchEventKind::Down | TouchEventKind::Move | TouchEventKind::LongPress
    );
    TouchActivitySnapshot {
        active,
        // Release must retain a timestamp so the normal post-touch quiet window
        // remains meaningful outside explicit panel transactions.
        last_nonzero_ms: Some(event.t_ms),
    }
}
