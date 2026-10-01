extern crate std;

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use tracing::subscriber::with_default;

const CAPACITY: usize = 2;
static COLLECTOR: Mutex<Collector<CAPACITY>> = Mutex::new(Collector::new());
static SAMPLE: Mutex<(u64, ExecutionContext)> = Mutex::new((
    0,
    ExecutionContext {
        core: 0,
        kind: 0,
        id: 0,
    },
));
static TEST_LOCK: Mutex<()> = Mutex::new(());
static ENABLED: AtomicBool = AtomicBool::new(true);
static PROVIDER_CALLS: AtomicUsize = AtomicUsize::new(0);

fn access(operation: &mut dyn FnMut(&mut Collector<CAPACITY>)) {
    operation(&mut COLLECTOR.lock().unwrap());
}
fn clock() -> u64 {
    PROVIDER_CALLS.fetch_add(1, Ordering::Relaxed);
    SAMPLE.lock().unwrap().0
}
fn context() -> ExecutionContext {
    PROVIDER_CALLS.fetch_add(1, Ordering::Relaxed);
    SAMPLE.lock().unwrap().1
}
fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
fn subscriber() -> TraceSubscriber<CAPACITY> {
    TraceSubscriber::new(access, clock, context)
}

fn reset() {
    let mut collector = COLLECTOR.lock().unwrap();
    *collector = Collector::new();
    ENABLED.store(true, Ordering::Relaxed);
    PROVIDER_CALLS.store(0, Ordering::Relaxed);
}

#[test]
fn in_place_initialization_uses_the_supplied_address() {
    let _test = TEST_LOCK.lock().unwrap();
    let mut storage = MaybeUninit::<Collector<1>>::uninit();
    let expected = storage.as_mut_ptr();
    let collector = Collector::init_in_place(&mut storage);
    assert_eq!(ptr::from_mut(collector), expected);
    assert!(collector.is_capturing());
    assert_eq!(collector.records().len(), 0);
    // SAFETY: `init_in_place` initialized the collector in `storage`, and the
    // mutable reference is no longer used.
    unsafe { storage.assume_init_drop() };
}

#[test]
fn disabled_gate_skips_fields_and_providers() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    ENABLED.store(false, Ordering::Relaxed);
    let mut evaluated = false;
    with_default(subscriber().with_enabled(enabled), || {
        tracing::info!(parent: None, value = { evaluated = true; 1_u64 });
    });
    assert!(!evaluated);
    assert_eq!(PROVIDER_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(COLLECTOR.lock().unwrap().records().len(), 0);
}

#[test]
fn macros_capture_numeric_fields_and_sample_each_context() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    with_default(subscriber(), || {
        *SAMPLE.lock().unwrap() = (
            11,
            ExecutionContext {
                core: 0,
                kind: 1,
                id: 7,
            },
        );
        tracing::info!(target: "firmware", parent: None, flow_id = 9_u64, signed = -2_i64, ready = true, "ignored text");
        *SAMPLE.lock().unwrap() = (
            12,
            ExecutionContext {
                core: 1,
                kind: 2,
                id: 3,
            },
        );
        tracing::warn!(parent: None, value = 1.5_f64);
    });
    let collector = COLLECTOR.lock().unwrap();
    let records: std::vec::Vec<_> = collector.records().collect();
    assert_eq!(records[0].timestamp, 11);
    assert_eq!(
        records[0].context,
        ExecutionContext {
            core: 0,
            kind: 1,
            id: 7
        }
    );
    assert_eq!(records[0].target, "firmware");
    assert_eq!(records[0].fields.len(), 3);
    assert_eq!(records[0].omitted_fields, 1); // formatted message
    assert_eq!(records[1].context.core, 1);
    assert_eq!(records[1].fields[0].value, FieldValue::F64(1.5));
    assert_eq!(collector.losses().unsupported_fields, 1);
}

#[test]
fn reaching_record_capacity_freezes_capture() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    with_default(subscriber(), || {
        tracing::info!(parent: None, n = 1_u64);
        tracing::info!(parent: None, n = 2_u64);
        tracing::info!(parent: None, n = 3_u64);
    });
    let collector = COLLECTOR.lock().unwrap();
    assert_eq!(collector.records().len(), 2);
    assert_eq!(collector.losses().records, 0);
    assert!(collector.is_full());
    assert!(!collector.is_capturing());
}

#[test]
fn zero_capacity_freezes_on_first_record_attempt() {
    let mut collector = Collector::<0>::new();
    let record = EventRecord {
        timestamp: 0,
        context: ExecutionContext {
            core: 0,
            kind: 0,
            id: 0,
        },
        name: "event",
        target: "test",
        level: tracing::Level::INFO,
        parent: None,
        fields: Vec::new(),
        omitted_fields: 0,
    };
    collector.push_record(record);
    assert!(collector.is_full());
    assert!(!collector.is_capturing());
    assert_eq!(collector.losses().records, 1);
}

#[test]
fn field_capacity_and_unsupported_types_are_fail_visible() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    with_default(subscriber(), || {
        tracing::event!(parent: None, tracing::Level::INFO,
            a=1_u64,b=2_u64,c=3_u64,d=4_u64,e=5_u64,f=6_u64,g=7_u64,text=?"x");
    });
    let collector = COLLECTOR.lock().unwrap();
    let record = collector.records().next().unwrap();
    assert_eq!(record.fields.len(), MAX_FIELDS);
    assert_eq!(record.omitted_fields, 2);
    assert_eq!(collector.losses().fields, 1);
    assert_eq!(collector.losses().unsupported_fields, 1);
}

#[test]
fn freeze_preserves_dump_and_rearm_starts_new_generation() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    with_default(subscriber(), || tracing::info!(parent: None, n=1_u64));
    COLLECTOR.lock().unwrap().freeze();
    with_default(subscriber(), || tracing::info!(parent: None, n=2_u64));
    assert_eq!(COLLECTOR.lock().unwrap().records().len(), 1);
    COLLECTOR.lock().unwrap().clear_and_rearm();
    with_default(subscriber(), || tracing::info!(parent: None, n=3_u64));
    let collector = COLLECTOR.lock().unwrap();
    assert_eq!(collector.generation(), 1);
    assert_eq!(collector.records().len(), 1);
    assert_eq!(collector.losses(), LossCounters::default());
}

#[test]
fn span_callsites_are_rejected_and_cannot_supply_implicit_parents() {
    let _test = TEST_LOCK.lock().unwrap();
    reset();
    with_default(subscriber(), || {
        let span = tracing::info_span!("not_captured", id = 4_u64);
        let _guard = span.enter();
        tracing::info!(flow_id = 4_u64);
    });
    let collector = COLLECTOR.lock().unwrap();
    let record = collector.records().next().unwrap();
    assert_eq!(record.parent, None);
    assert_eq!(record.fields[0].name, "flow_id");
}
