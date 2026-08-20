//! Who holds the radio, published by each claimant rather than asked of it.
//!
//! Before this existed, the Wi-Fi supervisor and the BLE stack each reached
//! into the other to answer one question — "do you hold the radio?" — which
//! made them mutually dependent. Each publishes its own state here now, and
//! reads the composite. The dependency runs claimant → arbitration in both
//! directions, and nowhere between the claimants.
//!
//! Every field is a plain atomic and every read is a snapshot. This is
//! deliberately not a lock: the callers are a supervisor task and a BLE probe
//! path that must not block each other, and the arbitration model
//! ([`crate::handoff`]) is what turns these observations into decisions.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

/// What the BLE side reports about its own hold on the radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    /// BLE is definitively not using the radio.
    KnownClosed,
    /// BLE holds the radio.
    Active,
    /// BLE cannot vouch for its own state; the supervisor must treat the radio
    /// as unsafe to seize.
    Unknown,
}

const OWNERSHIP_KNOWN_CLOSED: u8 = 0;
const OWNERSHIP_ACTIVE: u8 = 1;
const OWNERSHIP_UNKNOWN: u8 = 2;

// Unknown until BLE says otherwise: a supervisor that has heard nothing must
// not conclude the radio is free.
static BLE_OWNERSHIP: AtomicU8 = AtomicU8::new(OWNERSHIP_UNKNOWN);

static LEASE_VALID: AtomicBool = AtomicBool::new(false);
static LEASE_BOOT: AtomicU32 = AtomicU32::new(0);
static LEASE_EPOCH: AtomicU32 = AtomicU32::new(0);

static WIFI_CONTROLLER_RESIDENT: AtomicBool = AtomicBool::new(false);
static NET_RUNNER_RESIDENT: AtomicBool = AtomicBool::new(false);
static WIFI_LINK: AtomicBool = AtomicBool::new(false);
static SERVICE_LISTENING: AtomicBool = AtomicBool::new(false);
static RADIO_QUIESCED: AtomicBool = AtomicBool::new(false);

/// Publish the BLE side's hold on the radio.
pub fn set_ble_ownership(ownership: Ownership) {
    let raw = match ownership {
        Ownership::KnownClosed => OWNERSHIP_KNOWN_CLOSED,
        Ownership::Active => OWNERSHIP_ACTIVE,
        Ownership::Unknown => OWNERSHIP_UNKNOWN,
    };
    BLE_OWNERSHIP.store(raw, Ordering::Release);
}

/// Read the BLE side's hold on the radio. Anything unrecognised reads as
/// [`Ownership::Unknown`], which is the safe direction.
pub fn ble_ownership() -> Ownership {
    match BLE_OWNERSHIP.load(Ordering::Acquire) {
        OWNERSHIP_KNOWN_CLOSED => Ownership::KnownClosed,
        OWNERSHIP_ACTIVE => Ownership::Active,
        _ => Ownership::Unknown,
    }
}

/// Publish the supervisor's exclusive-off lease. The lease is the supervisor's
/// proof that it tore a complete Wi-Fi epoch down, and is matched exactly.
pub fn publish_exclusive_lease(boot_generation: u32, epoch: u32) {
    LEASE_BOOT.store(boot_generation, Ordering::Relaxed);
    LEASE_EPOCH.store(epoch, Ordering::Relaxed);
    LEASE_VALID.store(true, Ordering::Release);
}

pub fn clear_exclusive_lease() {
    LEASE_VALID.store(false, Ordering::Release);
}

pub fn exclusive_lease_matches(boot_generation: u32, epoch: u32) -> bool {
    LEASE_VALID.load(Ordering::Acquire)
        && LEASE_BOOT.load(Ordering::Relaxed) == boot_generation
        && LEASE_EPOCH.load(Ordering::Relaxed) == epoch
}

/// Publish which supervisor tasks are still resident. Both must be gone before
/// the radio is genuinely free.
pub fn set_residency(wifi_controller: bool, net_runner: bool) {
    WIFI_CONTROLLER_RESIDENT.store(wifi_controller, Ordering::Release);
    NET_RUNNER_RESIDENT.store(net_runner, Ordering::Release);
}

/// Publish whether the Wi-Fi link is up.
pub fn set_wifi_link(up: bool) {
    WIFI_LINK.store(up, Ordering::Release);
}

/// Publish whether the product's listener is accepting.
pub fn set_service_listening(listening: bool) {
    SERVICE_LISTENING.store(listening, Ordering::Release);
}

/// Publish the connection task's intentional-dormant policy. Not an ownership
/// proof — a complete epoch teardown does not update it — but claimants report
/// it alongside the rest when diagnosing a refused handoff.
pub fn set_radio_quiesced(quiesced: bool) {
    RADIO_QUIESCED.store(quiesced, Ordering::Release);
}

/// Everything the arbiter has been told, for diagnostics. Decisions should use
/// [`exclusive_ownership_confirmed`] rather than re-deriving from these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observations {
    pub wifi_controller_resident: bool,
    pub net_runner_resident: bool,
    pub wifi_link: bool,
    pub service_listening: bool,
    pub radio_quiesced: bool,
}

pub fn observations() -> Observations {
    Observations {
        wifi_controller_resident: WIFI_CONTROLLER_RESIDENT.load(Ordering::Acquire),
        net_runner_resident: NET_RUNNER_RESIDENT.load(Ordering::Acquire),
        wifi_link: WIFI_LINK.load(Ordering::Acquire),
        service_listening: SERVICE_LISTENING.load(Ordering::Acquire),
        radio_quiesced: RADIO_QUIESCED.load(Ordering::Acquire),
    }
}

/// The one question the BLE side used to assemble from four separate reaches
/// into the supervisor: is exclusive ownership of the radio confirmed for this
/// lease?
pub fn exclusive_ownership_confirmed(boot_generation: u32, epoch: u32) -> bool {
    crate::handoff::exclusive_ownership_confirmed(
        exclusive_lease_matches(boot_generation, epoch),
        WIFI_CONTROLLER_RESIDENT.load(Ordering::Acquire),
        NET_RUNNER_RESIDENT.load(Ordering::Acquire),
        WIFI_LINK.load(Ordering::Acquire),
        SERVICE_LISTENING.load(Ordering::Acquire),
    )
}
