use core::future::Future;
use core::pin::Pin;

use embassy_time::{with_timeout, Duration};
use heapless::Vec;
use trouble_host::prelude::*;

use super::protocol::{
    next_hid_stream_event, race_runtime, record_remote_disconnect, HidStreamEvent, RuntimeRace,
};
use super::runtime::SessionState;
use super::{CapturedHidReport, Controller, LiveConnectOutcome, MAX_DISCOVERED_SERVICES};

type Client<'a> = GattClient<'a, Controller<'a>, DefaultPacketPool, MAX_DISCOVERED_SERVICES>;

pub(super) async fn capture_hid_input<R, C>(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    client: &Client<'_>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    if state.options.hid_capture_duration.is_none() && state.options.hid_input_sink.is_none() {
        return true;
    }
    let Ok(mut listener) = client.listen_all() else {
        return true;
    };
    if !subscribe_reports(state, client, runner.as_mut(), client_task.as_mut()).await {
        return false;
    }
    stream_reports(
        state,
        connection,
        &mut listener,
        runner.as_mut(),
        client_task.as_mut(),
    )
    .await
}

async fn subscribe_reports<R, C>(
    state: &mut SessionState<'_, '_>,
    client: &Client<'_>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    for reference in state.report.hid_reports.iter() {
        let Some(cccd_handle) = reference.cccd_handle.filter(|_| reference.report_type == 1) else {
            continue;
        };
        let operation = with_timeout(
            state.options.connect_timeout,
            client.write_handle(cccd_handle, &[1, 0]),
        );
        match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
            RuntimeRace::Exited => {
                state.fail(LiveConnectOutcome::HostExited);
                return false;
            }
            RuntimeRace::Operation(Ok(Ok(()))) => {
                state.report.hid_subscriptions = state.report.hid_subscriptions.saturating_add(1);
            }
            RuntimeRace::Operation(Err(_)) | RuntimeRace::Operation(Ok(Err(_))) => {}
        }
    }
    true
}

async fn stream_reports<const MTU: usize, R, C>(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    listener: &mut NotificationListener<'_, MTU>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> bool
where
    R: Future,
    C: Future,
{
    let deadline = state
        .options
        .hid_capture_duration
        .map(|duration| (state.options.now_ticks)().saturating_add(duration.as_micros()));
    if let Some(sink) = state.options.hid_input_sink {
        sink.input_ready((state.options.now_ticks)());
    }

    loop {
        if capture_is_full(state) {
            break;
        }
        let event = match next_stream_event(
            state,
            connection,
            listener,
            deadline,
            runner.as_mut(),
            client_task.as_mut(),
        )
        .await
        {
            Some(event) => event,
            None if state.report.outcome == LiveConnectOutcome::HostExited => {
                close_sink(state);
                return false;
            }
            None => break,
        };
        if !record_stream_event(state, event) {
            return false;
        }
    }
    close_sink(state);
    true
}

async fn next_stream_event<const MTU: usize, R, C>(
    state: &mut SessionState<'_, '_>,
    connection: &Connection<'_, DefaultPacketPool>,
    listener: &mut NotificationListener<'_, MTU>,
    deadline: Option<u64>,
    mut runner: Pin<&mut R>,
    mut client_task: Pin<&mut C>,
) -> Option<HidStreamEvent<MTU>>
where
    R: Future,
    C: Future,
{
    loop {
        if state.stop_requested() {
            state.fail(LiveConnectOutcome::Cancelled);
            return None;
        }
        let wait = deadline
            .map(|absolute| {
                absolute
                    .saturating_sub((state.options.now_ticks)())
                    .min(Duration::from_millis(100).as_micros())
            })
            .unwrap_or(Duration::from_millis(100).as_micros());
        if wait == 0 {
            return None;
        }
        let operation = with_timeout(
            Duration::from_micros(wait),
            next_hid_stream_event(listener, connection),
        );
        match race_runtime(runner.as_mut(), client_task.as_mut(), operation).await {
            RuntimeRace::Exited => {
                state.fail(LiveConnectOutcome::HostExited);
                return None;
            }
            RuntimeRace::Operation(Ok(event)) => return Some(event),
            RuntimeRace::Operation(Err(_)) => {
                if deadline.is_some_and(|absolute| absolute <= (state.options.now_ticks)()) {
                    return None;
                }
            }
        }
    }
}

fn capture_is_full(state: &SessionState<'_, '_>) -> bool {
    state.options.hid_input_sink.is_none()
        && state.report.hid_notifications.len() >= crate::capacity::HID_NOTIFICATION_CAPTURE_MAX
}

fn record_stream_event<const MTU: usize>(
    state: &mut SessionState<'_, '_>,
    event: HidStreamEvent<MTU>,
) -> bool {
    let notification = match event {
        HidStreamEvent::Notification(notification) => notification,
        HidStreamEvent::Disconnected(reason) => {
            close_sink(state);
            record_remote_disconnect(
                &mut state.report,
                &mut state.connection,
                (state.options.now_ticks)(),
                reason,
            );
            state.fail(LiveConnectOutcome::DisconnectedDuringHidCapture);
            return false;
        }
    };
    let Some(reference) = state
        .report
        .hid_reports
        .iter()
        .find(|reference| reference.characteristic_handle == notification.handle())
    else {
        return true;
    };
    if let Some(sink) = state.options.hid_input_sink {
        sink.report(
            reference.report_id,
            (state.options.now_ticks)(),
            notification.as_ref(),
        );
    }
    let mut bytes = Vec::new();
    if bytes.extend_from_slice(notification.as_ref()).is_err() {
        return true;
    }
    let _ = state.report.hid_notifications.push(CapturedHidReport {
        characteristic_handle: notification.handle(),
        report_id: reference.report_id,
        captured_at_us: (state.options.now_ticks)(),
        bytes,
    });
    true
}

fn close_sink(state: &SessionState<'_, '_>) {
    if let Some(sink) = state.options.hid_input_sink {
        sink.input_closed((state.options.now_ticks)());
    }
}
