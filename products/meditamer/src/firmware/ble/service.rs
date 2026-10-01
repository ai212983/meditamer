use embassy_futures::select::{select3, Either3};
use embassy_time::{Duration, Timer};
use trouble_host::prelude::*;

use super::{
    log_phase1s_sample, now_micros, ticks_to_timeout, DiagnosticServer, Phase1PacketPool,
    ProbeRequest, SampleResult, DEADLINE_CONFIG, ONE_SECOND_TICKS, PHASE1S_ACTIVE_FREE,
    PHASE1S_CLOSE_REQUEST,
};
use core::sync::atomic::Ordering;

/// Advertises the diagnostic service and serves one bounded connection window.
pub(super) async fn advertise_and_serve_window(
    peripheral: &mut Peripheral<'_, super::BleController<'_>, Phase1PacketPool>,
    server: &DiagnosticServer<'_>,
    lifecycle: &mut ble::diagnostic::LifecycleStatusPublisher,
    request: ProbeRequest,
) -> SampleResult {
    let mut adv_data = [0u8; 31];
    let adv_data_len = match AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteServiceUuids128(&[ble::diagnostic::SERVICE_UUID]),
        ],
        &mut adv_data,
    ) {
        Ok(len) => len,
        Err(error) => {
            console::println!("BLE_PHASE1D state=adv_data_encode_error error={:?}", error);
            0
        }
    };
    let mut scan_data = [0u8; 31];
    let scan_data_len = match AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(b"Meditamer")],
        &mut scan_data,
    ) {
        Ok(len) => len,
        Err(error) => {
            console::println!("BLE_PHASE1D state=scan_data_encode_error error={:?}", error);
            0
        }
    };
    let params = AdvertisementParameters {
        interval_min: Duration::from_millis(250),
        interval_max: Duration::from_millis(250),
        ..Default::default()
    };
    let mut window = ble::deadline::VisibilityWindow::open(DEADLINE_CONFIG, now_micros())
        .expect("DEADLINE_CONFIG is ADR-0011's own defaults, always internally valid");

    console::println!(
        "BLE_PHASE1D state=advertising adv_len={} scan_len={}",
        adv_data_len,
        scan_data_len
    );
    let serve = async {
        let advertiser = peripheral
            .advertise(
                &params,
                Advertisement::ConnectableScannableUndirected {
                    adv_data: &adv_data[..adv_data_len],
                    scan_data: &scan_data[..scan_data_len],
                },
            )
            .await;
        let advertiser = match advertiser {
            Ok(advertiser) => advertiser,
            Err(error) => {
                console::println!("BLE_PHASE1D state=advertise_error error={:?}", error);
                return;
            }
        };
        console::println!("BLE_PHASE1D state=advertised waiting_for_connection=true");

        let accept_timeout = Timer::after(ticks_to_timeout(window.remaining_ticks(now_micros())));
        let connection = match select3(
            advertiser.accept(),
            accept_timeout,
            PHASE1S_CLOSE_REQUEST.wait(),
        )
        .await
        {
            Either3::First(Ok(connection)) => connection,
            Either3::First(Err(error)) => {
                console::println!("BLE_PHASE1D state=accept_error error={:?}", error);
                return;
            }
            Either3::Second(_) => {
                let reason = window.check_deadline(now_micros());
                console::println!("BLE_PHASE1D state=accept_timeout reason={:?}", reason);
                return;
            }
            Either3::Third(_) => {
                console::println!("BLE_PHASE1D state=close_requested_while_advertising");
                window.close();
                return;
            }
        };
        console::println!("BLE_PHASE1D state=connection_accepted");
        let conn = match connection.with_attribute_server(core::ops::Deref::deref(server)) {
            Ok(conn) => conn,
            Err(error) => {
                console::println!("BLE_PHASE1D state=attribute_server_error error={:?}", error);
                return;
            }
        };

        window.on_connected(now_micros());
        console::println!("BLE_PHASE1D state=connected");
        publish_lifecycle_status(server, &conn, lifecycle, &window).await;

        let mut echo_limiter = ble::diagnostic::EchoRateLimiter::new();
        loop {
            let deadline_timeout =
                Timer::after(ticks_to_timeout(window.remaining_ticks(now_micros())));
            match select3(conn.next(), deadline_timeout, PHASE1S_CLOSE_REQUEST.wait()).await {
                Either3::First(GattConnectionEvent::Disconnected { reason }) => {
                    console::println!("BLE_PHASE1D state=disconnected reason={:?}", reason);
                    window.close();
                    break;
                }
                Either3::First(GattConnectionEvent::Gatt {
                    event: GattEvent::Write(event),
                }) if event.handle() == server.diagnostic.echo.handle => {
                    window.on_activity(now_micros());
                    let mut echoed: heapless::Vec<u8, { ble::capacity::ECHO_PAYLOAD_MAX_BYTES }> =
                        heapless::Vec::new();
                    let payload_len = event.with_data(|_offset, data| {
                        let _ = echoed.extend_from_slice(data);
                        data.len()
                    });
                    console::println!("BLE_PHASE1D state=echo_write payload_len={}", payload_len);
                    let admitted =
                        echo_limiter.try_admit_write(payload_len, now_micros(), ONE_SECOND_TICKS);
                    match admitted {
                        Ok(()) => {
                            if let Ok(reply) = event.accept() {
                                reply.send().await;
                            }
                            let _ = server.diagnostic.echo.notify(&conn, &echoed, true).await;
                            console::println!("BLE_PHASE1D state=echo_notified");
                        }
                        Err(rejection) => {
                            console::println!(
                                "BLE_PHASE1D state=echo_write_rejected reason={:?}",
                                rejection
                            );
                            if let Ok(reply) = event.reject(AttErrorCode::WRITE_REQUEST_REJECTED) {
                                reply.send().await;
                            }
                        }
                    }
                }
                Either3::First(GattConnectionEvent::Gatt {
                    event: GattEvent::Write(event),
                }) if Some(event.handle()) == server.diagnostic.lifecycle_status.cccd_handle => {
                    window.on_activity(now_micros());
                    console::println!(
                        "BLE_PHASE1D state=lifecycle_cccd_write handle={}",
                        event.handle()
                    );
                    match event.accept() {
                        Ok(reply) => {
                            reply.send().await;
                            publish_lifecycle_status(server, &conn, lifecycle, &window).await;
                            console::println!("BLE_PHASE1D state=lifecycle_notified_after_cccd");
                        }
                        Err(error) => {
                            console::println!(
                                "BLE_PHASE1D state=lifecycle_cccd_accept_error error={:?}",
                                error
                            );
                        }
                    }
                }
                Either3::First(GattConnectionEvent::Gatt { event }) => {
                    window.on_activity(now_micros());
                    console::println!(
                        "BLE_PHASE1D state=gatt_event handle={:?}",
                        event.payload().handle()
                    );
                    console::println!("BLE_PHASE1D state=gatt_event_accepting");
                    match event.accept() {
                        Ok(reply) => {
                            console::println!(
                                "BLE_PHASE1D state=gatt_event_accepted sending_reply=true"
                            );
                            reply.send().await;
                            console::println!("BLE_PHASE1D state=gatt_event_reply_sent");
                        }
                        Err(error) => {
                            console::println!(
                                "BLE_PHASE1D state=gatt_event_accept_error error={:?}",
                                error
                            );
                        }
                    }
                }
                Either3::First(_) => {
                    console::println!("BLE_PHASE1D state=non_gatt_connection_event");
                }
                Either3::Second(_) => {
                    let Some(reason) = window.check_deadline(now_micros()) else {
                        continue;
                    };
                    console::println!("BLE_PHASE1D state=window_deadline reason={:?}", reason);
                    if matches!(reason, ble::deadline::WindowCloseReason::IdleTimeout) {
                        publish_lifecycle_status(server, &conn, lifecycle, &window).await;
                    }
                    break;
                }
                Either3::Third(_) => {
                    console::println!("BLE_PHASE1D state=close_requested_while_connected");
                    window.close();
                    break;
                }
            }
        }
        console::println!("BLE_PHASE1D state=serve_loop_exited");
    };

    serve.await;
    console::println!("BLE_PHASE1D state=serve_returned");
    let sample = log_phase1s_sample("active", request);
    PHASE1S_ACTIVE_FREE.store(sample.internal_free, Ordering::Relaxed);
    // The service window closes callback ingress before its resources return
    // to the controller lifecycle owner.
    esp_radio::ble::begin_hci_callback_shutdown();
    sample
}

async fn publish_lifecycle_status(
    server: &DiagnosticServer<'_>,
    conn: &GattConnection<'_, '_, Phase1PacketPool>,
    lifecycle: &mut ble::diagnostic::LifecycleStatusPublisher,
    window: &ble::deadline::VisibilityWindow,
) {
    let now = now_micros();
    lifecycle.publish(ble::diagnostic::LifecycleStatus {
        state: window.lifecycle_state(),
        remaining_seconds: (window.remaining_ticks(now) / ONE_SECOND_TICKS).min(u16::MAX as u64)
            as u16,
        ..lifecycle.latest()
    });
    if let Some(status) = lifecycle.take_pending_notification() {
        let _ = server
            .diagnostic
            .lifecycle_status
            .notify(conn, &status.encode(), true)
            .await;
    }
}
