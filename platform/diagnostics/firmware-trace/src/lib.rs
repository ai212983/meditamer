//! Allocation-free, event-only tracing capture for firmware.
#![no_std]

use core::{fmt, mem::MaybeUninit, ptr, slice};
use heapless::Vec;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

/// Maximum number of fields retained from one event.
pub const MAX_FIELDS: usize = 6;

/// Identity of the execution context which emitted an event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionContext {
    pub core: u8,
    /// Platform-defined kind, for example task, interrupt, or host thread.
    pub kind: u8,
    pub id: u32,
}

/// A value representable by the bounded wire-neutral capture format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldValue {
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventField {
    pub name: &'static str,
    pub value: FieldValue,
}

/// One captured `tracing` event.
#[derive(Clone, Debug)]
pub struct EventRecord {
    pub timestamp: u64,
    pub context: ExecutionContext,
    pub name: &'static str,
    pub target: &'static str,
    pub level: tracing::Level,
    /// Always `None`: this event-only collector rejects explicit span parents.
    pub parent: Option<u64>,
    pub fields: Vec<EventField, MAX_FIELDS>,
    /// Number of fields omitted from this event due to type or capacity.
    pub omitted_fields: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LossCounters {
    pub records: u64,
    pub fields: u64,
    pub unsupported_fields: u64,
    /// Events carrying an explicit span parent, which this event-only collector rejects.
    pub parented_events: u64,
}

/// Caller-owned bounded capture storage. Put this in the memory region chosen
/// by the firmware; this crate creates no large internal static.
pub struct Collector<const N: usize> {
    records: [MaybeUninit<EventRecord>; N],
    len: usize,
    losses: LossCounters,
    generation: u32,
    capturing: bool,
    full: bool,
}

impl<const N: usize> Collector<N> {
    pub const fn new() -> Self {
        Self {
            records: [const { MaybeUninit::uninit() }; N],
            len: 0,
            losses: LossCounters {
                records: 0,
                fields: 0,
                unsupported_fields: 0,
                parented_events: 0,
            },
            generation: 0,
            capturing: true,
            full: false,
        }
    }

    /// Initializes a collector directly in caller-provided storage.
    ///
    /// Unlike [`Collector::new`], this does not construct or move an
    /// `N * size_of::<EventRecord>()` temporary. It writes only the small
    /// header. The record array consists of `MaybeUninit` elements, for which
    /// every bit pattern is valid, and remains uninitialized until records are
    /// appended.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let collector = slot.as_mut_ptr();
        // SAFETY: `collector` is aligned, writable storage supplied by the
        // caller. Every possible bit pattern of the untouched `records` field
        // is a valid `[MaybeUninit<EventRecord>; N]`. The writes initialize all
        // remaining fields before the reference is formed. `len = 0` means no
        // element is treated as initialized.
        unsafe {
            ptr::addr_of_mut!((*collector).len).write(0);
            ptr::addr_of_mut!((*collector).losses).write(LossCounters::default());
            ptr::addr_of_mut!((*collector).generation).write(0);
            ptr::addr_of_mut!((*collector).capturing).write(true);
            ptr::addr_of_mut!((*collector).full).write(false);
            &mut *collector
        }
    }

    pub fn records(&self) -> impl ExactSizeIterator<Item = &EventRecord> {
        self.as_slice().iter()
    }

    pub fn losses(&self) -> LossCounters {
        self.losses
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }
    pub fn is_capturing(&self) -> bool {
        self.capturing
    }
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Stop capture while retaining records for dumping.
    pub fn freeze(&mut self) {
        self.capturing = false;
    }

    /// Clear the old capture and start a distinguishable new generation.
    pub fn clear_and_rearm(&mut self) {
        self.clear_records();
        self.losses = LossCounters::default();
        self.generation = self.generation.wrapping_add(1);
        self.capturing = true;
        self.full = false;
    }

    /// Append a caller-created event without running a tracing subscriber.
    /// This supports synchronized trigger records at a capture state boundary.
    pub fn push_record(&mut self, record: EventRecord) {
        self.push(record, FieldLosses::default());
    }

    fn push(&mut self, mut record: EventRecord, field_losses: FieldLosses) {
        if !self.capturing {
            return;
        }
        record.omitted_fields = field_losses.total();
        self.losses.fields = self
            .losses
            .fields
            .saturating_add(field_losses.capacity as u64);
        self.losses.unsupported_fields = self
            .losses
            .unsupported_fields
            .saturating_add(field_losses.unsupported as u64);
        if self.len == N {
            self.losses.records = self.losses.records.saturating_add(1);
            self.capturing = false;
            self.full = true;
        } else {
            self.records[self.len].write(record);
            self.len += 1;
            if self.len == N {
                self.capturing = false;
                self.full = true;
            }
        }
    }

    fn reject_parented(&mut self) {
        if self.capturing {
            self.losses.parented_events = self.losses.parented_events.saturating_add(1);
        }
    }

    fn as_slice(&self) -> &[EventRecord] {
        // SAFETY: elements below `len` are initialized exactly once by
        // `push`; `len` never exceeds N. No method exposes those slots while
        // mutating the collector.
        unsafe { slice::from_raw_parts(self.records.as_ptr().cast(), self.len) }
    }

    fn clear_records(&mut self) {
        while self.len != 0 {
            self.len -= 1;
            // SAFETY: this slot was initialized while it was below the old
            // length, and decrementing first prevents a double drop on panic.
            unsafe { self.records[self.len].assume_init_drop() };
        }
    }
}

impl<const N: usize> Default for Collector<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Drop for Collector<N> {
    fn drop(&mut self) {
        self.clear_records();
    }
}

/// Serializes access to caller-owned storage. Firmware normally masks the
/// relevant interrupts around `operation`; it must not call tracing recursively.
pub type AccessFn<const N: usize> = fn(&mut dyn FnMut(&mut Collector<N>));
pub type ClockFn = fn() -> u64;
pub type ContextFn = fn() -> ExecutionContext;
pub type EnabledFn = fn() -> bool;
pub type RecordSinkFn = fn(Option<EventRecord>, RecordLosses);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecordLosses {
    pub capacity: u16,
    pub unsupported: u16,
    pub parented: u16,
}

const fn always_enabled() -> bool {
    true
}

/// Tiny, copyable subscriber suitable for static startup installation.
#[derive(Clone, Copy)]
pub struct TraceSubscriber<const N: usize> {
    access: Option<AccessFn<N>>,
    sink: Option<RecordSinkFn>,
    clock: ClockFn,
    context: ContextFn,
    enabled: EnabledFn,
}

impl<const N: usize> TraceSubscriber<N> {
    pub const fn new(access: AccessFn<N>, clock: ClockFn, context: ContextFn) -> Self {
        Self {
            access: Some(access),
            sink: None,
            clock,
            context,
            enabled: always_enabled,
        }
    }

    pub const fn new_sink(clock: ClockFn, context: ContextFn, sink: RecordSinkFn) -> Self {
        Self {
            access: None,
            sink: Some(sink),
            clock,
            context,
            enabled: always_enabled,
        }
    }

    /// Adds a cheap, lock-free capture gate. Firmware should use this so a
    /// disabled capture does not evaluate event fields or sample providers.
    pub const fn with_enabled(mut self, enabled: EnabledFn) -> Self {
        self.enabled = enabled;
        self
    }
}

impl<const N: usize> Subscriber for TraceSubscriber<N> {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && (self.enabled)()
    }

    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        if metadata.is_event() {
            tracing::subscriber::Interest::sometimes()
        } else {
            tracing::subscriber::Interest::never()
        }
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event_enabled(&self, event: &Event<'_>) -> bool {
        event.metadata().is_event() && (self.enabled)()
    }

    fn event(&self, event: &Event<'_>) {
        if !(self.enabled)() {
            return;
        }
        // User providers run before storage serialization, never under its guard.
        let timestamp = (self.clock)();
        let context = (self.context)();
        // Correlation is an ordinary numeric field. Explicit span parents are
        // rejected; disabled span callsites prevent ambient span attachment.
        if event.parent().is_some() {
            if let Some(sink) = self.sink {
                sink(
                    None,
                    RecordLosses {
                        parented: 1,
                        ..RecordLosses::default()
                    },
                );
            }
            if let Some(access) = self.access {
                let mut operation = |collector: &mut Collector<N>| collector.reject_parented();
                access(&mut operation);
            }
            return;
        }
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);
        let record = EventRecord {
            timestamp,
            context,
            name: metadata.name(),
            target: metadata.target(),
            level: *metadata.level(),
            parent: None,
            fields: visitor.fields,
            omitted_fields: 0,
        };
        let losses = visitor.losses;
        if let Some(sink) = self.sink {
            sink(
                Some(record),
                RecordLosses {
                    capacity: losses.capacity,
                    unsupported: losses.unsupported,
                    parented: 0,
                },
            );
        } else if let Some(access) = self.access {
            let mut operation =
                |collector: &mut Collector<N>| collector.push(record.clone(), losses);
            access(&mut operation);
        }
    }

    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[derive(Clone, Copy, Default)]
struct FieldLosses {
    capacity: u16,
    unsupported: u16,
}

impl FieldLosses {
    fn total(self) -> u16 {
        self.capacity.saturating_add(self.unsupported)
    }
}

struct FieldVisitor {
    fields: Vec<EventField, MAX_FIELDS>,
    losses: FieldLosses,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            fields: Vec::new(),
            losses: FieldLosses::default(),
        }
    }
    fn push(&mut self, field: &Field, value: FieldValue) {
        if self
            .fields
            .push(EventField {
                name: field.name(),
                value,
            })
            .is_err()
        {
            self.losses.capacity = self.losses.capacity.saturating_add(1);
        }
    }
}

impl Visit for FieldVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, FieldValue::I64(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, FieldValue::U64(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, FieldValue::Bool(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field, FieldValue::F64(value));
    }
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {
        self.losses.unsupported = self.losses.unsupported.saturating_add(1);
    }
}

#[cfg(test)]
mod tests;
