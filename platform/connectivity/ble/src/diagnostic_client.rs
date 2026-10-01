//! A fake GATT client that exercises ADR-0011's diagnostic peripheral
//! specifically -- Build Info, Echo, Lifecycle Status by their own known
//! UUIDs -- rather than [`crate::live_connect`]'s generic "discover
//! everything, read the first readable characteristic" probe (shared BLE
//! runtime and roles plan's diagnostic-client validation).
//!
//! Same shape and the same real-outcomes-not-injected-state philosophy as
//! [`crate::live_connect`], and reuses its name-scan step directly (see
//! that module's doc comment for why address-free name matching, and for
//! the `with_timeout` treatment every remote call below repeats: neither
//! `Central::connect` nor a `GattClient` request honors its own configured
//! timeout on its own). Locates the Diagnostic Service and its
//! characteristics by [`crate::diagnostic`]'s own UUID constants
//! (`services_by_uuid`/`characteristic_by_uuid`) instead of bulk discovery,
//! since the target UUIDs are already known -- this also sidesteps
//! `live_connect`'s documented "bulk `characteristics()` does not retain
//! UUID" limitation entirely, because it never calls that method.
//!
//! Attempts, in order: name scan, connect, GATT client MTU exchange,
//! locate the Diagnostic Service, read Build Info, write-then-read-back
//! Echo (ADR-0011: "one equal-length response"), subscribe to and await one
//! Lifecycle Status notification, then disconnect. Stops at the first step
//! that fails and reports how far it got, same partial-progress-is-real-
//! evidence policy as `live_connect`.

use core::cell::RefCell;
use core::pin::pin;

use critical_section::Mutex;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use trouble_host::prelude::*;

use crate::diagnostic::{
    BuildInfo, DecodeError as DiagnosticDecodeError, LifecycleStatus, BUILD_INFO_UUID, ECHO_UUID,
    LIFECYCLE_STATUS_UUID, SERVICE_UUID,
};
use crate::live_scan::parse_local_name;

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;
const MAX_DISCOVERED_SERVICES: usize = 4;

type Controller = ExternalController<BleConnector<'static>, 1>;
type Resources = HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;

/// How far [`run`] got before stopping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticClientOutcome {
    /// Every attempted step (through disconnect) completed and every
    /// decode succeeded.
    Completed,
    ControllerInit,
    DeviceNotFound,
    ConnectFailed,
    GattClientInitFailed,
    /// The Diagnostic Service itself (by [`crate::diagnostic::SERVICE_UUID`])
    /// was not found, or more than one matched -- ADR-0011: "Multiple
    /// matching devices are an explicit ambiguous-device error", applied
    /// here to the service lookup rather than device discovery.
    ServiceNotFound,
    /// A known characteristic UUID was not found under the Diagnostic
    /// Service.
    CharacteristicNotFound,
    BuildInfoReadFailed,
    BuildInfoDecodeFailed,
    EchoWriteFailed,
    EchoReadbackFailed,
    /// The read-back payload did not equal what was written -- ADR-0011's
    /// "one equal-length response" contract violated.
    EchoMismatch,
    LifecycleSubscribeFailed,
    /// No Lifecycle Status notification arrived within `connect_timeout`
    /// after subscribing.
    LifecycleNotificationTimeout,
    LifecycleDecodeFailed,
    HostExited,
}

/// The outcome of [`run`], plus whatever it decoded before stopping.
pub struct DiagnosticClientReport {
    pub outcome: DiagnosticClientOutcome,
    pub address: Option<[u8; 6]>,
    pub build_info: Option<BuildInfo>,
    /// `Some(true)` once the write/read-back round trip succeeded and
    /// matched; `Some(false)` if it completed but mismatched; `None` if a
    /// step before or during it never completed.
    pub echo_round_trip_ok: Option<bool>,
    pub lifecycle_status: Option<LifecycleStatus>,
}

struct NameMatchHandler<'a> {
    target_name: &'a str,
    found: &'a Mutex<RefCell<Option<(AddrKind, [u8; 6])>>>,
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

enum HostRaceResult<T> {
    HostExited,
    Failed,
    Completed(T),
}

async fn race_with_runner<T, R, OR, O, E, OErr>(
    mut named_runner_future: core::pin::Pin<&mut R>,
    operation: O,
) -> HostRaceResult<T>
where
    R: core::future::Future<Output = OR>,
    O: core::future::Future<Output = Result<Result<T, E>, OErr>>,
{
    match select(named_runner_future.as_mut(), operation).await {
        Either::First(_) => HostRaceResult::HostExited,
        Either::Second(Err(_)) | Either::Second(Ok(Err(_))) => HostRaceResult::Failed,
        Either::Second(Ok(Ok(value))) => HostRaceResult::Completed(value),
    }
}

async fn race_with_runner_and_client<T, R, OR, C, CO, O, E, OErr>(
    mut named_runner_future: core::pin::Pin<&mut R>,
    mut client_task_future: core::pin::Pin<&mut C>,
    operation: O,
) -> HostRaceResult<T>
where
    R: core::future::Future<Output = OR>,
    C: core::future::Future<Output = CO>,
    O: core::future::Future<Output = Result<Result<T, E>, OErr>>,
{
    match select3(
        named_runner_future.as_mut(),
        client_task_future.as_mut(),
        operation,
    )
    .await
    {
        Either3::First(_) | Either3::Second(_) => HostRaceResult::HostExited,
        Either3::Third(Err(_)) | Either3::Third(Ok(Err(_))) => HostRaceResult::Failed,
        Either3::Third(Ok(Ok(value))) => HostRaceResult::Completed(value),
    }
}

fn complete_or_report<T>(
    result: HostRaceResult<T>,
    report: &mut DiagnosticClientReport,
    failed_outcome: DiagnosticClientOutcome,
) -> Option<T> {
    match result {
        HostRaceResult::Completed(value) => Some(value),
        HostRaceResult::HostExited => {
            report.outcome = DiagnosticClientOutcome::HostExited;
            None
        }
        HostRaceResult::Failed => {
            report.outcome = failed_outcome;
            None
        }
    }
}

/// Find, connect to, and exercise ADR-0011's diagnostic peripheral on
/// `target_name` (matched as a substring of the advertised local name),
/// then disconnect. `now_ticks` is the caller's monotonic clock in
/// microseconds, same convention as [`crate::live_connect::run`].
pub async fn run(
    device: BT<'static>,
    target_name: &str,
    scan_timeout: Duration,
    connect_timeout: Duration,
    now_ticks: fn() -> u64,
) -> DiagnosticClientReport {
    let mut report = DiagnosticClientReport {
        outcome: DiagnosticClientOutcome::ControllerInit,
        address: None,
        build_info: None,
        echo_round_trip_ok: None,
        lifecycle_status: None,
    };
    let _ = now_ticks; // reserved for a future generation-tagged session; see `live_connect` for why the parameter stays even where unused today.

    let connector = match BleConnector::new(device, Default::default()) {
        Ok(connector) => connector,
        Err(_) => return report,
    };
    let controller: Controller = ExternalController::new(connector);

    static RESOURCES: static_cell::StaticCell<Resources> = static_cell::StaticCell::new();
    let resources = RESOURCES.init(Resources::new());
    let stack = trouble_host::new(controller, resources)
        .set_random_address(Address::random([0xfe, 0x43, 0x4f, 0x4e, 0x4e, 0x02]))
        .build();
    let mut runner = stack.runner();
    let mut central = stack.central();

    // Scan: match the advertised local name.
    let found: Mutex<RefCell<Option<(AddrKind, [u8; 6])>>> = Mutex::new(RefCell::new(None));
    let handler = NameMatchHandler {
        target_name,
        found: &found,
    };
    let mut scanner = Scanner::new(&mut central);
    let scan_config = ScanConfig {
        active: true,
        ..Default::default()
    };
    // Polls for a match rather than unconditionally waiting out the full
    // `scan_timeout`: a target
    // that advertises forever tolerates the wait, but ADR-0011's diagnostic
    // peripheral stops advertising after its own bounded deadline -- waiting
    // out the whole scan window even after an instant match meant `connect`
    // below routinely started only once the peer had already stopped
    // advertising, surfacing as `ConnectFailed` rather than a timely connect.
    const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(200);
    let scan_body = async {
        match scanner.scan(&scan_config).await {
            Ok(session) => {
                let mut waited = Duration::from_millis(0);
                while critical_section::with(|token| found.borrow_ref(token).is_none()) {
                    if waited >= scan_timeout {
                        break;
                    }
                    Timer::after(SCAN_POLL_INTERVAL).await;
                    waited += SCAN_POLL_INTERVAL;
                }
                drop(session);
                Ok::<_, ()>(Ok(()))
            }
            Err(_) => Ok::<_, ()>(Err(())),
        }
    };
    let mut named_runner_future = pin!(async { runner.run_with_handler(&handler).await });
    if complete_or_report(
        race_with_runner(named_runner_future.as_mut(), scan_body).await,
        &mut report,
        DiagnosticClientOutcome::DeviceNotFound,
    )
    .is_none()
    {
        return report;
    }
    let target = critical_section::with(|token| *found.borrow_ref(token));
    let Some((addr_kind, address)) = target else {
        report.outcome = DiagnosticClientOutcome::DeviceNotFound;
        return report;
    };
    report.address = Some(address);
    drop(scanner);

    // Connect: keep the host runner polled until the peer responds.
    let target_address = Address::new(addr_kind, BdAddr::new(address));
    let connect_config = ConnectConfig {
        scan_config: ScanConfig {
            active: true,
            filter_accept_list: &[target_address],
            timeout: connect_timeout,
            ..Default::default()
        },
        connect_params: RequestedConnParams::default(),
    };
    let connect_body = with_timeout(connect_timeout, central.connect(&connect_config));
    let Some(connection) = complete_or_report(
        race_with_runner(named_runner_future.as_mut(), connect_body).await,
        &mut report,
        DiagnosticClientOutcome::ConnectFailed,
    ) else {
        return report;
    };

    // GATT setup: exchange MTU before discovery.
    let client_body = with_timeout(
        connect_timeout,
        GattClient::<Controller, DefaultPacketPool, MAX_DISCOVERED_SERVICES>::new(
            &stack,
            &connection,
        ),
    );
    let Some(client) = complete_or_report(
        race_with_runner(named_runner_future.as_mut(), client_body).await,
        &mut report,
        DiagnosticClientOutcome::GattClientInitFailed,
    ) else {
        return report;
    };

    // `GattClient::new` receives the MTU response directly, but every later
    // ATT response is forwarded to the request's private response channel by
    // `GattClient::task`. Keep that dispatcher continuously polled alongside
    // the host runner and each operation; without it, the first discovery
    // response reaches Trouble's connection queue but `services_by_uuid()`
    // waits forever on an empty response channel.
    let mut client_task_future = pin!(async { client.task().await });

    // Service discovery: locate the Diagnostic Service by its known UUID.
    let service_uuid = Uuid::new_long(SERVICE_UUID);
    let services_body = with_timeout(connect_timeout, client.services_by_uuid(&service_uuid));
    let Some(services) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            services_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::ServiceNotFound,
    ) else {
        return report;
    };
    let Some(service) = services.first() else {
        report.outcome = DiagnosticClientOutcome::ServiceNotFound;
        return report;
    };

    // Build Info: locate by UUID, then read.
    let build_info_uuid = Uuid::new_long(BUILD_INFO_UUID);
    let build_info_char_body = with_timeout(
        connect_timeout,
        client.characteristic_by_uuid::<[u8]>(service, &build_info_uuid),
    );
    let Some(build_info_char) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            build_info_char_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::CharacteristicNotFound,
    ) else {
        return report;
    };
    let mut build_info_buffer = [0u8; crate::capacity::BUILD_INFO_BYTES];
    let build_info_read_body = with_timeout(
        connect_timeout,
        client.read_characteristic(&build_info_char, &mut build_info_buffer),
    );
    if complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            build_info_read_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::BuildInfoReadFailed,
    )
    .is_none()
    {
        return report;
    }
    match BuildInfo::decode(&build_info_buffer) {
        Ok(build_info) => report.build_info = Some(build_info),
        Err(DiagnosticDecodeError::WrongLength | DiagnosticDecodeError::UnknownState) => {
            report.outcome = DiagnosticClientOutcome::BuildInfoDecodeFailed;
            return report;
        }
    }

    // Echo: locate by UUID, write a probe payload, read it back,
    // and confirm the round trip (ADR-0011: "one equal-length response").
    let echo_uuid = Uuid::new_long(ECHO_UUID);
    let echo_char_body = with_timeout(
        connect_timeout,
        client.characteristic_by_uuid::<[u8]>(service, &echo_uuid),
    );
    let Some(echo_char) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            echo_char_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::CharacteristicNotFound,
    ) else {
        return report;
    };
    const ECHO_PROBE_PAYLOAD: &[u8] = b"diagnostic-client-probe";
    let echo_write_body = with_timeout(
        connect_timeout,
        client.write_characteristic(&echo_char, ECHO_PROBE_PAYLOAD),
    );
    if complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            echo_write_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::EchoWriteFailed,
    )
    .is_none()
    {
        return report;
    }
    let mut echo_buffer = [0u8; crate::capacity::ECHO_PAYLOAD_MAX_BYTES];
    let echo_read_body = with_timeout(
        connect_timeout,
        client.read_characteristic(&echo_char, &mut echo_buffer),
    );
    let Some(echo_read_len) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            echo_read_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::EchoReadbackFailed,
    ) else {
        return report;
    };
    let matches = &echo_buffer[..echo_read_len] == ECHO_PROBE_PAYLOAD;
    report.echo_round_trip_ok = Some(matches);
    if !matches {
        report.outcome = DiagnosticClientOutcome::EchoMismatch;
        return report;
    }

    // Lifecycle Status: locate by UUID, subscribe, await one
    // notification.
    let lifecycle_uuid = Uuid::new_long(LIFECYCLE_STATUS_UUID);
    let lifecycle_char_body = with_timeout(
        connect_timeout,
        client.characteristic_by_uuid::<[u8]>(service, &lifecycle_uuid),
    );
    let Some(lifecycle_char) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            lifecycle_char_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::CharacteristicNotFound,
    ) else {
        return report;
    };
    let subscribe_body = with_timeout(connect_timeout, client.subscribe(&lifecycle_char, false));
    let Some(mut listener) = complete_or_report(
        race_with_runner_and_client(
            named_runner_future.as_mut(),
            client_task_future.as_mut(),
            subscribe_body,
        )
        .await,
        &mut report,
        DiagnosticClientOutcome::LifecycleSubscribeFailed,
    ) else {
        return report;
    };
    let notification_body = with_timeout(connect_timeout, listener.next());
    match select3(
        named_runner_future.as_mut(),
        client_task_future.as_mut(),
        notification_body,
    )
    .await
    {
        Either3::First(_) | Either3::Second(_) => {
            report.outcome = DiagnosticClientOutcome::HostExited;
            return report;
        }
        Either3::Third(Err(_)) => {
            report.outcome = DiagnosticClientOutcome::LifecycleNotificationTimeout;
            return report;
        }
        Either3::Third(Ok(notification)) => {
            let bytes: &[u8] = notification.as_ref();
            match LifecycleStatus::decode(bytes) {
                Ok(status) => report.lifecycle_status = Some(status),
                Err(_) => {
                    report.outcome = DiagnosticClientOutcome::LifecycleDecodeFailed;
                    return report;
                }
            }
        }
    }

    drop(connection);
    report.outcome = DiagnosticClientOutcome::Completed;
    report
}
