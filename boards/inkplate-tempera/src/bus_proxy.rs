//! Owned, bounded transactions for a CPU-local I2C owner.
//!
//! One caller is admitted at a time. Cancellation before the owner's atomic
//! claim prevents execution; after that claim, the transfer may have effects.
//! The owner completes or cancels that transfer before receiving another one.
use core::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};
use embassy_sync::{
    blocking_mutex::{raw::CriticalSectionRawMutex, Mutex as BlockingMutex},
    channel::Channel,
    mutex::Mutex,
    signal::Signal,
};
use embassy_time::{with_deadline, Duration, Instant};
use embedded_hal_async::i2c::{ErrorType, I2c, Operation};

const CAPACITY: usize = 64;
const OPERATIONS: usize = 4;
const CLAIMED: u32 = 1 << 31;
const QUEUE_DEADLINE: Duration = Duration::from_millis(40);
const TRANSFER_DEADLINE: Duration = Duration::from_millis(40);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutPhase {
    ClientLock,
    Publish,
    OwnerClaim,
    TransferReply,
}

impl TimeoutPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClientLock => "client_lock",
            Self::Publish => "publish",
            Self::OwnerClaim => "owner_claim",
            Self::TransferReply => "transfer_reply",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerPhase {
    NotStarted,
    Waiting,
    Executing,
}

impl OwnerPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Waiting => "waiting",
            Self::Executing => "executing",
        }
    }
}

#[derive(Clone, Copy)]
struct OwnerState {
    phase: OwnerPhase,
    since: Option<Instant>,
    id: u32,
    address: u8,
    received: u32,
    discarded: u32,
}

/// Failure metadata sampled before calling the sink; it contains no payload.
#[derive(Clone, Copy, Debug)]
pub struct TimeoutDiagnostic {
    pub phase: TimeoutPhase,
    pub id: u32,
    pub address: u8,
    pub elapsed_us: u32,
    pub claimed: bool,
    pub owner_phase: OwnerPhase,
    pub owner_id: u32,
    pub owner_address: u8,
    pub owner_age_us: u32,
    pub received: u32,
    pub discarded: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusRate {
    Standard,
    Panel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProxyError<E> {
    Bus(E),
    Capacity,
    IdentityExhausted,
    /// No transaction was claimed by the owner.
    QueueTimeout,
    /// The owner claimed the transaction: writes may have taken effect.
    TransferTimeout,
}

impl<E: embedded_hal::i2c::Error> embedded_hal::i2c::Error for ProxyError<E> {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match self {
            Self::Bus(error) => error.kind(),
            _ => embedded_hal::i2c::ErrorKind::Other,
        }
    }
}

/// Implement beside the HAL; configuration and the entire transaction share
/// one owner-local bus lock. HAL handles never enter the message transport.
#[allow(async_fn_in_trait)]
pub trait OwnerBus {
    type Error;
    async fn transaction(
        &mut self,
        rate: BusRate,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Default)]
struct Segment {
    read: bool,
    len: u8,
}
struct Request {
    id: u32,
    expires: Instant,
    rate: BusRate,
    address: u8,
    segments: [Segment; OPERATIONS],
    count: usize,
    data: [u8; CAPACITY],
}
struct Reply<E> {
    id: u32,
    data: [u8; CAPACITY],
    result: Result<(), ProxyError<E>>,
}

pub struct BusProxy<E> {
    requests: Channel<CriticalSectionRawMutex, Request, 1>,
    replies: Signal<CriticalSectionRawMutex, Reply<E>>,
    client: Mutex<CriticalSectionRawMutex, ()>,
    sequence: AtomicU32,
    active: AtomicU32,
    queue_max_us: AtomicU32,
    owner: BlockingMutex<CriticalSectionRawMutex, Cell<OwnerState>>,
    timeout_sink: Option<fn(TimeoutDiagnostic)>,
}

impl<E> Default for BusProxy<E> {
    fn default() -> Self {
        Self::new()
    }
}
impl<E> BusProxy<E> {
    pub const fn new() -> Self {
        Self::with_timeout_sink(None)
    }

    pub const fn with_timeout_sink(timeout_sink: Option<fn(TimeoutDiagnostic)>) -> Self {
        Self {
            requests: Channel::new(),
            replies: Signal::new(),
            client: Mutex::new(()),
            sequence: AtomicU32::new(0),
            active: AtomicU32::new(0),
            queue_max_us: AtomicU32::new(0),
            owner: BlockingMutex::new(Cell::new(OwnerState {
                phase: OwnerPhase::NotStarted,
                since: None,
                id: 0,
                address: 0xff,
                received: 0,
                discarded: 0,
            })),
            timeout_sink,
        }
    }

    fn timeout(&self, phase: TimeoutPhase, id: u32, address: u8, started: Instant, claimed: bool) {
        if let Some(sink) = self.timeout_sink {
            let (owner, now) = self.owner.lock(|state| (state.get(), Instant::now()));
            sink(TimeoutDiagnostic {
                phase,
                id,
                address,
                elapsed_us: now
                    .saturating_duration_since(started)
                    .as_micros()
                    .min(u32::MAX as u64) as u32,
                claimed,
                owner_phase: owner.phase,
                owner_id: owner.id,
                owner_address: owner.address,
                owner_age_us: owner.since.map_or(0, |since| {
                    now.saturating_duration_since(since)
                        .as_micros()
                        .min(u32::MAX as u64) as u32
                }),
                received: owner.received,
                discarded: owner.discarded,
            });
        }
    }

    pub fn queue_max_us(&self) -> u32 {
        self.queue_max_us.load(Ordering::Relaxed)
    }
    pub fn reset_queue_timing(&self) {
        self.queue_max_us.store(0, Ordering::Relaxed);
    }

    pub fn device(&self, rate: BusRate) -> BusDevice<'_, E> {
        BusDevice { proxy: self, rate }
    }

    pub async fn serve(&self, bus: &mut impl OwnerBus<Error = E>) -> ! {
        loop {
            self.owner.lock(|state| {
                let mut owner = state.get();
                owner.phase = OwnerPhase::Waiting;
                owner.since = Some(Instant::now());
                state.set(owner);
            });
            let request = self.requests.receive().await;
            self.owner.lock(|state| {
                let mut owner = state.get();
                owner.received = owner.received.wrapping_add(1);
                state.set(owner);
            });
            // Atomic claim is the queued/in-flight boundary. Cancellation
            // racing this point either prevents the claim or becomes an
            // uncertain in-flight operation; it cannot silently become queued.
            if Instant::now() >= request.expires
                || self
                    .active
                    .compare_exchange(
                        request.id,
                        request.id | CLAIMED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
            {
                self.owner.lock(|state| {
                    let mut owner = state.get();
                    owner.discarded = owner.discarded.wrapping_add(1);
                    state.set(owner);
                });
                continue;
            }
            self.owner.lock(|state| {
                let mut owner = state.get();
                owner.phase = OwnerPhase::Executing;
                owner.since = Some(Instant::now());
                owner.id = request.id;
                owner.address = request.address;
                state.set(owner);
            });
            self.queue_max_us.fetch_max(
                Instant::now()
                    .saturating_duration_since(request.expires - QUEUE_DEADLINE)
                    .as_micros()
                    .min(u32::MAX as u64) as u32,
                Ordering::Relaxed,
            );
            let reply = execute(request, bus).await;
            // An abandoned caller must never backpressure the owner.
            self.replies.signal(reply);
        }
    }
}

async fn execute<E>(mut request: Request, bus: &mut impl OwnerBus<Error = E>) -> Reply<E> {
    let result = {
        let mut remaining = request.data.as_mut_slice();
        let mut operations = core::array::from_fn::<_, OPERATIONS, _>(|_| Operation::Write(&[]));
        for (operation, segment) in operations
            .iter_mut()
            .zip(&request.segments)
            .take(request.count)
        {
            let (data, rest) = remaining.split_at_mut(segment.len as usize);
            remaining = rest;
            *operation = if segment.read {
                Operation::Read(data)
            } else {
                Operation::Write(data)
            };
        }
        with_deadline(
            Instant::now() + TRANSFER_DEADLINE,
            bus.transaction(
                request.rate,
                request.address,
                &mut operations[..request.count],
            ),
        )
        .await
    };
    Reply {
        id: request.id,
        data: request.data,
        result: match result {
            Ok(result) => result.map_err(ProxyError::Bus),
            Err(_) => Err(ProxyError::TransferTimeout),
        },
    }
}

struct Active<'a>(&'a AtomicU32);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        // The client mutex remains held until this guard is dropped. A newer
        // caller cannot have published its identity yet.
        self.0.store(0, Ordering::Release);
    }
}

pub struct BusDevice<'a, E> {
    proxy: &'a BusProxy<E>,
    rate: BusRate,
}
impl<E: embedded_hal::i2c::Error> ErrorType for BusDevice<'_, E> {
    type Error = ProxyError<E>;
}
impl<E: embedded_hal::i2c::Error> I2c for BusDevice<'_, E> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() > OPERATIONS {
            return Err(ProxyError::Capacity);
        }
        let started = Instant::now();
        let expires = started + QUEUE_DEADLINE;
        let mut request = Request {
            id: 0,
            expires,
            rate: self.rate,
            address,
            segments: [Segment::default(); OPERATIONS],
            count: operations.len(),
            data: [0; CAPACITY],
        };
        let mut offset = 0usize;
        for (segment, operation) in request.segments.iter_mut().zip(operations.iter()) {
            let (read, data): (_, &[u8]) = match operation {
                Operation::Write(data) => (false, data),
                Operation::Read(data) => (true, data),
            };
            if data.len() > CAPACITY - offset {
                return Err(ProxyError::Capacity);
            }
            *segment = Segment {
                read,
                len: data.len() as u8,
            };
            if !read {
                request.data[offset..offset + data.len()].copy_from_slice(data);
            }
            offset += data.len();
        }
        let _client = with_deadline(expires, self.proxy.client.lock())
            .await
            .map_err(|_| {
                self.proxy
                    .timeout(TimeoutPhase::ClientLock, 0, address, started, false);
                ProxyError::QueueTimeout
            })?;
        let id = self
            .proxy
            .sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                (id < CLAIMED - 1).then_some(id + 1)
            })
            .map_err(|_| ProxyError::IdentityExhausted)?
            + 1;
        request.id = id;
        self.proxy.active.store(id, Ordering::Release);
        let _active = Active(&self.proxy.active);
        with_deadline(expires, self.proxy.requests.send(request))
            .await
            .map_err(|_| {
                self.proxy
                    .timeout(TimeoutPhase::Publish, id, address, started, false);
                ProxyError::QueueTimeout
            })?;
        let mut deadline = expires;
        loop {
            match with_deadline(deadline, self.proxy.replies.wait()).await {
                Ok(reply) if reply.id == id => {
                    reply.result.inspect_err(|error| {
                        if matches!(error, ProxyError::TransferTimeout) {
                            self.proxy.timeout(
                                TimeoutPhase::TransferReply,
                                id,
                                address,
                                started,
                                true,
                            );
                        }
                    })?;
                    let mut offset = 0;
                    for operation in operations {
                        let len = match operation {
                            Operation::Read(data) => {
                                data.copy_from_slice(&reply.data[offset..offset + data.len()]);
                                data.len()
                            }
                            Operation::Write(data) => data.len(),
                        };
                        offset += len;
                    }
                    return Ok(());
                }
                Ok(_) => continue,
                Err(_) if deadline == expires => {
                    // Atomically cancel queued work before reporting a safe
                    // timeout; a concurrent owner claim changes the outcome.
                    if self
                        .proxy
                        .active
                        .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        self.proxy
                            .timeout(TimeoutPhase::OwnerClaim, id, address, started, false);
                        return Err(ProxyError::QueueTimeout);
                    }
                    deadline = expires + TRANSFER_DEADLINE;
                }
                Err(_) => {
                    self.proxy
                        .timeout(TimeoutPhase::TransferReply, id, address, started, true);
                    return Err(ProxyError::TransferTimeout);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
