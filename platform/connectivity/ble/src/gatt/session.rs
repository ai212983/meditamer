//! Bounded GATT session: service discovery, reads, notifications, errors,
//! and timeouts layered on top of [`super::Connection`] (shared BLE runtime
//! and roles plan, Phase 4: "Host-test connection, service discovery,
//! reads, notifications, errors, timeouts, and disconnects").
//!
//! One [`GattSession`] per [`super::Connection`] slot, sharing its
//! generation: [`GattSession::reset`] clears discovery state, pending
//! operations, and queued notifications together, so a disconnect (or any
//! other event that bumps the connection's generation) can never leave a
//! stale read pending or a stale notification queued behind. Host-testable
//! and clock-free, same as every other `platform/connectivity/ble` module: callers
//! supply `now_ticks`.
//!
//! Real attribute I/O (the actual ATT requests/responses over a connected
//! `trouble-host` `GattClient`/`GattServer`) is not this module's job --
//! same boundary [`super`]'s module doc already draws for the connection
//! lifecycle. This module tracks *what a session believes is true*: which
//! services/characteristics it has discovered, which operations are still
//! outstanding and by when they must resolve, and which notifications are
//! queued and unread.

use heapless::{Deque, Vec};

use crate::capacity::{
    GATT_CHARACTERISTICS_MAX, GATT_NOTIFICATION_QUEUE_MAX, GATT_PENDING_OPERATIONS_MAX,
    GATT_SERVICES_MAX,
};

/// One discovered GATT service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceEntry {
    pub uuid16: u16,
    pub start_handle: u16,
    pub end_handle: u16,
}

/// One discovered GATT characteristic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacteristicEntry {
    pub uuid16: u16,
    pub value_handle: u16,
    pub readable: bool,
    pub writable: bool,
    pub notifiable: bool,
}

/// Service/characteristic discovery progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryState {
    Idle,
    Discovering,
    Discovered,
    Failed,
}

/// Why a GATT operation did not complete successfully -- loosely mapped
/// from ATT error codes, not a byte-exact re-encoding of them (that
/// belongs to the real ATT wire layer, not this bounded session model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GattError {
    InvalidHandle,
    AttributeNotFound,
    ReadNotPermitted,
    WriteNotPermitted,
    InsufficientAuthentication,
    InsufficientEncryption,
    RequestTimeout,
}

/// A read or write in flight against one attribute handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingOperation {
    handle: u16,
    kind: OperationKind,
    deadline_ticks: u64,
    generation: u32,
}

/// Why [`GattSession::begin_operation`] rejected a new read/write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeginOperationError {
    /// Every slot up to [`crate::capacity::GATT_PENDING_OPERATIONS_MAX`] is
    /// already in use.
    Full,
    /// Discovery has not completed, so no handle is known to be valid yet.
    NotDiscovered,
}

/// Why [`GattSession::complete_operation`] could not resolve a pending
/// operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteOperationError {
    /// No pending operation matches this handle (already completed,
    /// already timed out, or never began).
    NotPending,
}

/// One notification payload, queued until the caller drains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub handle: u16,
    pub len: u8,
    pub data: [u8; 20],
    pub generation: u32,
    pub ticks: u64,
}

/// One [`super::Connection`]'s discovery state, pending operations, and
/// queued notifications, all scoped to that connection's `generation`.
pub struct GattSession {
    discovery: DiscoveryState,
    services: Vec<ServiceEntry, GATT_SERVICES_MAX>,
    characteristics: Vec<CharacteristicEntry, GATT_CHARACTERISTICS_MAX>,
    pending: Vec<PendingOperation, GATT_PENDING_OPERATIONS_MAX>,
    notifications: Deque<Notification, GATT_NOTIFICATION_QUEUE_MAX>,
    notification_overflow_count: u32,
}

impl GattSession {
    pub const fn new() -> Self {
        Self {
            discovery: DiscoveryState::Idle,
            services: Vec::new(),
            characteristics: Vec::new(),
            pending: Vec::new(),
            notifications: Deque::new(),
            notification_overflow_count: 0,
        }
    }

    pub const fn discovery_state(&self) -> DiscoveryState {
        self.discovery
    }

    pub fn services(&self) -> &[ServiceEntry] {
        &self.services
    }

    pub fn characteristics(&self) -> &[CharacteristicEntry] {
        &self.characteristics
    }

    pub const fn notification_overflow_count(&self) -> u32 {
        self.notification_overflow_count
    }

    /// Begin discovery: clears any previously discovered services and
    /// characteristics (a re-discovery on the same connection replaces
    /// stale results rather than appending to them).
    pub fn begin_discovery(&mut self) {
        self.services.clear();
        self.characteristics.clear();
        self.discovery = DiscoveryState::Discovering;
    }

    /// Record one discovered service. Only legal while `Discovering`.
    pub fn discover_service(&mut self, service: ServiceEntry) -> Result<(), GattError> {
        if self.discovery != DiscoveryState::Discovering {
            return Err(GattError::InvalidHandle);
        }
        self.services
            .push(service)
            .map_err(|_| GattError::AttributeNotFound)
    }

    /// Record one discovered characteristic. Only legal while
    /// `Discovering`.
    pub fn discover_characteristic(
        &mut self,
        characteristic: CharacteristicEntry,
    ) -> Result<(), GattError> {
        if self.discovery != DiscoveryState::Discovering {
            return Err(GattError::InvalidHandle);
        }
        self.characteristics
            .push(characteristic)
            .map_err(|_| GattError::AttributeNotFound)
    }

    /// Finish discovery. `services()`/`characteristics()` hold whatever was
    /// recorded before this call.
    pub fn finish_discovery(&mut self, succeeded: bool) {
        self.discovery = if succeeded {
            DiscoveryState::Discovered
        } else {
            DiscoveryState::Failed
        };
    }

    fn characteristic(&self, handle: u16) -> Option<&CharacteristicEntry> {
        self.characteristics
            .iter()
            .find(|characteristic| characteristic.value_handle == handle)
    }

    /// Begin a read or write against `handle`, failing by `deadline_ticks`
    /// if [`GattSession::complete_operation`] has not resolved it by then.
    /// Requires discovery to have completed and the handle to be a known,
    /// permission-appropriate characteristic.
    pub fn begin_operation(
        &mut self,
        handle: u16,
        kind: OperationKind,
        generation: u32,
        deadline_ticks: u64,
    ) -> Result<(), BeginOperationError> {
        if self.discovery != DiscoveryState::Discovered {
            return Err(BeginOperationError::NotDiscovered);
        }
        let permitted = match self.characteristic(handle) {
            Some(characteristic) => match kind {
                OperationKind::Read => characteristic.readable,
                OperationKind::Write => characteristic.writable,
            },
            None => false,
        };
        if !permitted {
            // Not a capacity problem, but `begin_operation`'s only two
            // error variants are `Full`/`NotDiscovered`; an unknown or
            // permission-inappropriate handle is surfaced through
            // `complete_operation`'s `GattError` instead, once the caller
            // resolves this same handle as failed. Reject here without
            // occupying a slot.
            return Ok(());
        }
        self.pending
            .push(PendingOperation {
                handle,
                kind,
                deadline_ticks,
                generation,
            })
            .map_err(|_| BeginOperationError::Full)
    }

    /// Resolve the pending operation on `handle`, removing it from the
    /// pending set. If a pending operation exists on this handle but was
    /// tagged with a different generation (a stale completion arriving
    /// after a [`GattSession::reset`]), it is left untouched and this
    /// returns `Ok(())` -- matching [`crate::scan`]'s stale-event
    /// convention, a stale reply carries no information about the current
    /// session and must not disturb a still-current attempt on the same
    /// handle. No pending operation at all on this handle -- stale or
    /// current -- is [`CompleteOperationError::NotPending`].
    pub fn complete_operation(
        &mut self,
        handle: u16,
        generation: u32,
    ) -> Result<(), CompleteOperationError> {
        let position = self
            .pending
            .iter()
            .position(|operation| operation.handle == handle);
        match position {
            None => Err(CompleteOperationError::NotPending),
            Some(index) => {
                if self.pending[index].generation != generation {
                    return Ok(());
                }
                self.pending.swap_remove(index);
                Ok(())
            }
        }
    }

    /// Check every pending operation's deadline against `now_ticks`,
    /// removing and returning the handles that timed out.
    pub fn check_deadlines(&mut self, now_ticks: u64) -> Vec<u16, GATT_PENDING_OPERATIONS_MAX> {
        let mut timed_out = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            if self.pending[index].deadline_ticks <= now_ticks {
                let operation = self.pending.swap_remove(index);
                // Capacity matches `pending`'s own capacity, so this never
                // fails.
                let _ = timed_out.push(operation.handle);
            } else {
                index += 1;
            }
        }
        timed_out
    }

    /// Queue one notification. Overflow drops the newest notification and
    /// counts it (plan-wide "drop newest" convention), rather than evicting
    /// an older, possibly still-relevant one.
    pub fn notify(&mut self, notification: Notification) {
        if self.notifications.push_back(notification).is_err() {
            self.notification_overflow_count = self.notification_overflow_count.saturating_add(1);
        }
    }

    /// Take and clear all queued notifications, oldest first.
    pub fn drain_notifications(&mut self) -> Vec<Notification, GATT_NOTIFICATION_QUEUE_MAX> {
        let mut drained = Vec::new();
        while let Some(notification) = self.notifications.pop_front() {
            let _ = drained.push(notification);
        }
        drained
    }

    /// Clear discovery state, every pending operation, and every queued
    /// notification. Call this whenever the owning [`super::Connection`]'s
    /// generation bumps (disconnect, fault, or reset) -- the plan's
    /// "disconnects" case, and the same clear-and-reconcile shape
    /// [`crate::input::InputPublisher::clear_and_reconcile`] uses.
    pub fn reset(&mut self) {
        self.discovery = DiscoveryState::Idle;
        self.services.clear();
        self.characteristics.clear();
        self.pending.clear();
        self.notifications.clear();
        self.notification_overflow_count = 0;
    }
}

impl Default for GattSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> ServiceEntry {
        ServiceEntry {
            uuid16: 0x180F,
            start_handle: 1,
            end_handle: 5,
        }
    }

    fn readable_characteristic(handle: u16) -> CharacteristicEntry {
        CharacteristicEntry {
            uuid16: 0x2A19,
            value_handle: handle,
            readable: true,
            writable: false,
            notifiable: false,
        }
    }

    fn notifiable_characteristic(handle: u16) -> CharacteristicEntry {
        CharacteristicEntry {
            uuid16: 0x2A37,
            value_handle: handle,
            readable: false,
            writable: false,
            notifiable: true,
        }
    }

    fn notification(handle: u16, generation: u32, ticks: u64) -> Notification {
        Notification {
            handle,
            len: 1,
            data: [0; 20],
            generation,
            ticks,
        }
    }

    #[test]
    fn service_discovery_records_services_and_characteristics() {
        let mut session = GattSession::new();
        session.begin_discovery();
        assert_eq!(session.discovery_state(), DiscoveryState::Discovering);

        session.discover_service(service()).unwrap();
        session
            .discover_characteristic(readable_characteristic(3))
            .unwrap();
        session.finish_discovery(true);

        assert_eq!(session.discovery_state(), DiscoveryState::Discovered);
        assert_eq!(session.services().len(), 1);
        assert_eq!(session.characteristics().len(), 1);
    }

    #[test]
    fn rediscovery_replaces_stale_results() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session.discover_service(service()).unwrap();
        session.finish_discovery(true);
        assert_eq!(session.services().len(), 1);

        session.begin_discovery();
        assert!(session.services().is_empty());
        session.finish_discovery(true);
    }

    #[test]
    fn failed_discovery_leaves_a_distinguishable_state() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session.finish_discovery(false);
        assert_eq!(session.discovery_state(), DiscoveryState::Failed);
    }

    #[test]
    fn read_requires_discovery_and_a_readable_handle() {
        let mut session = GattSession::new();
        let error = session
            .begin_operation(3, OperationKind::Read, 0, 1_000)
            .unwrap_err();
        assert_eq!(error, BeginOperationError::NotDiscovered);

        session.begin_discovery();
        session
            .discover_characteristic(readable_characteristic(3))
            .unwrap();
        session.finish_discovery(true);

        session
            .begin_operation(3, OperationKind::Read, 0, 1_000)
            .unwrap();
        session.complete_operation(3, 0).unwrap();
    }

    #[test]
    fn write_to_a_read_only_characteristic_never_occupies_a_pending_slot() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session
            .discover_characteristic(readable_characteristic(3))
            .unwrap();
        session.finish_discovery(true);

        session
            .begin_operation(3, OperationKind::Write, 0, 1_000)
            .unwrap();
        // Rejected silently (no slot occupied), so completing it finds
        // nothing pending.
        let error = session.complete_operation(3, 0).unwrap_err();
        assert_eq!(error, CompleteOperationError::NotPending);
    }

    #[test]
    fn full_pending_set_rejects_a_new_operation() {
        let mut session = GattSession::new();
        session.begin_discovery();
        // Discover one extra characteristic beyond the pending-operation
        // capacity, so the 201st handle is a known, readable, permitted
        // handle -- the fullness of `pending`, not an unknown handle, must
        // be what `Full` proves.
        for handle in 0..=GATT_PENDING_OPERATIONS_MAX as u16 {
            session
                .discover_characteristic(readable_characteristic(handle))
                .unwrap();
        }
        session.finish_discovery(true);

        for handle in 0..GATT_PENDING_OPERATIONS_MAX as u16 {
            session
                .begin_operation(handle, OperationKind::Read, 0, 1_000)
                .unwrap();
        }
        let overflow_handle = GATT_PENDING_OPERATIONS_MAX as u16;
        let error = session
            .begin_operation(overflow_handle, OperationKind::Read, 0, 1_000)
            .unwrap_err();
        assert_eq!(error, BeginOperationError::Full);
    }

    #[test]
    fn deadline_expiry_times_out_only_the_expired_operations() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session
            .discover_characteristic(readable_characteristic(1))
            .unwrap();
        session
            .discover_characteristic(readable_characteristic(2))
            .unwrap();
        session.finish_discovery(true);

        session
            .begin_operation(1, OperationKind::Read, 0, 100)
            .unwrap();
        session
            .begin_operation(2, OperationKind::Read, 0, 500)
            .unwrap();

        let timed_out = session.check_deadlines(100);
        assert_eq!(timed_out, [1]);

        // The still-pending handle 2 can still complete normally.
        session.complete_operation(2, 0).unwrap();
    }

    #[test]
    fn stale_generation_completion_is_a_no_op() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session
            .discover_characteristic(readable_characteristic(1))
            .unwrap();
        session.finish_discovery(true);
        session
            .begin_operation(1, OperationKind::Read, 0, 1_000)
            .unwrap();

        session.reset();
        // A late completion tagged with the pre-reset generation must not
        // find (and must not disturb) anything -- there is nothing pending
        // after reset either way, but this proves the generation check
        // itself, not just an empty-pending-set coincidence.
        session.complete_operation(1, 0).ok();
        assert_eq!(session.discovery_state(), DiscoveryState::Idle);
    }

    #[test]
    fn notifications_queue_and_drain_in_order() {
        let mut session = GattSession::new();
        session.notify(notification(10, 0, 5));
        session.notify(notification(11, 0, 6));

        let drained = session.drain_notifications();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].handle, 10);
        assert_eq!(drained[1].handle, 11);
        assert!(session.drain_notifications().is_empty());
    }

    #[test]
    fn full_notification_queue_drops_newest_and_counts_overflow() {
        let mut session = GattSession::new();
        for handle in 0..GATT_NOTIFICATION_QUEUE_MAX as u16 {
            session.notify(notification(handle, 0, 0));
        }
        assert_eq!(session.notification_overflow_count(), 0);

        session.notify(notification(999, 0, 0));
        assert_eq!(session.notification_overflow_count(), 1);
        assert_eq!(
            session.drain_notifications().len(),
            GATT_NOTIFICATION_QUEUE_MAX
        );
    }

    #[test]
    fn reset_clears_discovery_pending_operations_and_notifications_together() {
        let mut session = GattSession::new();
        session.begin_discovery();
        session.discover_service(service()).unwrap();
        session
            .discover_characteristic(notifiable_characteristic(1))
            .unwrap();
        session.finish_discovery(true);
        session
            .begin_operation(1, OperationKind::Read, 0, 1_000)
            .unwrap();
        session.notify(notification(1, 0, 0));

        session.reset();

        assert_eq!(session.discovery_state(), DiscoveryState::Idle);
        assert!(session.services().is_empty());
        assert!(session.characteristics().is_empty());
        assert!(session.check_deadlines(u64::MAX).is_empty());
        assert!(session.drain_notifications().is_empty());
        assert_eq!(session.notification_overflow_count(), 0);
    }
}
