use core::future::Future;
use core::pin::Pin;

use embassy_futures::select::{select, Either};
use embassy_time::with_timeout;
use heapless::Vec;
use trouble_host::prelude::*;

use super::protocol::{
    classify_service_discovery_error, hid_report_reference_uuid, is_hid_report, is_hid_report_map,
    is_hid_service, queued_disconnect_reason, race_runtime, record_remote_disconnect, RuntimeRace,
};
use super::runtime::{pair_connection, SessionState};
use super::{
    Controller, DiscoveredCharacteristic, DiscoveredService, DiscoveredUuid, HidReportReference,
    LiveConnectOutcome, ServiceDiscoveryDiagnostic, ServiceDiscoveryFailure,
    MAX_CHARACTERISTICS_PER_SERVICE, MAX_DISCOVERED_SERVICES, READ_BUFFER_MAX,
};
use crate::gatt::{CharacteristicEntry, GattSession, ServiceEntry};

type Client<'a> = GattClient<'a, Controller<'a>, DefaultPacketPool, MAX_DISCOVERED_SERVICES>;

struct DiscoveryContext<'options, 'session, 'state, 'client> {
    state: &'state mut SessionState<'options, 'session>,
    client: &'client Client<'client>,
    gatt_session: &'state mut GattSession,
}

pub(super) async fn discover_services<R, C>(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    client: &Client<'_>,
    gatt_session: &mut GattSession,
    paired_this_call: &mut bool,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> Option<Vec<ServiceHandle, MAX_DISCOVERED_SERVICES>>
where
    R: Future,
    C: Future,
{
    loop {
        let started_at = (state.options.now_ticks)();
        let disconnected = async {
            loop {
                if let ConnectionEvent::Disconnected { reason } = connection.next().await {
                    return reason.into_inner();
                }
            }
        };
        let operation = select(
            with_timeout(state.options.connect_timeout, client.services()),
            disconnected,
        );
        let result = match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
            RuntimeRace::Exited => {
                state.fail(LiveConnectOutcome::HostExited);
                return None;
            }
            RuntimeRace::Operation(result) => result,
        };
        match result {
            Either::First(Ok(Ok(services))) => return Some(services),
            Either::Second(reason) => {
                record_remote_disconnect(
                    &mut state.report,
                    &mut state.connection,
                    (state.options.now_ticks)(),
                    reason,
                );
                record_discovery_failure(
                    state,
                    connection,
                    started_at,
                    ServiceDiscoveryFailure::Disconnected(reason),
                    false,
                    Some(reason),
                );
                gatt_session.finish_discovery(false);
                state.fail(LiveConnectOutcome::DisconnectedDuringServiceDiscovery);
                return None;
            }
            Either::First(Err(_)) => {
                let link_connected = connection.is_connected();
                let reason = queued_disconnect_reason(connection).await;
                record_discovery_failure(
                    state,
                    connection,
                    started_at,
                    ServiceDiscoveryFailure::Timeout,
                    link_connected,
                    reason,
                );
                gatt_session.finish_discovery(false);
                state.fail(LiveConnectOutcome::ServiceDiscoveryFailed);
                return None;
            }
            Either::First(Ok(Err(error))) => {
                if should_pair_after(&error, state.options.request_pairing, *paired_this_call) {
                    *paired_this_call = true;
                    if !pair_connection(state, connection, runner.as_mut()).await {
                        return None;
                    }
                    continue;
                }
                let link_connected = connection.is_connected();
                let reason = queued_disconnect_reason(connection).await;
                let failure = classify_service_discovery_error(&error);
                record_discovery_failure(
                    state,
                    connection,
                    started_at,
                    failure,
                    link_connected,
                    reason,
                );
                gatt_session.finish_discovery(false);
                state.fail(LiveConnectOutcome::ServiceDiscoveryFailed);
                return None;
            }
        }
    }
}

fn should_pair_after<E>(error: &BleHostError<E>, request_pairing: bool, paired: bool) -> bool {
    let needs_security = matches!(
        error,
        BleHostError::BleHost(Error::Att(code))
            if *code == AttErrorCode::INSUFFICIENT_AUTHENTICATION
                || *code == AttErrorCode::INSUFFICIENT_ENCRYPTION
    );
    request_pairing && needs_security && !paired
}

fn record_discovery_failure(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    started_at: u64,
    failure: ServiceDiscoveryFailure,
    link_connected: bool,
    disconnect_reason: Option<u8>,
) {
    state.report.service_discovery_diagnostic = Some(ServiceDiscoveryDiagnostic {
        failure,
        elapsed_us: (state.options.now_ticks)().saturating_sub(started_at),
        att_mtu: connection.att_mtu(),
        link_connected,
        disconnect_reason,
    });
}

pub(super) async fn inspect_services<R, C>(
    state: &mut SessionState<'_, '_>,
    client: &Client<'_>,
    gatt_session: &mut GattSession,
    services: &[ServiceHandle],
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let mut context = DiscoveryContext {
        state,
        client,
        gatt_session,
    };
    let mut read_done = false;
    for service in services {
        if !inspect_service(
            &mut context,
            service,
            &mut read_done,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await
        {
            return false;
        }
    }
    true
}

async fn inspect_service<R, C>(
    context: &mut DiscoveryContext<'_, '_, '_, '_>,
    service: &ServiceHandle,
    read_done: &mut bool,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let uuid = DiscoveredUuid::from(service.uuid());
    let _ = context.state.report.services.push(DiscoveredService {
        uuid,
        start_handle: *service.handle_range().start(),
        end_handle: *service.handle_range().end(),
    });
    let uuid16 = match uuid {
        DiscoveredUuid::Uuid16(value) => value,
        DiscoveredUuid::Uuid32(_) | DiscoveredUuid::Uuid128(_) => 0,
    };
    let _ = context.gatt_session.discover_service(ServiceEntry {
        uuid16,
        start_handle: *service.handle_range().start(),
        end_handle: *service.handle_range().end(),
    });

    let operation = with_timeout(
        context.state.options.connect_timeout,
        context
            .client
            .characteristics::<MAX_CHARACTERISTICS_PER_SERVICE>(service),
    );
    let characteristics = match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await
    {
        RuntimeRace::Exited => {
            context.state.fail(LiveConnectOutcome::HostExited);
            return false;
        }
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => return true,
        RuntimeRace::Operation(Ok(Ok(characteristics))) => characteristics,
    };

    for characteristic in characteristics.iter() {
        if !inspect_characteristic(
            context,
            uuid,
            characteristic,
            read_done,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await
        {
            return false;
        }
    }
    true
}

async fn inspect_characteristic<R, C>(
    context: &mut DiscoveryContext<'_, '_, '_, '_>,
    service_uuid: DiscoveredUuid,
    characteristic: &Characteristic<[u8]>,
    read_done: &mut bool,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let uuid = DiscoveredUuid::from(characteristic.uuid);
    let readable = characteristic.props.any(&[CharacteristicProp::Read]);
    let writable = characteristic.props.any(&[
        CharacteristicProp::Write,
        CharacteristicProp::WriteWithoutResponse,
    ]);
    let notifiable = characteristic.props.any(&[CharacteristicProp::Notify]);
    let _ = context
        .state
        .report
        .characteristics
        .push(DiscoveredCharacteristic {
            uuid,
            handle: characteristic.handle,
            readable,
            writable,
            notifiable,
        });

    if is_hid_service(service_uuid)
        && is_hid_report_map(uuid)
        && readable
        && !read_hid_report_map(
            context,
            characteristic,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await
    {
        return false;
    }
    if is_hid_service(service_uuid)
        && is_hid_report(uuid)
        && !read_hid_report_reference(
            context,
            characteristic,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await
    {
        return false;
    }

    let _ = context
        .gatt_session
        .discover_characteristic(CharacteristicEntry {
            uuid16: 0,
            value_handle: characteristic.handle,
            readable,
            writable,
            notifiable,
        });
    if readable && !*read_done {
        *read_done = true;
        return read_first_value(
            context,
            characteristic,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await;
    }
    true
}

async fn read_hid_report_map<R, C>(
    context: &mut DiscoveryContext<'_, '_, '_, '_>,
    characteristic: &Characteristic<[u8]>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let mut map = [0u8; crate::capacity::HID_DESCRIPTOR_MAX_BYTES];
    let operation = with_timeout(
        context.state.options.connect_timeout,
        context.client.read_characteristic(characteristic, &mut map),
    );
    match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
        RuntimeRace::Exited => {
            context.state.fail(LiveConnectOutcome::HostExited);
            false
        }
        RuntimeRace::Operation(Ok(Ok(len))) => {
            let _ = context
                .state
                .report
                .hid_report_map
                .extend_from_slice(&map[..len]);
            true
        }
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => true,
    }
}

async fn read_hid_report_reference<R, C>(
    context: &mut DiscoveryContext<'_, '_, '_, '_>,
    characteristic: &Characteristic<[u8]>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let reference_uuid = hid_report_reference_uuid();
    let operation = with_timeout(
        context.state.options.connect_timeout,
        context
            .client
            .descriptor_by_uuid::<_, [u8]>(characteristic, &reference_uuid),
    );
    let descriptor = match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
        RuntimeRace::Exited => {
            context.state.fail(LiveConnectOutcome::HostExited);
            return false;
        }
        RuntimeRace::Operation(Ok(Ok(descriptor))) => descriptor,
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => return true,
    };

    let mut value = [0u8; 2];
    let operation = with_timeout(
        context.state.options.connect_timeout,
        context.client.read_descriptor(&descriptor, &mut value),
    );
    match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
        RuntimeRace::Exited => {
            context.state.fail(LiveConnectOutcome::HostExited);
            false
        }
        RuntimeRace::Operation(Ok(Ok(2))) => {
            let _ = context.state.report.hid_reports.push(HidReportReference {
                characteristic_handle: characteristic.handle,
                cccd_handle: characteristic.cccd_handle,
                report_id: value[0],
                report_type: value[1],
            });
            true
        }
        RuntimeRace::Operation(Err(_))
        | RuntimeRace::Operation(Ok(Err(_)))
        | RuntimeRace::Operation(Ok(Ok(_))) => true,
    }
}

async fn read_first_value<R, C>(
    context: &mut DiscoveryContext<'_, '_, '_, '_>,
    characteristic: &Characteristic<[u8]>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let handle = characteristic.handle;
    let deadline = (context.state.options.now_ticks)()
        .saturating_add(context.state.options.connect_timeout.as_micros());
    if context
        .gatt_session
        .begin_operation(handle, crate::gatt::OperationKind::Read, 0, deadline)
        .is_err()
    {
        return true;
    }

    let mut buffer = [0u8; READ_BUFFER_MAX];
    let operation = with_timeout(
        context.state.options.connect_timeout,
        context
            .client
            .read_characteristic(characteristic, &mut buffer),
    );
    match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
        RuntimeRace::Exited => {
            context.state.fail(LiveConnectOutcome::HostExited);
            false
        }
        RuntimeRace::Operation(Ok(Ok(len))) => {
            let _ = context.gatt_session.complete_operation(handle, 0);
            context.state.report.read_handle = Some(handle);
            let end = len.min(READ_BUFFER_MAX);
            let _ = context
                .state
                .report
                .read_bytes
                .extend_from_slice(&buffer[..end]);
            true
        }
        RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => {
            let _ = context.gatt_session.complete_operation(handle, 0);
            true
        }
    }
}
