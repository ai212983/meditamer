//! CPU0-owned PSRAM trace storage with a bounded internal-DRAM ingress.

use super::{psram::ExternalValue, touch::debug_log::uart_write_all, types::SerialWriter};
use core::{
    cell::RefCell,
    fmt::Write,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use embassy_sync::{
    blocking_mutex::{raw::CriticalSectionRawMutex, Mutex},
    signal::Signal,
};
use firmware_trace::{
    Collector, EventField, EventRecord, ExecutionContext, FieldValue, RecordLosses, TraceSubscriber,
};

const CAPACITY: usize = 512;
const INGRESS_CAPACITY: usize = 16;
const WIRE_LINE_CAPACITY: usize = 384;
const MODE_UNINITIALIZED: u8 = 0;
const MODE_FROZEN: u8 = 1;
const MODE_ARMED: u8 = 2;
const MODE_CAPTURING: u8 = 3;
const MODE_FULL: u8 = 4;
type TraceCollector = Collector<CAPACITY>;

struct QueuedRecord {
    record: EventRecord,
    checksum: u64,
}
struct Ingress {
    queue: heapless::Deque<QueuedRecord, INGRESS_CAPACITY>,
    mode: u8,
    generation: u32,
    accepted: usize,
    field_overflow: u64,
    unsupported: u64,
    parented: u64,
    ingress_overflow: u64,
    integrity_errors: u64,
}
impl Ingress {
    const fn new() -> Self {
        Self {
            queue: heapless::Deque::new(),
            mode: MODE_UNINITIALIZED,
            generation: 0,
            accepted: 0,
            field_overflow: 0,
            unsupported: 0,
            parented: 0,
            ingress_overflow: 0,
            integrity_errors: 0,
        }
    }
    fn reset(&mut self, mode: u8, generation: u32) {
        self.queue.clear();
        self.mode = mode;
        self.generation = generation;
        self.accepted = 0;
        self.field_overflow = 0;
        self.unsupported = 0;
        self.parented = 0;
        self.ingress_overflow = 0;
        self.integrity_errors = 0;
    }
}

static COLLECTOR: Mutex<CriticalSectionRawMutex, RefCell<Option<&'static mut TraceCollector>>> =
    Mutex::new(RefCell::new(None));
static INGRESS: Mutex<CriticalSectionRawMutex, RefCell<Ingress>> =
    Mutex::new(RefCell::new(Ingress::new()));
static DRAIN_WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PROBE_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PROBE_DONE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static CAPTURING: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static MODE: AtomicU8 = AtomicU8::new(MODE_UNINITIALIZED);
static PHYSICAL_TOUCH_CONTACT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitError {
    Allocation,
    SubscriberAlreadyInstalled,
}

pub fn init() -> Result<(), InitError> {
    assert_cpu0();
    if INITIALIZED.load(Ordering::Acquire) {
        return Err(InitError::SubscriberAlreadyInstalled);
    }
    let collector = allocate_collector()?;
    collector.freeze();
    COLLECTOR.lock(|slot| *slot.borrow_mut() = Some(collector));
    INGRESS.lock(|state| state.borrow_mut().reset(MODE_FROZEN, 0));
    publish_mode(MODE_FROZEN);
    let subscriber =
        TraceSubscriber::<CAPACITY>::new_sink(clock, context, sink).with_enabled(is_capturing);
    tracing::dispatcher::set_global_default(tracing::Dispatch::new(subscriber))
        .map_err(|_| InitError::SubscriberAlreadyInstalled)?;
    INITIALIZED.store(true, Ordering::Release);
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::install_render_hook();
    Ok(())
}

#[inline(never)]
fn allocate_collector() -> Result<&'static mut TraceCollector, InitError> {
    let storage = ExternalValue::try_new_with(MaybeUninit::<TraceCollector>::uninit)
        .map_err(|_| InitError::Allocation)?;
    Ok(TraceCollector::init_in_place(storage.leak()))
}

#[inline]
pub fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}
fn publish_mode(mode: u8) {
    MODE.store(mode, Ordering::Release);
    CAPTURING.store(mode == MODE_CAPTURING, Ordering::Release);
}
fn clock() -> u64 {
    embassy_time::Instant::now().as_micros()
}
fn context() -> ExecutionContext {
    let (core, kind, id) = cpu_load::execution_context();
    ExecutionContext { core, kind, id }
}

fn sink(record: Option<EventRecord>, losses: RecordLosses) {
    let mut wake = false;
    INGRESS.lock(|state| {
        let mut state = state.borrow_mut();
        if state.mode != MODE_CAPTURING {
            return;
        }
        state.field_overflow = state
            .field_overflow
            .saturating_add(u64::from(losses.capacity));
        state.unsupported = state
            .unsupported
            .saturating_add(u64::from(losses.unsupported));
        state.parented = state.parented.saturating_add(u64::from(losses.parented));
        if let Some(record) = record {
            wake = enqueue_locked(&mut state, record);
        }
    });
    if wake {
        DRAIN_WAKE.signal(());
    }
}

fn enqueue_locked(state: &mut Ingress, record: EventRecord) -> bool {
    if state.accepted == CAPACITY {
        state.mode = MODE_FULL;
        publish_mode(MODE_FULL);
        return false;
    }
    let checksum = checksum(&record);
    if state
        .queue
        .push_back(QueuedRecord { record, checksum })
        .is_err()
    {
        state.ingress_overflow = state.ingress_overflow.saturating_add(1);
        return false;
    }
    state.accepted += 1;
    if state.accepted == CAPACITY {
        state.mode = MODE_FULL;
        publish_mode(MODE_FULL);
    }
    true
}

fn checksum(record: &EventRecord) -> u64 {
    let mut value = record.timestamp
        ^ (u64::from(record.context.core) << 56)
        ^ (u64::from(record.context.kind) << 48)
        ^ u64::from(record.context.id);
    for byte in record.target.bytes().chain(record.name.bytes()) {
        value = value.rotate_left(5) ^ u64::from(byte);
    }
    for field in &record.fields {
        for byte in field.name.bytes() {
            value = value.rotate_left(5) ^ u64::from(byte);
        }
        let bits = match field.value {
            FieldValue::U64(v) => v,
            FieldValue::I64(v) => v as u64,
            FieldValue::F64(v) => v.to_bits(),
            FieldValue::Bool(v) => u64::from(v),
        };
        value = value.rotate_left(7) ^ bits;
    }
    value
}

fn assert_cpu0() {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::ProCpu
    );
}
fn drain_pending_cpu0() {
    assert_cpu0();
    loop {
        let drained = COLLECTOR.lock(|slot| {
            INGRESS.lock(|state| {
                let Some(queued) = state.borrow_mut().queue.pop_front() else {
                    return false;
                };
                if checksum(&queued.record) != queued.checksum {
                    let mut state = state.borrow_mut();
                    state.integrity_errors = state.integrity_errors.saturating_add(1);
                } else if let Some(collector) = slot.borrow_mut().as_mut() {
                    collector.push_record(queued.record);
                }
                true
            })
        });
        if !drained {
            break;
        }
    }
}

#[embassy_executor::task]
pub async fn drain_task() {
    assert_cpu0();
    loop {
        DRAIN_WAKE.wait().await;
        drain_pending_cpu0();
    }
}

#[embassy_executor::task]
pub async fn probe_task() {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    loop {
        PROBE_REQUEST.wait().await;
        for index in 0..16_u64 {
            probe_event(1, index);
            embassy_time::Timer::after_millis(2).await;
        }
        PROBE_DONE.signal(());
    }
}

fn probe_event(producer: u64, index: u64) {
    let value = (producer << 32) | index;
    tracing::event!(name:"probe", target:"firmware", parent:None, tracing::Level::INFO,
        producer, index, value, inverse = !value);
}

pub async fn command(uart: &mut SerialWriter, operation: u8) {
    match operation {
        0 => start(uart).await,
        1 => stop(uart).await,
        2 => dump(uart).await,
        3 => arm(uart).await,
        4 => status(uart).await,
        5 => selftest(uart).await,
        _ => write_error(uart, b"TRACE ERROR reason=invalid_operation\r\n").await,
    }
}

fn reset_capture(mode: u8, force: bool, first: Option<EventRecord>) -> Result<u32, ()> {
    assert_cpu0();
    let generation = COLLECTOR.lock(|slot| {
        let mut slot = slot.borrow_mut();
        let collector = slot.as_mut().ok_or(())?;
        INGRESS.lock(|state| {
            let mut state = state.borrow_mut();
            if matches!(state.mode, MODE_ARMED | MODE_CAPTURING) && !force {
                return Err(());
            }
            publish_mode(MODE_FROZEN);
            state.queue.clear();
            collector.clear_and_rearm();
            let generation = collector.generation();
            state.reset(mode, generation);
            if let Some(record) = first {
                let _ = enqueue_locked(&mut state, record);
            }
            publish_mode(state.mode);
            Ok(generation)
        })
    })?;
    DRAIN_WAKE.signal(());
    Ok(generation)
}

fn capture_started_record() -> EventRecord {
    let mut fields = heapless::Vec::new();
    let _ = fields.push(EventField {
        name: "version",
        value: FieldValue::U64(1),
    });
    EventRecord {
        timestamp: clock(),
        context: context(),
        name: "capture_started",
        target: "firmware",
        level: tracing::Level::INFO,
        parent: None,
        fields,
        omitted_fields: 0,
    }
}
async fn start(uart: &mut SerialWriter) {
    let first = capture_started_record();
    match reset_capture(MODE_CAPTURING, false, Some(first)) {
        Ok(g) => ack(uart, "START", g).await,
        Err(_) => write_error(uart, b"TRACE ERROR reason=already_active\r\n").await,
    }
}
async fn arm(uart: &mut SerialWriter) {
    match reset_capture(MODE_ARMED, false, None) {
        Ok(g) => ack(uart, "ARM", g).await,
        Err(_) => write_error(uart, b"TRACE ERROR reason=already_active\r\n").await,
    }
}
async fn ack(uart: &mut SerialWriter, name: &str, generation: u32) {
    let mut line = heapless::String::<96>::new();
    if write!(
        line,
        "TRACE {name} OK generation={generation} capacity={CAPACITY}\r\n"
    )
    .is_ok()
    {
        let _ = uart_write_all(uart, line.as_bytes()).await;
    } else {
        write_error(uart, b"TRACE ERROR reason=format_overflow\r\n").await;
    }
}

fn freeze_and_drain() -> Option<(u8, u32, usize)> {
    assert_cpu0();
    let snap = INGRESS.lock(|state| {
        let mut s = state.borrow_mut();
        if s.mode != MODE_FULL {
            s.mode = MODE_FROZEN;
        }
        publish_mode(s.mode);
        (s.mode, s.generation, s.accepted)
    });
    drain_pending_cpu0();
    COLLECTOR.lock(|slot| {
        if let Some(c) = slot.borrow_mut().as_mut() {
            c.freeze();
        }
    });
    Some(snap)
}
async fn stop(uart: &mut SerialWriter) {
    let Some((_, g, n)) = freeze_and_drain() else {
        return;
    };
    let mut line = heapless::String::<96>::new();
    let _ = write!(line, "TRACE STOP OK generation={g} count={n}\r\n");
    let _ = uart_write_all(uart, line.as_bytes()).await;
}
async fn status(uart: &mut SerialWriter) {
    let (mode, g, n) = INGRESS.lock(|s| {
        let s = s.borrow();
        (s.mode, s.generation, s.accepted)
    });
    let state = match mode {
        MODE_ARMED => "armed",
        MODE_CAPTURING => "capturing",
        MODE_FULL => "full",
        _ => "frozen",
    };
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        line,
        "TRACE STATUS state={state} generation={g} count={n}\r\n"
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn selftest(uart: &mut SerialWriter) {
    let first = capture_started_record();
    let Ok(g) = reset_capture(MODE_CAPTURING, true, Some(first)) else {
        write_error(uart, b"TRACE ERROR reason=not_initialized\r\n").await;
        return;
    };
    PROBE_DONE.reset();
    PROBE_REQUEST.signal(());
    for index in 0..16_u64 {
        probe_event(0, index);
        embassy_time::Timer::after_millis(2).await;
        drain_pending_cpu0();
    }
    if embassy_time::with_timeout(embassy_time::Duration::from_secs(2), PROBE_DONE.wait())
        .await
        .is_err()
    {
        freeze_and_drain();
        write_error(uart, b"TRACE ERROR reason=selftest_timeout\r\n").await;
        return;
    }
    freeze_and_drain();
    let mut line = heapless::String::<96>::new();
    let _ = write!(line, "TRACE SELFTEST OK generation={g}\r\n");
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(crate) fn physical_touch_sample(source_ms: u64, count: u8) {
    let contact = count > 0;
    let prior = PHYSICAL_TOUCH_CONTACT.swap(contact, Ordering::Relaxed);
    if contact && !prior {
        trigger(1, source_ms, false)
    }
}
pub(crate) fn trigger_wake(source_ms: u64) {
    trigger(2, source_ms, true)
}
fn trigger(source: u64, source_ms: u64, wake: bool) {
    if MODE.load(Ordering::Acquire) != MODE_ARMED {
        return;
    }
    let timestamp = clock();
    let context = context();
    let mut signal = false;
    INGRESS.lock(|slot| {
        let mut s = slot.borrow_mut();
        if s.mode != MODE_ARMED {
            return;
        }
        s.mode = MODE_CAPTURING;
        publish_mode(MODE_CAPTURING);
        signal |= enqueue_locked(
            &mut s,
            marker(timestamp, context, "capture_triggered", source, source_ms),
        );
        if wake {
            signal |= enqueue_locked(
                &mut s,
                marker(timestamp, context, "wake_pressed", 2, source_ms),
            );
        }
    });
    if signal {
        DRAIN_WAKE.signal(())
    }
}
fn marker(
    timestamp: u64,
    context: ExecutionContext,
    name: &'static str,
    source: u64,
    source_ms: u64,
) -> EventRecord {
    let mut fields = heapless::Vec::new();
    let _ = fields.push(EventField {
        name: "source",
        value: FieldValue::U64(source),
    });
    let _ = fields.push(EventField {
        name: "source_ms",
        value: FieldValue::U64(source_ms),
    });
    EventRecord {
        timestamp,
        context,
        name,
        target: "firmware",
        level: tracing::Level::INFO,
        parent: None,
        fields,
        omitted_fields: 0,
    }
}

async fn dump(uart: &mut SerialWriter) {
    let Some((mode, g, _)) = freeze_and_drain() else {
        return;
    };
    let (stored, losses) = COLLECTOR
        .lock(|slot| {
            let slot = slot.borrow();
            let c = slot.as_ref()?;
            let count = c.records().len();
            let losses = c.losses();
            Some((count, losses))
        })
        .unwrap_or_default();
    let (fov, unsup, parented, ingress, integrity) = INGRESS.lock(|s| {
        let s = s.borrow();
        (
            s.field_overflow,
            s.unsupported,
            s.parented,
            s.ingress_overflow,
            s.integrity_errors,
        )
    });
    let mut format_errors = 0_u64;
    for i in 0..stored {
        if record_at(i).is_none_or(|r| format_event(&r, i).is_err()) {
            format_errors += 1
        }
    }
    let count = stored.saturating_sub(format_errors as usize);
    let mut line = heapless::String::<256>::new();
    let _=write!(line,"TRACE BEGIN version=1 generation={g} count={count} overflow={} unsupported={unsup} hook_failures={} field_overflow={fov} parented={parented} formatting_errors={format_errors} buffer_full={} ingress_overflow={ingress} integrity_errors={integrity}\r\n",losses.records,hook_failures(),u8::from(mode==MODE_FULL));
    let _ = uart_write_all(uart, line.as_bytes()).await;
    let mut seq = 0;
    for i in 0..stored {
        if let Some(r) = record_at(i) {
            match format_event(&r, seq) {
                Ok(v) => {
                    let _ = uart_write_all(uart, v.as_bytes()).await;
                    seq += 1
                }
                Err(_) => {
                    let _ = uart_write_all(uart, b"TRACE ERROR reason=format_overflow\r\n").await;
                }
            }
        }
    }
    line.clear();
    let _ = write!(line, "TRACE END generation={g}\r\n");
    let _ = uart_write_all(uart, line.as_bytes()).await;
}
fn record_at(index: usize) -> Option<EventRecord> {
    assert_cpu0();
    COLLECTOR.lock(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|c| c.records().nth(index).cloned())
    })
}
fn format_event(r: &EventRecord, seq: usize) -> Result<heapless::String<WIRE_LINE_CAPACITY>, ()> {
    let mut l = heapless::String::new();
    write!(
        l,
        "TRACE EVENT seq={seq} ts={} core={} context_kind={} context_id={} target=",
        r.timestamp, r.context.core, r.context.kind, r.context.id
    )
    .map_err(drop)?;
    encode(&mut l, r.target)?;
    l.push_str(" name=").map_err(drop)?;
    encode(&mut l, r.name)?;
    for f in &r.fields {
        l.push_str(" f.").map_err(drop)?;
        encode(&mut l, f.name)?;
        match f.value {
            FieldValue::U64(v) => write!(l, "=u:{v}").map_err(drop)?,
            FieldValue::I64(v) => write!(l, "=i:{v}").map_err(drop)?,
            FieldValue::Bool(v) => write!(l, "=b:{}", u8::from(v)).map_err(drop)?,
            FieldValue::F64(v) if v.is_finite() => write!(l, "=f:{v}").map_err(drop)?,
            FieldValue::F64(_) => return Err(()),
        }
    }
    l.push_str("\r\n").map_err(drop)?;
    Ok(l)
}
fn encode<const N: usize>(out: &mut heapless::String<N>, v: &str) -> Result<(), ()> {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b)).map_err(drop)?
        } else {
            out.push('%').map_err(drop)?;
            out.push(char::from(H[(b >> 4) as usize])).map_err(drop)?;
            out.push(char::from(H[(b & 15) as usize])).map_err(drop)?
        }
    }
    Ok(())
}
#[cfg(feature = "ui-interaction-trace")]
fn hook_failures() -> u32 {
    render::interaction_trace::failures()
}
#[cfg(not(feature = "ui-interaction-trace"))]
fn hook_failures() -> u32 {
    0
}
async fn write_error(uart: &mut SerialWriter, msg: &[u8]) {
    let _ = uart_write_all(uart, msg).await;
}

// Sixteen complete ingress records plus state stay below 6 KiB internal DRAM.
const _: () = assert!(core::mem::size_of::<Ingress>() < 6 * 1024);
