//! Bounded GATT connection lifecycle plus session state (shared BLE runtime
//! and roles plan, Phase 2/4: "Host-test connection, service discovery,
//! reads, notifications, errors, timeouts, and disconnects").
//!
//! Phase 2 covers the connection lifecycle itself: a bounded,
//! deadline-driven state machine ([`Connection`]/[`ConnectionTable`],
//! this module), generation-tagged so a reply or event from a torn-down
//! connection cannot be mistaken for the current one, plus a fixed-capacity
//! table so more simultaneous connections than the target supports are
//! rejected rather than silently dropped elsewhere.
//!
//! Phase 4 adds [`session::GattSession`]: service/characteristic discovery,
//! bounded pending reads/writes with deadlines, and a bounded notification
//! queue, one session per [`Connection`] slot sharing its generation. The
//! actual ATT wire I/O over a real `trouble-host` controller is not this
//! module's job (Phase 3's `live_scan` is the template for wiring a
//! host-tested module to a real controller, once that lands here); this
//! module only tracks *what a connection/session believes is true* right
//! now. Host-testable and clock-free, same as [`crate::scan`]: callers
//! supply `now_ticks`.

use heapless::Vec;

use crate::capacity::GATT_CONNECTIONS_MAX;

pub mod session;
pub use session::{
    BeginOperationError, CharacteristicEntry, CompleteOperationError, DiscoveryState, GattError,
    GattSession, Notification, OperationKind, ServiceEntry,
};

/// A connection slot's lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// No connection attempt in progress; the slot is free.
    Idle,
    /// A connect attempt is in flight, with a deadline.
    Connecting,
    /// Connected; service discovery/reads/notifications may proceed.
    Connected,
    /// A graceful teardown is in flight, with a deadline.
    Disconnecting,
    /// Torn down cleanly; the slot is free for a new attempt.
    Disconnected,
    /// A deadline expired, or the caller reported a transport error. The
    /// slot must be explicitly [`Connection::reset`] before reuse -- unlike
    /// `Disconnected`, `Faulted` does not imply the radio/controller state
    /// is known-clean.
    Faulted,
}

/// Why an operation on a [`Connection`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionError {
    /// The requested transition is not legal from the current state (e.g.
    /// disconnecting an already-`Idle` slot).
    InvalidTransition,
    /// The caller's `generation` does not match [`Connection::generation`]:
    /// a stale event from before the last [`Connection::reset`].
    StaleGeneration,
}

/// One bounded, deadline-driven connection lifecycle.
#[derive(Clone, Copy, Debug)]
pub struct Connection {
    state: ConnectionState,
    generation: u32,
    deadline_ticks: Option<u64>,
}

impl Connection {
    /// A fresh, idle connection at generation 0.
    pub const fn new() -> Self {
        Self {
            state: ConnectionState::Idle,
            generation: 0,
            deadline_ticks: None,
        }
    }

    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// Begin a connect attempt, failing by `deadline_ticks` if it has not
    /// reached [`ConnectionState::Connected`] by then. Only legal from
    /// `Idle` or `Disconnected`.
    pub fn begin_connect(&mut self, deadline_ticks: u64) -> Result<(), ConnectionError> {
        match self.state {
            ConnectionState::Idle | ConnectionState::Disconnected => {
                self.state = ConnectionState::Connecting;
                self.deadline_ticks = Some(deadline_ticks);
                Ok(())
            }
            _ => Err(ConnectionError::InvalidTransition),
        }
    }

    /// Report that the connect attempt tagged `generation` succeeded.
    pub fn mark_connected(&mut self, generation: u32) -> Result<(), ConnectionError> {
        if generation != self.generation {
            return Err(ConnectionError::StaleGeneration);
        }
        if self.state != ConnectionState::Connecting {
            return Err(ConnectionError::InvalidTransition);
        }
        self.state = ConnectionState::Connected;
        self.deadline_ticks = None;
        Ok(())
    }

    /// Begin a graceful teardown, failing by `deadline_ticks` if it has not
    /// reached [`ConnectionState::Disconnected`] by then. Only legal from
    /// `Connected`.
    pub fn begin_disconnect(
        &mut self,
        generation: u32,
        deadline_ticks: u64,
    ) -> Result<(), ConnectionError> {
        if generation != self.generation {
            return Err(ConnectionError::StaleGeneration);
        }
        if self.state != ConnectionState::Connected {
            return Err(ConnectionError::InvalidTransition);
        }
        self.state = ConnectionState::Disconnecting;
        self.deadline_ticks = Some(deadline_ticks);
        Ok(())
    }

    /// Report that the teardown tagged `generation` completed.
    pub fn mark_disconnected(&mut self, generation: u32) -> Result<(), ConnectionError> {
        if generation != self.generation {
            return Err(ConnectionError::StaleGeneration);
        }
        if self.state != ConnectionState::Disconnecting {
            return Err(ConnectionError::InvalidTransition);
        }
        self.state = ConnectionState::Disconnected;
        self.deadline_ticks = None;
        Ok(())
    }

    /// Report a transport error tagged `generation`. Legal from any state
    /// except `Idle`/`Faulted` (an already-idle or already-faulted slot has
    /// nothing to fault further). Stale-generation faults are silently
    /// ignored, same as [`crate::scan::ScanWindow::observe_if_current`]'s
    /// stale-event handling: an error report about a torn-down connection
    /// carries no information about the current one.
    pub fn fault(&mut self, generation: u32) {
        if generation != self.generation
            || matches!(self.state, ConnectionState::Idle | ConnectionState::Faulted)
        {
            return;
        }
        self.state = ConnectionState::Faulted;
        self.deadline_ticks = None;
    }

    /// Check `now_ticks` against a pending deadline, faulting the
    /// connection if it has passed. Idempotent: calling this after the
    /// state has already moved on (or after an unrelated deadline has
    /// already fired) is a no-op.
    pub fn check_deadline(&mut self, now_ticks: u64) {
        if let Some(deadline) = self.deadline_ticks {
            if now_ticks >= deadline {
                self.state = ConnectionState::Faulted;
                self.deadline_ticks = None;
            }
        }
    }

    /// Return this slot to `Idle` at a new generation, clearing any
    /// deadline. The only way out of `Faulted`; also usable from
    /// `Disconnected` to make the slot available again without carrying its
    /// old generation forward, so `mark_connected`/`mark_disconnected` calls
    /// tagged with the old generation are provably stale afterward.
    pub fn reset(&mut self) {
        self.state = ConnectionState::Idle;
        self.generation = self.generation.wrapping_add(1);
        self.deadline_ticks = None;
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}

/// Why [`ConnectionTable::open`] could not start a new connection attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// Every slot up to [`crate::capacity::GATT_CONNECTIONS_MAX`] is
    /// already in use.
    Full,
}

/// A fixed-capacity table of [`Connection`]s (plan Phase 8: central and
/// peripheral connections open at once).
pub struct ConnectionTable {
    slots: Vec<Connection, GATT_CONNECTIONS_MAX>,
}

impl ConnectionTable {
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub fn connections(&self) -> &[Connection] {
        &self.slots
    }

    /// Start a new connect attempt in a fresh slot, if one is free.
    pub fn open(&mut self, deadline_ticks: u64) -> Result<usize, TableError> {
        let mut connection = Connection::new();
        connection
            .begin_connect(deadline_ticks)
            .expect("a fresh Connection can always begin_connect");
        self.slots.push(connection).map_err(|_| TableError::Full)?;
        Ok(self.slots.len() - 1)
    }

    /// Advance every slot's deadline check against `now_ticks`.
    pub fn check_deadlines(&mut self, now_ticks: u64) {
        for connection in self.slots.iter_mut() {
            connection.check_deadline(now_ticks);
        }
    }

    /// Reclaim every slot that is `Disconnected` or `Faulted`, in place.
    /// Call this once the caller has observed and reported those
    /// terminal states, so a later `open` can reuse the freed capacity.
    pub fn reclaim_terminal(&mut self) {
        self.slots.retain(|connection| {
            !matches!(
                connection.state(),
                ConnectionState::Disconnected | ConnectionState::Faulted
            )
        });
    }
}

impl Default for ConnectionTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_then_disconnect_happy_path() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        assert_eq!(connection.state(), ConnectionState::Connecting);

        connection.mark_connected(connection.generation()).unwrap();
        assert_eq!(connection.state(), ConnectionState::Connected);

        connection
            .begin_disconnect(connection.generation(), 2_000)
            .unwrap();
        assert_eq!(connection.state(), ConnectionState::Disconnecting);

        connection
            .mark_disconnected(connection.generation())
            .unwrap();
        assert_eq!(connection.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let mut connection = Connection::new();
        let error = connection.mark_connected(0).unwrap_err();
        assert_eq!(error, ConnectionError::InvalidTransition);

        connection.begin_connect(1_000).unwrap();
        let error = connection.begin_connect(2_000).unwrap_err();
        assert_eq!(error, ConnectionError::InvalidTransition);
    }

    #[test]
    fn connect_timeout_faults_the_connection() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();

        connection.check_deadline(999);
        assert_eq!(connection.state(), ConnectionState::Connecting);

        connection.check_deadline(1_000);
        assert_eq!(connection.state(), ConnectionState::Faulted);
    }

    #[test]
    fn disconnect_timeout_faults_the_connection() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        connection.mark_connected(connection.generation()).unwrap();
        connection
            .begin_disconnect(connection.generation(), 2_000)
            .unwrap();

        connection.check_deadline(2_000);
        assert_eq!(connection.state(), ConnectionState::Faulted);
    }

    #[test]
    fn reset_bumps_generation_and_makes_prior_generation_calls_stale() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        let old_generation = connection.generation();
        connection.check_deadline(1_000);
        assert_eq!(connection.state(), ConnectionState::Faulted);

        connection.reset();
        assert_eq!(connection.state(), ConnectionState::Idle);
        assert_eq!(connection.generation(), old_generation.wrapping_add(1));

        let error = connection.mark_connected(old_generation).unwrap_err();
        assert_eq!(error, ConnectionError::StaleGeneration);
    }

    #[test]
    fn stale_generation_fault_is_ignored() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        connection.mark_connected(connection.generation()).unwrap();
        let stale_generation = connection.generation();
        connection.reset();

        // A late error report tagged with the pre-reset generation must not
        // fault the new, unrelated connection attempt.
        connection.fault(stale_generation);
        assert_eq!(connection.state(), ConnectionState::Idle);
    }

    #[test]
    fn fault_from_connected_state_is_immediate() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        connection.mark_connected(connection.generation()).unwrap();

        connection.fault(connection.generation());
        assert_eq!(connection.state(), ConnectionState::Faulted);
    }

    #[test]
    fn table_open_up_to_capacity_then_rejects_full() {
        let mut table = ConnectionTable::new();
        for _ in 0..GATT_CONNECTIONS_MAX {
            table.open(1_000).unwrap();
        }
        assert_eq!(table.connections().len(), GATT_CONNECTIONS_MAX);

        let error = table.open(1_000).unwrap_err();
        assert_eq!(error, TableError::Full);
        assert_eq!(table.connections().len(), GATT_CONNECTIONS_MAX);
    }

    #[test]
    fn reclaim_terminal_frees_capacity_for_a_new_open() {
        let mut table = ConnectionTable::new();
        for _ in 0..GATT_CONNECTIONS_MAX {
            table.open(1_000).unwrap();
        }
        table.check_deadlines(1_000);
        assert!(table
            .connections()
            .iter()
            .all(|connection| connection.state() == ConnectionState::Faulted));

        table.reclaim_terminal();
        assert!(table.connections().is_empty());

        table.open(2_000).unwrap();
        assert_eq!(table.connections().len(), 1);
    }

    #[test]
    fn check_deadline_after_state_moved_on_is_a_no_op() {
        let mut connection = Connection::new();
        connection.begin_connect(1_000).unwrap();
        connection.mark_connected(connection.generation()).unwrap();

        // The connect deadline has technically passed, but the connection
        // already moved to `Connected` and cleared it.
        connection.check_deadline(1_000);
        assert_eq!(connection.state(), ConnectionState::Connected);
    }
}
