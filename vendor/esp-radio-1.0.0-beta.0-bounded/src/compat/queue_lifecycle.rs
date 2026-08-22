use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

#[cfg(not(any(test, feature = "queue-host-harness")))]
use crate::preempt::queue::{QueueHandle, QueuePtr};

const SLOT_COUNT: usize = 8;
const OPERATION_SLOT_COUNT: usize = 16;
const TASK_SLOT_COUNT: usize = 4;
const EMPTY: u8 = 0;
const ACTIVE: u8 = 1;
const RETIRED: u8 = 2;
const CLAIMING: u8 = 3;
const TASK_OPERATION: u8 = 4;
const ISR_OPERATION: u8 = 5;
const COMPLETING_OPERATION: u8 = 6;
const OPERATION_STATE_BITS: usize = 3;
const OPERATION_STATE_MASK: usize = (1 << OPERATION_STATE_BITS) - 1;
const OPERATION_GENERATION_MAX: usize = usize::MAX >> OPERATION_STATE_BITS;
const TASK_DELETE_REQUESTED: u8 = 2;
const TASK_DELETED: u8 = 3;

struct QueueSlot {
    pointer: AtomicUsize,
    state: AtomicU8,
    in_flight: AtomicUsize,
    payload_bytes: AtomicUsize,
    owner_epoch: AtomicUsize,
}

impl QueueSlot {
    const fn new() -> Self {
        Self {
            pointer: AtomicUsize::new(0),
            state: AtomicU8::new(EMPTY),
            in_flight: AtomicUsize::new(0),
            payload_bytes: AtomicUsize::new(0),
            owner_epoch: AtomicUsize::new(0),
        }
    }
}

static QUEUES: [QueueSlot; SLOT_COUNT] = [const { QueueSlot::new() }; SLOT_COUNT];

struct OperationSlot {
    token: AtomicUsize,
    queue_slot: AtomicU8,
    owner_task: AtomicUsize,
    owner_epoch: AtomicUsize,
}

impl OperationSlot {
    const fn new() -> Self {
        Self {
            token: AtomicUsize::new(0),
            queue_slot: AtomicU8::new(0),
            owner_task: AtomicUsize::new(0),
            owner_epoch: AtomicUsize::new(0),
        }
    }
}

struct TaskSlot {
    pointer: AtomicUsize,
    state: AtomicU8,
    owner_epoch: AtomicUsize,
}

impl TaskSlot {
    const fn new() -> Self {
        Self {
            pointer: AtomicUsize::new(0),
            state: AtomicU8::new(EMPTY),
            owner_epoch: AtomicUsize::new(0),
        }
    }
}

static OPERATIONS: [OperationSlot; OPERATION_SLOT_COUNT] =
    [const { OperationSlot::new() }; OPERATION_SLOT_COUNT];
static BTDM_TASKS: [TaskSlot; TASK_SLOT_COUNT] = [const { TaskSlot::new() }; TASK_SLOT_COUNT];
static CURRENT_RECLAIMABLE_EPOCH: AtomicUsize = AtomicUsize::new(0);
static NEXT_EPOCH: AtomicUsize = AtomicUsize::new(1);
static CREATE_COUNT: AtomicUsize = AtomicUsize::new(0);
static RECLAIM_COUNT: AtomicUsize = AtomicUsize::new(0);
static RECLAIM_FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);
static LATE_USE_REJECTED: AtomicUsize = AtomicUsize::new(0);
static UNKNOWN_USE_REJECTED: AtomicUsize = AtomicUsize::new(0);
static DUPLICATE_RETIRE_COUNT: AtomicUsize = AtomicUsize::new(0);
static SLOT_HIGH_WATER: AtomicUsize = AtomicUsize::new(0);
static OPERATION_STARTED: AtomicUsize = AtomicUsize::new(0);
static OPERATION_COMPLETED: AtomicUsize = AtomicUsize::new(0);
static OPERATION_CANCELLED_ON_TASK_DELETE: AtomicUsize = AtomicUsize::new(0);
static OPERATION_REGISTRY_FULL: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_CREATED: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_DELETE_REQUESTED_COUNT: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_DELETED_COUNT: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_LIVE: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_REGISTRY_FAILURES: AtomicUsize = AtomicUsize::new(0);
static BTDM_TASK_DELETE_UNATTRIBUTED: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(any(test, feature = "queue-host-harness")))]
static LIFECYCLE_LOCK: esp_sync::RawMutex = esp_sync::RawMutex::new();

#[cfg(not(any(test, feature = "queue-host-harness")))]
fn with_lifecycle_lock<R>(f: impl FnOnce() -> R) -> R {
    LIFECYCLE_LOCK.lock(f)
}

#[cfg(any(test, feature = "queue-host-harness"))]
fn with_lifecycle_lock<R>(f: impl FnOnce() -> R) -> R {
    f()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegisterError {
    NullPointer,
    DuplicatePointer,
    RegistryFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetireResult {
    Retired,
    AlreadyRetired,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReclaimError {
    NoActiveEpoch,
    ActiveQueue,
    OperationInFlight,
    BtdmTaskLive,
    OperationAccounting,
    LowerOwnerRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskRegisterError {
    NullPointer,
    NoActiveEpoch,
    DuplicatePointer,
    RegistryFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskDeleteToken {
    slot: usize,
    pointer: usize,
    epoch: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Snapshot of bounded queue ownership and explicit source-lifetime state.
pub struct QueueLifecycleStats {
    /// Queue registrations since boot.
    pub created: usize,
    /// Registered queues currently admitting operations.
    pub active: usize,
    /// Queues retired by their owner but not yet reclaimed.
    pub retired: usize,
    /// Queues reclaimed after source quiescence.
    pub reclaimed: usize,
    /// Operations currently holding an admission guard.
    pub in_flight: usize,
    /// Payload bytes owned by active queues.
    pub active_payload_bytes: usize,
    /// Payload bytes owned by retired queues.
    pub retired_payload_bytes: usize,
    /// Operations rejected after queue retirement.
    pub late_use_rejected: usize,
    /// Operations rejected for an unknown pointer.
    pub unknown_use_rejected: usize,
    /// Duplicate or unknown retire requests.
    pub duplicate_retire: usize,
    /// Current source-scoped reclaimable epoch, or zero.
    pub active_reclaimable_epoch: usize,
    /// Failed explicit reclamation attempts.
    pub reclaim_failures: usize,
    /// Highest registry slot index used since boot.
    pub slot_high_water: usize,
    /// Compile-time lifecycle registry capacity.
    pub slot_capacity: usize,
    /// Queue operations admitted since boot.
    pub operation_started: usize,
    /// Queue operations whose Rust guard completed normally.
    pub operation_completed: usize,
    /// Task-owned operations cancelled when the owning BTDM task was deleted.
    pub operation_cancelled_on_task_delete: usize,
    /// Queue operations rejected because the bounded operation registry was full.
    pub operation_registry_full: usize,
    /// One when operation totals do not match completed, cancelled, and live operations.
    pub operation_balance_error: usize,
    /// BTDM tasks registered since boot.
    pub btdm_task_created: usize,
    /// BTDM task deletion requests correlated to a registered task.
    pub btdm_task_delete_requested: usize,
    /// BTDM tasks whose deletion boundary completed.
    pub btdm_task_deleted: usize,
    /// Registered BTDM tasks still live in the active queue epoch.
    pub btdm_task_live: usize,
    /// BTDM task registrations rejected by the bounded registry.
    pub btdm_task_registry_failures: usize,
    /// Task deletions that could not be correlated to the active epoch.
    pub btdm_task_delete_unattributed: usize,
    /// Lower fixed-owner queue creations since boot.
    pub owner_created: usize,
    /// Lower fixed-owner queue retirements since boot.
    pub owner_deleted: usize,
    /// Lower fixed-owner active slots.
    pub owner_active: usize,
    /// Lower fixed-owner retired slots.
    pub owner_retired: usize,
    /// Lower fixed-owner reclaimed slots since boot.
    pub owner_reclaimed: usize,
    /// Detected lower control/storage canary failures.
    pub owner_corruption: usize,
    /// Task-side queue operations rejected by same-core nested use.
    pub owner_task_contention_rejected: usize,
    /// ISR-side queue operations rejected by same-core nested use.
    pub owner_isr_contention_rejected: usize,
    /// Nominally blocking operations redirected to one nonblocking attempt.
    pub owner_nonblocking_context_redirected: usize,
    /// Payload bytes currently reserved by lower owner slots.
    pub owner_payload_bytes: usize,
    /// Compile-time aggregate lower payload ceiling.
    pub owner_payload_capacity_bytes: usize,
    /// Highest lower fixed-owner slot index used since boot.
    pub owner_slot_high_water: usize,
    /// Compile-time lower fixed-owner capacity.
    pub owner_slot_capacity: usize,
}

pub(crate) struct QueueUseGuard {
    operation: usize,
    active_token: usize,
}

impl Drop for QueueUseGuard {
    fn drop(&mut self) {
        with_lifecycle_lock(|| {
            complete_operation_locked(self.operation, self.active_token, false);
        });
    }
}

fn update_high_water(value: usize) {
    let mut high = SLOT_HIGH_WATER.load(Ordering::Relaxed);
    while value > high {
        match SLOT_HIGH_WATER.compare_exchange_weak(
            high,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => high = observed,
        }
    }
}

#[cfg(not(any(test, feature = "queue-host-harness")))]
fn retire_lower_owner(pointer: usize) {
    let ptr = QueuePtr::new(pointer as *mut ()).expect("registered queue pointer is null");
    core::mem::drop(unsafe { QueueHandle::from_ptr(ptr) });
}

#[cfg(any(test, feature = "queue-host-harness"))]
fn retire_lower_owner(_pointer: usize) {}

#[cfg(not(any(test, feature = "queue-host-harness")))]
fn reclaim_lower_owner(pointer: usize) -> bool {
    let ptr = QueuePtr::new(pointer as *mut ()).expect("registered queue pointer is null");
    unsafe { esp_radio_rtos_driver::queue::compat_queue_reclaim(ptr) }
}

#[cfg(any(test, feature = "queue-host-harness"))]
fn reclaim_lower_owner(_pointer: usize) -> bool {
    true
}

pub(crate) fn begin_reclaimable_epoch() -> usize {
    let epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);
    assert_ne!(epoch, 0, "queue owner epoch overflow");
    CURRENT_RECLAIMABLE_EPOCH
        .compare_exchange(0, epoch, Ordering::AcqRel, Ordering::Acquire)
        .expect("queue owner epoch already active");
    epoch
}

pub(crate) fn record_source_quiescence_failure() {
    RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn register(pointer: usize, payload_bytes: usize) -> Result<(), RegisterError> {
    if pointer == 0 {
        return Err(RegisterError::NullPointer);
    }
    if QUEUES
        .iter()
        .any(|slot| slot.pointer.load(Ordering::Acquire) == pointer)
    {
        return Err(RegisterError::DuplicatePointer);
    }

    for (index, slot) in QUEUES.iter().enumerate() {
        if slot
            .pointer
            .compare_exchange(0, pointer, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            slot.payload_bytes.store(payload_bytes, Ordering::Relaxed);
            slot.owner_epoch.store(
                CURRENT_RECLAIMABLE_EPOCH.load(Ordering::Acquire),
                Ordering::Relaxed,
            );
            slot.in_flight.store(0, Ordering::Relaxed);
            slot.state.store(ACTIVE, Ordering::Release);
            CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
            update_high_water(index + 1);
            return Ok(());
        }
    }
    Err(RegisterError::RegistryFull)
}

const fn operation_state(token: usize) -> u8 {
    (token & OPERATION_STATE_MASK) as u8
}

const fn operation_generation(token: usize) -> usize {
    token >> OPERATION_STATE_BITS
}

const fn operation_token(generation: usize, state: u8) -> usize {
    (generation << OPERATION_STATE_BITS) | state as usize
}

fn claim_operation(record: &OperationSlot) -> Option<usize> {
    let observed = record.token.load(Ordering::Acquire);
    if operation_state(observed) != EMPTY {
        return None;
    }
    let generation = operation_generation(observed).wrapping_add(1) & OPERATION_GENERATION_MAX;
    assert_ne!(generation, 0, "queue operation generation overflow");
    let claiming = operation_token(generation, CLAIMING);
    record
        .token
        .compare_exchange(observed, claiming, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    Some(generation)
}

fn complete_operation_locked(
    operation: usize,
    active_token: usize,
    cancelled_on_task_delete: bool,
) -> bool {
    let record = &OPERATIONS[operation];
    let generation = operation_generation(active_token);
    if record
        .token
        .compare_exchange(
            active_token,
            operation_token(generation, COMPLETING_OPERATION),
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return false;
    }

    let queue_slot = record.queue_slot.load(Ordering::Relaxed) as usize;
    if cancelled_on_task_delete {
        OPERATION_CANCELLED_ON_TASK_DELETE.fetch_add(1, Ordering::Relaxed);
    } else {
        OPERATION_COMPLETED.fetch_add(1, Ordering::Relaxed);
    }
    // The in-flight decrement is the quiescence publication. An Acquire load
    // that observes zero must also observe the accounting update above.
    QUEUES[queue_slot].in_flight.fetch_sub(1, Ordering::AcqRel);
    record
        .token
        .store(operation_token(generation, EMPTY), Ordering::Release);
    true
}

fn begin_use_with_owner(
    pointer: usize,
    owner_task: usize,
    operation_kind: u8,
) -> Option<QueueUseGuard> {
    let Some((index, slot)) = QUEUES
        .iter()
        .enumerate()
        .find(|(_, slot)| slot.pointer.load(Ordering::Acquire) == pointer)
    else {
        UNKNOWN_USE_REJECTED.fetch_add(1, Ordering::Relaxed);
        return None;
    };

    if slot.state.load(Ordering::Acquire) != ACTIVE {
        LATE_USE_REJECTED.fetch_add(1, Ordering::Relaxed);
        return None;
    }

    let epoch = slot.owner_epoch.load(Ordering::Acquire);
    let Some((operation, record, generation)) =
        OPERATIONS
            .iter()
            .enumerate()
            .find_map(|(operation, record)| {
                claim_operation(record).map(|generation| (operation, record, generation))
            })
    else {
        OPERATION_REGISTRY_FULL.fetch_add(1, Ordering::Relaxed);
        return None;
    };

    record.queue_slot.store(index as u8, Ordering::Relaxed);
    record.owner_task.store(owner_task, Ordering::Relaxed);
    record.owner_epoch.store(epoch, Ordering::Relaxed);
    slot.in_flight.fetch_add(1, Ordering::AcqRel);
    OPERATION_STARTED.fetch_add(1, Ordering::Relaxed);
    let active_token = operation_token(generation, operation_kind);
    record.token.store(active_token, Ordering::Release);

    if slot.state.load(Ordering::Acquire) != ACTIVE {
        complete_operation_locked(operation, active_token, false);
        LATE_USE_REJECTED.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    Some(QueueUseGuard {
        operation,
        active_token,
    })
}

#[cfg(not(any(test, feature = "queue-host-harness")))]
pub(crate) fn begin_task_use(pointer: usize) -> Option<QueueUseGuard> {
    with_lifecycle_lock(|| {
        let owner = crate::preempt::current_task().as_ptr() as usize;
        if BTDM_TASKS.iter().any(|task| {
            task.pointer.load(Ordering::Acquire) == owner
                && task.state.load(Ordering::Acquire) != ACTIVE
        }) {
            LATE_USE_REJECTED.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        begin_use_with_owner(pointer, owner, TASK_OPERATION)
    })
}

#[cfg(any(test, feature = "queue-host-harness"))]
pub(crate) fn begin_task_use(pointer: usize) -> Option<QueueUseGuard> {
    with_lifecycle_lock(|| begin_use_with_owner(pointer, 1, TASK_OPERATION))
}

pub(crate) fn begin_isr_use(pointer: usize) -> Option<QueueUseGuard> {
    with_lifecycle_lock(|| begin_use_with_owner(pointer, 0, ISR_OPERATION))
}

pub(crate) fn register_btdm_task(pointer: usize) -> Result<(), TaskRegisterError> {
    with_lifecycle_lock(|| register_btdm_task_locked(pointer))
}

fn register_btdm_task_locked(pointer: usize) -> Result<(), TaskRegisterError> {
    if pointer == 0 {
        BTDM_TASK_REGISTRY_FAILURES.fetch_add(1, Ordering::Relaxed);
        return Err(TaskRegisterError::NullPointer);
    }
    let epoch = CURRENT_RECLAIMABLE_EPOCH.load(Ordering::Acquire);
    if epoch == 0 {
        BTDM_TASK_REGISTRY_FAILURES.fetch_add(1, Ordering::Relaxed);
        return Err(TaskRegisterError::NoActiveEpoch);
    }
    if BTDM_TASKS.iter().any(|task| {
        task.pointer.load(Ordering::Acquire) == pointer
            && task.state.load(Ordering::Acquire) != EMPTY
    }) {
        BTDM_TASK_REGISTRY_FAILURES.fetch_add(1, Ordering::Relaxed);
        return Err(TaskRegisterError::DuplicatePointer);
    }

    for task in &BTDM_TASKS {
        if task
            .pointer
            .compare_exchange(0, pointer, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            task.owner_epoch.store(epoch, Ordering::Relaxed);
            task.state.store(ACTIVE, Ordering::Release);
            BTDM_TASK_CREATED.fetch_add(1, Ordering::Relaxed);
            BTDM_TASK_LIVE.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    }

    BTDM_TASK_REGISTRY_FAILURES.fetch_add(1, Ordering::Relaxed);
    Err(TaskRegisterError::RegistryFull)
}

pub(crate) fn record_btdm_task_registry_failure() {
    BTDM_TASK_REGISTRY_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn prepare_btdm_task_delete(pointer: usize) -> Option<TaskDeleteToken> {
    with_lifecycle_lock(|| prepare_btdm_task_delete_locked(pointer))
}

fn prepare_btdm_task_delete_locked(pointer: usize) -> Option<TaskDeleteToken> {
    let Some((slot, task)) = BTDM_TASKS.iter().enumerate().find(|(_, task)| {
        task.pointer.load(Ordering::Acquire) == pointer
            && task.state.load(Ordering::Acquire) == ACTIVE
    }) else {
        BTDM_TASK_DELETE_UNATTRIBUTED.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    if task
        .state
        .compare_exchange(
            ACTIVE,
            TASK_DELETE_REQUESTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        BTDM_TASK_DELETE_UNATTRIBUTED.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    BTDM_TASK_DELETE_REQUESTED_COUNT.fetch_add(1, Ordering::Relaxed);
    Some(TaskDeleteToken {
        slot,
        pointer,
        epoch: task.owner_epoch.load(Ordering::Acquire),
    })
}

pub(crate) fn complete_btdm_task_delete(token: TaskDeleteToken) {
    with_lifecycle_lock(|| complete_btdm_task_delete_locked(token));
}

fn complete_btdm_task_delete_locked(token: TaskDeleteToken) {
    for (operation, record) in OPERATIONS.iter().enumerate() {
        let active_token = record.token.load(Ordering::Acquire);
        if record.owner_task.load(Ordering::Acquire) != token.pointer
            || record.owner_epoch.load(Ordering::Acquire) != token.epoch
            || operation_state(active_token) != TASK_OPERATION
        {
            continue;
        }
        complete_operation_locked(operation, active_token, true);
    }

    let task = &BTDM_TASKS[token.slot];
    task.state.store(TASK_DELETED, Ordering::Release);
    BTDM_TASK_LIVE.fetch_sub(1, Ordering::AcqRel);
    BTDM_TASK_DELETED_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn retire(pointer: usize) -> RetireResult {
    let Some(slot) = QUEUES
        .iter()
        .find(|slot| slot.pointer.load(Ordering::Acquire) == pointer)
    else {
        DUPLICATE_RETIRE_COUNT.fetch_add(1, Ordering::Relaxed);
        return RetireResult::Unknown;
    };

    match slot
        .state
        .compare_exchange(ACTIVE, RETIRED, Ordering::AcqRel, Ordering::Acquire)
    {
        Ok(_) => {
            retire_lower_owner(pointer);
            RetireResult::Retired
        }
        Err(RETIRED) => {
            DUPLICATE_RETIRE_COUNT.fetch_add(1, Ordering::Relaxed);
            RetireResult::AlreadyRetired
        }
        Err(_) => {
            DUPLICATE_RETIRE_COUNT.fetch_add(1, Ordering::Relaxed);
            RetireResult::Unknown
        }
    }
}

/// Reclaim queues owned by the current epoch after the callback source is disabled and quiescent.
pub(crate) fn reclaim_current_epoch_after_source_quiescent() -> Result<usize, ReclaimError> {
    let epoch = CURRENT_RECLAIMABLE_EPOCH.load(Ordering::Acquire);
    if epoch == 0 {
        return Err(ReclaimError::NoActiveEpoch);
    }

    if BTDM_TASKS.iter().any(|task| {
        task.owner_epoch.load(Ordering::Acquire) == epoch
            && task.state.load(Ordering::Acquire) != TASK_DELETED
    }) {
        RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(ReclaimError::BtdmTaskLive);
    }

    for slot in &QUEUES {
        if slot.owner_epoch.load(Ordering::Acquire) != epoch {
            continue;
        }
        if slot.state.load(Ordering::Acquire) != RETIRED {
            RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return Err(ReclaimError::ActiveQueue);
        }
        if slot.in_flight.load(Ordering::Acquire) != 0 {
            RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return Err(ReclaimError::OperationInFlight);
        }
    }

    let started = OPERATION_STARTED.load(Ordering::Acquire);
    let accounted = OPERATION_COMPLETED
        .load(Ordering::Acquire)
        .saturating_add(OPERATION_CANCELLED_ON_TASK_DELETE.load(Ordering::Acquire));
    if started != accounted {
        RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        return Err(ReclaimError::OperationAccounting);
    }

    let mut reclaimed = 0;
    for slot in &QUEUES {
        if slot.owner_epoch.load(Ordering::Acquire) != epoch {
            continue;
        }
        let pointer = slot.pointer.load(Ordering::Acquire);
        if !reclaim_lower_owner(pointer) {
            RECLAIM_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
            return Err(ReclaimError::LowerOwnerRejected);
        }
        slot.state.store(EMPTY, Ordering::Relaxed);
        slot.in_flight.store(0, Ordering::Relaxed);
        slot.payload_bytes.store(0, Ordering::Relaxed);
        slot.owner_epoch.store(0, Ordering::Relaxed);
        slot.pointer.store(0, Ordering::Release);
        reclaimed += 1;
    }

    CURRENT_RECLAIMABLE_EPOCH
        .compare_exchange(epoch, 0, Ordering::AcqRel, Ordering::Acquire)
        .expect("queue owner epoch changed during reclamation");
    for task in &BTDM_TASKS {
        if task.owner_epoch.load(Ordering::Acquire) == epoch {
            task.state.store(EMPTY, Ordering::Relaxed);
            task.owner_epoch.store(0, Ordering::Relaxed);
            task.pointer.store(0, Ordering::Release);
        }
    }
    RECLAIM_COUNT.fetch_add(reclaimed, Ordering::Relaxed);
    Ok(reclaimed)
}

pub(crate) fn snapshot() -> QueueLifecycleStats {
    let operation_started = OPERATION_STARTED.load(Ordering::Relaxed);
    let operation_completed = OPERATION_COMPLETED.load(Ordering::Relaxed);
    let operation_cancelled_on_task_delete =
        OPERATION_CANCELLED_ON_TASK_DELETE.load(Ordering::Relaxed);
    let mut stats = QueueLifecycleStats {
        created: CREATE_COUNT.load(Ordering::Relaxed),
        reclaimed: RECLAIM_COUNT.load(Ordering::Relaxed),
        late_use_rejected: LATE_USE_REJECTED.load(Ordering::Relaxed),
        unknown_use_rejected: UNKNOWN_USE_REJECTED.load(Ordering::Relaxed),
        duplicate_retire: DUPLICATE_RETIRE_COUNT.load(Ordering::Relaxed),
        active_reclaimable_epoch: CURRENT_RECLAIMABLE_EPOCH.load(Ordering::Acquire),
        reclaim_failures: RECLAIM_FAILURE_COUNT.load(Ordering::Relaxed),
        slot_high_water: SLOT_HIGH_WATER.load(Ordering::Relaxed),
        slot_capacity: SLOT_COUNT,
        operation_started,
        operation_completed,
        operation_cancelled_on_task_delete,
        operation_registry_full: OPERATION_REGISTRY_FULL.load(Ordering::Relaxed),
        operation_balance_error: usize::from(
            operation_started
                != operation_completed
                    .saturating_add(operation_cancelled_on_task_delete)
                    .saturating_add(
                        QUEUES
                            .iter()
                            .map(|slot| slot.in_flight.load(Ordering::Relaxed))
                            .sum(),
                    ),
        ),
        btdm_task_created: BTDM_TASK_CREATED.load(Ordering::Relaxed),
        btdm_task_delete_requested: BTDM_TASK_DELETE_REQUESTED_COUNT.load(Ordering::Relaxed),
        btdm_task_deleted: BTDM_TASK_DELETED_COUNT.load(Ordering::Relaxed),
        btdm_task_live: BTDM_TASK_LIVE.load(Ordering::Acquire),
        btdm_task_registry_failures: BTDM_TASK_REGISTRY_FAILURES.load(Ordering::Relaxed),
        btdm_task_delete_unattributed: BTDM_TASK_DELETE_UNATTRIBUTED.load(Ordering::Relaxed),
        ..QueueLifecycleStats::default()
    };
    #[cfg(not(any(test, feature = "queue-host-harness")))]
    {
        let owner = esp_radio_rtos_driver::queue::compat_queue_pool_stats();
        stats.owner_created = owner.created;
        stats.owner_deleted = owner.deleted;
        stats.owner_active = owner.active;
        stats.owner_retired = owner.retired;
        stats.owner_reclaimed = owner.reclaimed;
        stats.owner_corruption = owner.corruption;
        stats.owner_task_contention_rejected = owner.task_contention_rejected;
        stats.owner_isr_contention_rejected = owner.isr_contention_rejected;
        stats.owner_nonblocking_context_redirected = owner.nonblocking_context_redirected;
        stats.owner_payload_bytes = owner.reserved_payload_bytes;
        stats.owner_payload_capacity_bytes = owner.payload_capacity_bytes;
        stats.owner_slot_high_water = owner.slot_high_water;
        stats.owner_slot_capacity = owner.slot_capacity;
    }
    for slot in &QUEUES {
        match slot.state.load(Ordering::Acquire) {
            ACTIVE => {
                stats.active += 1;
                stats.active_payload_bytes += slot.payload_bytes.load(Ordering::Relaxed);
            }
            RETIRED => {
                stats.retired += 1;
                stats.retired_payload_bytes += slot.payload_bytes.load(Ordering::Relaxed);
            }
            _ => {}
        }
        stats.in_flight += slot.in_flight.load(Ordering::Relaxed);
    }
    stats
}

#[cfg(test)]
fn reset_for_test() {
    for slot in &QUEUES {
        slot.pointer.store(0, Ordering::Relaxed);
        slot.state.store(EMPTY, Ordering::Relaxed);
        slot.in_flight.store(0, Ordering::Relaxed);
        slot.payload_bytes.store(0, Ordering::Relaxed);
        slot.owner_epoch.store(0, Ordering::Relaxed);
    }
    for operation in &OPERATIONS {
        operation.token.store(0, Ordering::Relaxed);
        operation.queue_slot.store(0, Ordering::Relaxed);
        operation.owner_task.store(0, Ordering::Relaxed);
        operation.owner_epoch.store(0, Ordering::Relaxed);
    }
    for task in &BTDM_TASKS {
        task.pointer.store(0, Ordering::Relaxed);
        task.state.store(EMPTY, Ordering::Relaxed);
        task.owner_epoch.store(0, Ordering::Relaxed);
    }
    CURRENT_RECLAIMABLE_EPOCH.store(0, Ordering::Relaxed);
    NEXT_EPOCH.store(1, Ordering::Relaxed);
    CREATE_COUNT.store(0, Ordering::Relaxed);
    RECLAIM_COUNT.store(0, Ordering::Relaxed);
    RECLAIM_FAILURE_COUNT.store(0, Ordering::Relaxed);
    LATE_USE_REJECTED.store(0, Ordering::Relaxed);
    UNKNOWN_USE_REJECTED.store(0, Ordering::Relaxed);
    DUPLICATE_RETIRE_COUNT.store(0, Ordering::Relaxed);
    SLOT_HIGH_WATER.store(0, Ordering::Relaxed);
    OPERATION_STARTED.store(0, Ordering::Relaxed);
    OPERATION_COMPLETED.store(0, Ordering::Relaxed);
    OPERATION_CANCELLED_ON_TASK_DELETE.store(0, Ordering::Relaxed);
    OPERATION_REGISTRY_FULL.store(0, Ordering::Relaxed);
    BTDM_TASK_CREATED.store(0, Ordering::Relaxed);
    BTDM_TASK_DELETE_REQUESTED_COUNT.store(0, Ordering::Relaxed);
    BTDM_TASK_DELETED_COUNT.store(0, Ordering::Relaxed);
    BTDM_TASK_LIVE.store(0, Ordering::Relaxed);
    BTDM_TASK_REGISTRY_FAILURES.store(0, Ordering::Relaxed);
    BTDM_TASK_DELETE_UNATTRIBUTED.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::sync::Mutex;

    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn reclamation_requires_retirement_and_zero_in_flight_operations() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        let epoch = begin_reclaimable_epoch();
        assert_eq!(epoch, 1);
        register(0x1000, 96).unwrap();

        assert_eq!(
            reclaim_current_epoch_after_source_quiescent(),
            Err(ReclaimError::ActiveQueue)
        );
        let guard = begin_task_use(0x1000).expect("active queue");
        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert_eq!(
            reclaim_current_epoch_after_source_quiescent(),
            Err(ReclaimError::OperationInFlight)
        );
        drop(guard);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));

        let stats = snapshot();
        assert_eq!(stats.reclaimed, 1);
        assert_eq!(stats.active_reclaimable_epoch, 0);
        assert_eq!(stats.reclaim_failures, 2);
    }

    #[test]
    fn quiescent_reclamation_allows_slot_and_pointer_reuse() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert!(begin_task_use(0x1000).is_none());
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));

        begin_reclaimable_epoch();
        register(0x1000, 64).expect("quiescent slot can be reused");
        assert!(begin_task_use(0x1000).is_some());
        assert!(begin_task_use(0x2000).is_none());
        let stats = snapshot();
        assert_eq!(stats.late_use_rejected, 1);
        assert_eq!(stats.unknown_use_rejected, 1);
        assert_eq!(stats.created, 2);
    }

    #[test]
    fn fixed_registry_exhaustion_is_explicit() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        for index in 0..SLOT_COUNT {
            register(0x1000 + index * 0x10, index + 1).unwrap();
        }
        assert_eq!(register(0x9000, 1), Err(RegisterError::RegistryFull));
        assert_eq!(snapshot().slot_high_water, SLOT_COUNT);
    }

    #[test]
    fn operation_registry_exhaustion_fails_closed_and_is_observable() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        let guards: std::vec::Vec<_> = (0..OPERATION_SLOT_COUNT)
            .map(|owner| {
                begin_use_with_owner(0x1000, 0x2000 + owner, TASK_OPERATION)
                    .expect("bounded operation slot")
            })
            .collect();
        assert!(begin_task_use(0x1000).is_none());
        assert_eq!(snapshot().operation_registry_full, 1);
        drop(guards);
        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));
    }

    #[test]
    fn task_registry_rejects_missing_epoch_duplicate_and_overflow() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        assert_eq!(
            register_btdm_task(0xaaaa),
            Err(TaskRegisterError::NoActiveEpoch)
        );

        begin_reclaimable_epoch();
        register_btdm_task(0xaaaa).unwrap();
        assert_eq!(
            register_btdm_task(0xaaaa),
            Err(TaskRegisterError::DuplicatePointer)
        );
        for pointer in [0xbbbb, 0xcccc, 0xdddd] {
            register_btdm_task(pointer).unwrap();
        }
        assert_eq!(
            register_btdm_task(0xeeee),
            Err(TaskRegisterError::RegistryFull)
        );
        assert_eq!(snapshot().btdm_task_registry_failures, 3);
    }

    #[test]
    fn deleting_blocked_btdm_task_cancels_unwound_operation_exactly_once() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        let guard = begin_use_with_owner(0x1000, 0xaaaa, TASK_OPERATION).unwrap();
        core::mem::forget(guard);

        assert_eq!(retire(0x1000), RetireResult::Retired);
        let delete = prepare_btdm_task_delete(0xaaaa).unwrap();
        complete_btdm_task_delete(delete);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));

        let stats = snapshot();
        assert_eq!(stats.operation_started, 1);
        assert_eq!(stats.operation_completed, 0);
        assert_eq!(stats.operation_cancelled_on_task_delete, 1);
        assert_eq!(stats.operation_balance_error, 0);
        assert_eq!(stats.btdm_task_live, 0);
        assert_eq!(stats.btdm_task_deleted, 1);
    }

    #[test]
    fn deleting_one_task_never_cancels_another_tasks_operation() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        register_btdm_task(0xbbbb).unwrap();
        let guard = begin_use_with_owner(0x1000, 0xaaaa, TASK_OPERATION).unwrap();
        assert_eq!(retire(0x1000), RetireResult::Retired);

        let delete_b = prepare_btdm_task_delete(0xbbbb).unwrap();
        complete_btdm_task_delete(delete_b);
        assert_eq!(snapshot().in_flight, 1);
        assert_eq!(snapshot().operation_cancelled_on_task_delete, 0);
        assert_eq!(
            reclaim_current_epoch_after_source_quiescent(),
            Err(ReclaimError::BtdmTaskLive)
        );

        drop(guard);
        let delete_a = prepare_btdm_task_delete(0xaaaa).unwrap();
        complete_btdm_task_delete(delete_a);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));
    }

    #[test]
    fn live_isr_operation_still_blocks_reclamation() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        let isr = begin_isr_use(0x1000).unwrap();
        assert_eq!(retire(0x1000), RetireResult::Retired);
        let delete = prepare_btdm_task_delete(0xaaaa).unwrap();
        complete_btdm_task_delete(delete);

        assert_eq!(
            reclaim_current_epoch_after_source_quiescent(),
            Err(ReclaimError::OperationInFlight)
        );
        drop(isr);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));
    }

    #[test]
    fn cancelled_guard_cannot_complete_a_reused_operation_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register(0x2000, 64).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        let stale = begin_use_with_owner(0x1000, 0xaaaa, TASK_OPERATION).unwrap();
        let delete = prepare_btdm_task_delete(0xaaaa).unwrap();
        complete_btdm_task_delete(delete);

        let current = begin_use_with_owner(0x2000, 0xcccc, TASK_OPERATION).unwrap();
        drop(stale);
        assert_eq!(snapshot().in_flight, 1);
        assert_eq!(snapshot().operation_completed, 0);
        assert_eq!(QUEUES[0].in_flight.load(Ordering::Relaxed), 0);
        assert_eq!(QUEUES[1].in_flight.load(Ordering::Relaxed), 1);
        drop(current);
        assert_eq!(snapshot().in_flight, 0);
        assert_eq!(snapshot().operation_completed, 1);

        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert_eq!(retire(0x2000), RetireResult::Retired);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(2));
    }

    #[test]
    fn task_pointer_reuse_is_scoped_by_queue_epoch() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        let delete = prepare_btdm_task_delete(0xaaaa).unwrap();
        complete_btdm_task_delete(delete);
        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));

        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        register_btdm_task(0xaaaa).unwrap();
        assert_eq!(snapshot().btdm_task_live, 1);
        assert_eq!(snapshot().btdm_task_created, 2);
    }

    #[test]
    fn unknown_task_deletion_never_cancels_an_operation() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_for_test();
        begin_reclaimable_epoch();
        register(0x1000, 96).unwrap();
        let guard = begin_use_with_owner(0x1000, 0xaaaa, TASK_OPERATION).unwrap();

        assert!(prepare_btdm_task_delete(0xbbbb).is_none());
        assert_eq!(snapshot().in_flight, 1);
        assert_eq!(snapshot().btdm_task_delete_unattributed, 1);
        drop(guard);
        assert_eq!(retire(0x1000), RetireResult::Retired);
        assert_eq!(reclaim_current_epoch_after_source_quiescent(), Ok(1));
    }
}
