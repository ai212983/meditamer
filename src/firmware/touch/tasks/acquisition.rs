use embassy_time::{Duration, Instant, Timer};

mod state;

use crate::drivers::inkplate::TouchInitStatus;
use crate::firmware::{
    config::APP_EVENTS,
    input::gpio36::{Gpio36Action, Gpio36Classifier, Gpio36Mode},
    types::{AppEvent, Gpio36InputPin, InkplateTouchDriver, TouchSampleFrame, TouchStatus},
};

use super::{push_touch_input_sample, request_touch_pipeline_reset};
use crate::firmware::touch::config::{
    GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED, TOUCH_IMU_STATUS, TOUCH_INIT_RETRY_MS,
};
use crate::firmware::touch::scheduling;
use state::AcquisitionTiming;

const GPIO_POLL_MS: u64 = 2;

#[embassy_executor::task]
pub(crate) async fn touch_acquisition_task(mut touch: InkplateTouchDriver, gpio36: Gpio36InputPin) {
    let mode = if GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
        Gpio36Mode::ButtonOnly
    } else {
        Gpio36Mode::SharedWithTouch
    };
    let mut classifier = Gpio36Classifier::new();
    let mut observed_low = gpio36.is_low();
    let mut touch_ready = false;
    let mut retry_at = Instant::now();
    let mut timing = AcquisitionTiming::new(Instant::now().as_millis());
    let mut stack_sample_at = Instant::now();

    if matches!(mode, Gpio36Mode::ButtonOnly) {
        let _ = touch.shutdown().await;
        publish_touch_status(TouchStatus::Fault).await;
    }

    loop {
        let now = Instant::now();
        if matches!(mode, Gpio36Mode::SharedWithTouch) && !touch_ready && now >= retry_at {
            publish_touch_status(TouchStatus::Initializing).await;
            match touch.init_with_status().await {
                Ok(TouchInitStatus::Ready { x_res, y_res }) => {
                    esp_println::println!(
                        "touch: ready phase=acquisition x_res={} y_res={}",
                        x_res,
                        y_res
                    );
                    touch_ready = true;
                    timing.reset_contact();
                    request_touch_pipeline_reset();
                    publish_touch_status(TouchStatus::Ready { x_res, y_res }).await;
                }
                status => {
                    let ext = touch.probe_external().await;
                    let controller = touch.probe_controller().await;
                    esp_println::println!(
                        "touch: init_failed phase=acquisition status={:?} probe_ext={} probe_touch={}",
                        status,
                        ext,
                        controller
                    );
                    let _ = touch.shutdown().await;
                    retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
                    publish_touch_status(TouchStatus::Fault).await;
                }
            }
        }

        let low = gpio36.is_low();
        if low != observed_low {
            observed_low = low;
            let edge_ms = Instant::now().as_millis();
            if low {
                esp_println::println!("input: gpio36 raw=low");
                publish_action(classifier.on_asserted(edge_ms, mode)).await;
                timing.on_asserted(edge_ms);
                if touch_ready
                    && read_and_publish(&mut touch, edge_ms, &mut classifier, &mut timing)
                        .await
                        .is_err()
                {
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
                }
            } else {
                esp_println::println!("input: gpio36 raw=high");
                timing.on_released();
                publish_action(classifier.on_released()).await;
                if touch_ready
                    && read_and_publish(&mut touch, edge_ms, &mut classifier, &mut timing)
                        .await
                        .is_err()
                {
                    handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
                }
            }
        }

        let now = Instant::now();
        let now_ms = now.as_millis();
        if timing.take_active_probe_due(now_ms, observed_low, touch_ready, classifier.is_pending())
        {
            if read_and_publish(&mut touch, now_ms, &mut classifier, &mut timing)
                .await
                .is_err()
            {
                handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
            }
        }

        if touch_ready && timing.take_release_confirm_due(now_ms) {
            if read_and_publish(&mut touch, now_ms, &mut classifier, &mut timing)
                .await
                .is_err()
            {
                handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
            }
        }

        if timing.take_idle_recovery_due(now_ms, touch_ready) {
            if read_and_publish(&mut touch, now_ms, &mut classifier, &mut timing)
                .await
                .is_err()
            {
                handle_fault(&mut touch, &mut touch_ready, &mut retry_at).await;
            }
        }

        let sleep_started_ms = Instant::now().as_millis();
        Timer::after_millis(GPIO_POLL_MS).await;
        let woke_at_ms = Instant::now().as_millis();
        let gap_ms = woke_at_ms.saturating_sub(sleep_started_ms);
        scheduling::record_loop_gap(gap_ms);
        if Instant::now() >= stack_sample_at {
            crate::firmware::telemetry::record_touch_core_stack_headroom();
            stack_sample_at = Instant::now() + Duration::from_secs(1);
        }
    }
}

async fn read_and_publish(
    touch: &mut InkplateTouchDriver,
    t_ms: u64,
    classifier: &mut Gpio36Classifier,
    timing: &mut AcquisitionTiming,
) -> Result<(), ()> {
    let sample = touch.read_sample(0).await.map_err(|_| ())?;
    scheduling::record_sample(t_ms, sample.touch_count);
    publish_action(classifier.observe_touch_probe(t_ms, sample.touch_count > 0)).await;
    timing.record_sample(t_ms, sample.touch_count);
    push_touch_input_sample(TouchSampleFrame { t_ms, sample }).await;
    Ok(())
}

async fn handle_fault(touch: &mut InkplateTouchDriver, ready: &mut bool, retry_at: &mut Instant) {
    *ready = false;
    let _ = touch.shutdown().await;
    request_touch_pipeline_reset();
    *retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
    publish_touch_status(TouchStatus::Fault).await;
    esp_println::println!("touch: read_error; retrying");
}

async fn publish_touch_status(status: TouchStatus) {
    TOUCH_IMU_STATUS.signal(status);
    APP_EVENTS.send(AppEvent::TouchStatus(status)).await;
}

async fn publish_action(action: Option<Gpio36Action>) {
    if let Some(action @ (Gpio36Action::WakeButtonPressed | Gpio36Action::WakeButtonReleased)) =
        action
    {
        APP_EVENTS.send(AppEvent::Gpio36Action(action)).await;
    }
}
