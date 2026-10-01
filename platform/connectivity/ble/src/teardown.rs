//! Pure post-teardown invariant checks (shared BLE runtime and roles plan,
//! Phase 4: extracted from the existing `ble-foundation` diagnostic
//! peripheral's `validate_queue_teardown`/`validate_transport_teardown` --
//! see `products/meditamer/src/firmware/ble/mod.rs`'s history). These are
//! the "BLE foundation gates" the plan's Phase 4 text says to preserve when
//! adapting that probe: after a diagnostic window closes, every queue the
//! vendor-patched radio stack tracks must be back to zero, and the HCI
//! transport must not have faulted or dropped anything -- ambiguous
//! teardown is a safety condition (ADR-0011: "Ambiguous teardown favors
//! safety over availability"), not merely a diagnostic one.
//!
//! [`QueueLifecycleSnapshot`] and [`TransportSnapshot`] mirror
//! `esp_radio`'s `QueueLifecycleStats`/`HciTransportStats` field-for-field
//! so the validators stay pure and host-testable; the `#[cfg(target_os =
//! "none")]` `From` impls below do the one-time conversion from the real
//! vendor types where they are sampled.

/// Mirrors `esp_radio::QueueLifecycleStats`'s fields relevant to
/// post-teardown validation (every counter that must return to zero, plus
/// the two used for the task-cancellation delta and capacity check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueueLifecycleSnapshot {
    pub active: usize,
    pub retired: usize,
    pub owner_active: usize,
    pub owner_retired: usize,
    pub late_use_rejected: usize,
    pub unknown_use_rejected: usize,
    pub reclaim_failures: usize,
    pub owner_corruption: usize,
    pub owner_task_contention_rejected: usize,
    pub owner_isr_contention_rejected: usize,
    pub operation_cancelled_on_task_delete: usize,
    pub slot_capacity: usize,
    pub operation_balance_error: usize,
    pub operation_registry_full: usize,
    pub btdm_task_live: usize,
    pub btdm_task_registry_failures: usize,
    pub btdm_task_delete_unattributed: usize,
}

/// Mirrors `esp_radio::ble::HciTransportStats`'s fields relevant to
/// post-teardown validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransportSnapshot {
    pub faulted: bool,
    pub rx_queue_overflow: u32,
    pub rx_oversize: u32,
    pub tx_rejected: u32,
    pub tx_timeout: u32,
}

#[cfg(target_os = "none")]
impl From<esp_radio::QueueLifecycleStats> for QueueLifecycleSnapshot {
    fn from(stats: esp_radio::QueueLifecycleStats) -> Self {
        Self {
            active: stats.active,
            retired: stats.retired,
            owner_active: stats.owner_active,
            owner_retired: stats.owner_retired,
            late_use_rejected: stats.late_use_rejected,
            unknown_use_rejected: stats.unknown_use_rejected,
            reclaim_failures: stats.reclaim_failures,
            owner_corruption: stats.owner_corruption,
            owner_task_contention_rejected: stats.owner_task_contention_rejected,
            owner_isr_contention_rejected: stats.owner_isr_contention_rejected,
            operation_cancelled_on_task_delete: stats.operation_cancelled_on_task_delete,
            slot_capacity: stats.slot_capacity,
            operation_balance_error: stats.operation_balance_error,
            operation_registry_full: stats.operation_registry_full,
            btdm_task_live: stats.btdm_task_live,
            btdm_task_registry_failures: stats.btdm_task_registry_failures,
            btdm_task_delete_unattributed: stats.btdm_task_delete_unattributed,
        }
    }
}

#[cfg(target_os = "none")]
impl From<esp_radio::ble::HciTransportStats> for TransportSnapshot {
    fn from(stats: esp_radio::ble::HciTransportStats) -> Self {
        Self {
            faulted: stats.faulted,
            rx_queue_overflow: stats.rx_queue_overflow,
            rx_oversize: stats.rx_oversize,
            tx_rejected: stats.tx_rejected,
            tx_timeout: stats.tx_timeout,
        }
    }
}

/// A queue-lifecycle invariant did not hold after teardown, or the
/// task-cancellation delta could not be computed (the "before" sample was
/// larger than the "after" one, which cannot happen for a monotonic
/// counter and indicates the samples were taken out of order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueTeardownFault;

/// The HCI transport faulted, or dropped/rejected traffic, during the
/// window being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportTeardownFault;

/// Validates that every queue the radio stack tracks is back to zero after
/// teardown, and returns the number of task-owned operations cancelled by
/// the probe's own task deletion during the window (informational, not a
/// fault by itself).
///
/// `cancelled_before` is the `operation_cancelled_on_task_delete` counter
/// sampled before the window started, so unrelated cancellations elsewhere
/// in the system do not appear as this window's delta.
pub fn validate_queue_teardown(
    queues: QueueLifecycleSnapshot,
    cancelled_before: usize,
) -> Result<usize, QueueTeardownFault> {
    let cancelled = queues
        .operation_cancelled_on_task_delete
        .checked_sub(cancelled_before)
        .ok_or(QueueTeardownFault)?;
    if queues.active != 0
        || queues.retired != 0
        || queues.owner_active != 0
        || queues.owner_retired != 0
        || queues.late_use_rejected != 0
        || queues.unknown_use_rejected != 0
        || queues.reclaim_failures != 0
        || queues.owner_corruption != 0
        || queues.owner_task_contention_rejected != 0
        || queues.owner_isr_contention_rejected != 0
        || cancelled > queues.slot_capacity
        || queues.operation_balance_error != 0
        || queues.operation_registry_full != 0
        || queues.btdm_task_live != 0
        || queues.btdm_task_registry_failures != 0
        || queues.btdm_task_delete_unattributed != 0
    {
        return Err(QueueTeardownFault);
    }
    Ok(cancelled)
}

/// Validates that the HCI transport neither faulted nor dropped/rejected
/// traffic across the "active" and "teardown" samples of one window.
pub fn validate_transport_teardown(
    active: TransportSnapshot,
    teardown: TransportSnapshot,
) -> Result<(), TransportTeardownFault> {
    if active.faulted
        || teardown.faulted
        || active.rx_queue_overflow != 0
        || active.rx_oversize != 0
        || active.tx_rejected != 0
        || active.tx_timeout != 0
    {
        return Err(TransportTeardownFault);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_queues() -> QueueLifecycleSnapshot {
        QueueLifecycleSnapshot {
            slot_capacity: 8,
            ..QueueLifecycleSnapshot::default()
        }
    }

    #[test]
    fn clean_teardown_reports_zero_cancelled_by_default() {
        assert_eq!(validate_queue_teardown(clean_queues(), 0), Ok(0));
    }

    #[test]
    fn cancelled_delta_is_computed_against_before_sample() {
        let queues = QueueLifecycleSnapshot {
            operation_cancelled_on_task_delete: 5,
            ..clean_queues()
        };
        assert_eq!(validate_queue_teardown(queues, 3), Ok(2));
    }

    #[test]
    fn before_sample_larger_than_after_is_a_fault() {
        let queues = QueueLifecycleSnapshot {
            operation_cancelled_on_task_delete: 1,
            ..clean_queues()
        };
        assert_eq!(validate_queue_teardown(queues, 2), Err(QueueTeardownFault));
    }

    #[test]
    fn cancelled_delta_beyond_capacity_is_a_fault() {
        let queues = QueueLifecycleSnapshot {
            operation_cancelled_on_task_delete: 10,
            slot_capacity: 8,
            ..QueueLifecycleSnapshot::default()
        };
        assert_eq!(validate_queue_teardown(queues, 0), Err(QueueTeardownFault));
    }

    #[test]
    fn nonzero_active_is_a_fault() {
        let queues = QueueLifecycleSnapshot {
            active: 1,
            ..clean_queues()
        };
        assert_eq!(validate_queue_teardown(queues, 0), Err(QueueTeardownFault));
    }

    #[test]
    fn nonzero_owner_corruption_is_a_fault() {
        let queues = QueueLifecycleSnapshot {
            owner_corruption: 1,
            ..clean_queues()
        };
        assert_eq!(validate_queue_teardown(queues, 0), Err(QueueTeardownFault));
    }

    #[test]
    fn nonzero_btdm_task_live_is_a_fault() {
        let queues = QueueLifecycleSnapshot {
            btdm_task_live: 1,
            ..clean_queues()
        };
        assert_eq!(validate_queue_teardown(queues, 0), Err(QueueTeardownFault));
    }

    #[test]
    fn clean_transport_passes() {
        assert_eq!(
            validate_transport_teardown(TransportSnapshot::default(), TransportSnapshot::default()),
            Ok(())
        );
    }

    #[test]
    fn faulted_active_sample_is_a_fault() {
        let active = TransportSnapshot {
            faulted: true,
            ..TransportSnapshot::default()
        };
        assert_eq!(
            validate_transport_teardown(active, TransportSnapshot::default()),
            Err(TransportTeardownFault)
        );
    }

    #[test]
    fn faulted_teardown_sample_is_a_fault() {
        let teardown = TransportSnapshot {
            faulted: true,
            ..TransportSnapshot::default()
        };
        assert_eq!(
            validate_transport_teardown(TransportSnapshot::default(), teardown),
            Err(TransportTeardownFault)
        );
    }

    #[test]
    fn rx_queue_overflow_during_active_is_a_fault() {
        let active = TransportSnapshot {
            rx_queue_overflow: 1,
            ..TransportSnapshot::default()
        };
        assert_eq!(
            validate_transport_teardown(active, TransportSnapshot::default()),
            Err(TransportTeardownFault)
        );
    }

    #[test]
    fn tx_timeout_during_active_is_a_fault() {
        let active = TransportSnapshot {
            tx_timeout: 1,
            ..TransportSnapshot::default()
        };
        assert_eq!(
            validate_transport_teardown(active, TransportSnapshot::default()),
            Err(TransportTeardownFault)
        );
    }
}
