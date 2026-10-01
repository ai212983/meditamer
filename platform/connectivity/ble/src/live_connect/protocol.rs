use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;

use critical_section::Mutex;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{with_timeout, Duration};
use trouble_host::prelude::*;

use super::{
    DiscoveredUuid, LiveConnectReport, ServiceDiscoveryFailure, HID_REPORT_MAP_UUID,
    HID_REPORT_REFERENCE_UUID, HID_REPORT_UUID, HID_SERVICE_UUID,
};
use crate::gatt::Connection as SessionConnection;
use crate::live_scan::parse_local_name;

pub(super) enum RuntimeRace<T> {
    Exited,
    Operation(T),
}

pub(super) async fn race_runner<R, O>(runner: Pin<&mut R>, operation: O) -> RuntimeRace<O::Output>
where
    R: Future,
    O: Future,
{
    match select(runner, operation).await {
        Either::First(_) => RuntimeRace::Exited,
        Either::Second(output) => RuntimeRace::Operation(output),
    }
}

pub(super) async fn race_runtime<R, C, O>(
    runner: Pin<&mut R>,
    client: Pin<&mut C>,
    operation: O,
) -> RuntimeRace<O::Output>
where
    R: Future,
    C: Future,
    O: Future,
{
    match select3(runner, client, operation).await {
        Either3::First(_) | Either3::Second(_) => RuntimeRace::Exited,
        Either3::Third(output) => RuntimeRace::Operation(output),
    }
}

pub(super) enum PairingEvent {
    Complete {
        security_level: SecurityLevel,
        bond: Option<BondInformation>,
    },
    Failed,
    Disconnected(u8),
}

pub(super) enum HidStreamEvent<const MTU: usize> {
    Notification(Notification<MTU>),
    Disconnected(u8),
}

pub(super) async fn next_hid_stream_event<const MTU: usize>(
    listener: &mut NotificationListener<'_, MTU>,
    connection: &Connection<'_, DefaultPacketPool>,
) -> HidStreamEvent<MTU> {
    let disconnected = async {
        loop {
            if let ConnectionEvent::Disconnected { reason } = connection.next().await {
                return reason.into_inner();
            }
        }
    };
    match select(listener.next(), disconnected).await {
        Either::First(notification) => HidStreamEvent::Notification(notification),
        Either::Second(reason) => HidStreamEvent::Disconnected(reason),
    }
}

pub(super) async fn next_pairing_event(
    connection: &Connection<'_, DefaultPacketPool>,
) -> PairingEvent {
    loop {
        match connection.next().await {
            ConnectionEvent::PairingComplete {
                security_level,
                bond,
            } => {
                return PairingEvent::Complete {
                    security_level,
                    bond,
                };
            }
            ConnectionEvent::PairingFailed(_) => return PairingEvent::Failed,
            ConnectionEvent::Disconnected { reason } => {
                return PairingEvent::Disconnected(reason.into_inner());
            }
            _ => {}
        }
    }
}

pub(super) fn record_completed_pairing(
    report: &mut LiveConnectReport,
    bond_table: &mut crate::bond::BondTable,
    security_level: SecurityLevel,
    bond: Option<BondInformation>,
) {
    report.security_level = Some(match security_level {
        SecurityLevel::NoEncryption => crate::bond::SecurityLevel::NoEncryption,
        SecurityLevel::Encrypted => crate::bond::SecurityLevel::Encrypted,
        SecurityLevel::EncryptedAuthenticated => crate::bond::SecurityLevel::EncryptedAuthenticated,
    });
    if let Some(bond) = bond {
        report.bonded = true;
        let record = crate::pairing::record_from_bond_information(&bond);
        if bond_table.insert(record).is_err() {
            report.bond_table_full = true;
        }
    }
}

pub(super) fn record_remote_disconnect(
    report: &mut LiveConnectReport,
    session_connection: &mut SessionConnection,
    now_ticks: u64,
    reason: u8,
) {
    let generation = session_connection.generation();
    session_connection
        .begin_disconnect(generation, now_ticks)
        .ok();
    session_connection.mark_disconnected(generation).ok();
    report.connection_state = session_connection.state();
    report.pairing_disconnect_reason = Some(reason);
}

pub(super) fn classify_service_discovery_error<E>(
    error: &BleHostError<E>,
) -> ServiceDiscoveryFailure {
    match error {
        BleHostError::Controller(_) => ServiceDiscoveryFailure::Controller,
        BleHostError::BleHost(Error::Att(code)) => ServiceDiscoveryFailure::Att(code.to_u8()),
        BleHostError::BleHost(Error::InsufficientSpace) => {
            ServiceDiscoveryFailure::InsufficientSpace
        }
        BleHostError::BleHost(Error::InvalidValue) => ServiceDiscoveryFailure::InvalidValue,
        BleHostError::BleHost(Error::UnexpectedGattResponse) => {
            ServiceDiscoveryFailure::UnexpectedGattResponse
        }
        BleHostError::BleHost(Error::InvalidUuidLength(length)) => {
            ServiceDiscoveryFailure::InvalidUuidLength(*length)
        }
        BleHostError::BleHost(_) => ServiceDiscoveryFailure::OtherHost,
    }
}

pub(super) async fn queued_disconnect_reason(
    connection: &Connection<'_, DefaultPacketPool>,
) -> Option<u8> {
    if connection.is_connected() {
        return None;
    }
    with_timeout(Duration::from_millis(50), async {
        loop {
            if let ConnectionEvent::Disconnected { reason } = connection.next().await {
                return reason.into_inner();
            }
        }
    })
    .await
    .ok()
}

pub(super) struct NameMatchHandler<'a> {
    pub(super) target_name: &'a str,
    pub(super) found: &'a Mutex<RefCell<Option<(AddrKind, [u8; 6])>>>,
}

impl EventHandler for NameMatchHandler<'_> {
    fn on_adv_reports(&self, reports: bt_hci::param::LeAdvReportsIter) {
        for report in reports.into_iter().flatten() {
            let name = parse_local_name(report.data);
            if name.is_empty() || !name.contains(self.target_name) {
                continue;
            }
            let address: [u8; 6] = report.addr.raw().try_into().unwrap_or([0; 6]);
            critical_section::with(|token| {
                let mut found = self.found.borrow_ref_mut(token);
                if found.is_none() {
                    *found = Some((report.addr_kind, address));
                }
            });
        }
    }
}

pub(super) fn is_hid_service(uuid: DiscoveredUuid) -> bool {
    uuid == DiscoveredUuid::Uuid16(HID_SERVICE_UUID)
}

pub(super) fn is_hid_report_map(uuid: DiscoveredUuid) -> bool {
    uuid == DiscoveredUuid::Uuid16(HID_REPORT_MAP_UUID)
}

pub(super) fn is_hid_report(uuid: DiscoveredUuid) -> bool {
    uuid == DiscoveredUuid::Uuid16(HID_REPORT_UUID)
}

pub(super) fn hid_report_reference_uuid() -> Uuid {
    Uuid::new_short(HID_REPORT_REFERENCE_UUID)
}
