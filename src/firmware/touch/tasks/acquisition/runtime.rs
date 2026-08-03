use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{Duration, Instant, Timer};

use crate::firmware::{
    config::APP_EVENTS,
    input::gpio36::{Gpio36Action, Gpio36Classifier},
    touch::{
        config::{TOUCH_IMU_STATUS, TOUCH_INIT_RETRY_MS},
        scheduling,
    },
    types::{AppEvent, Gpio36InputPin, InkplateTouchDriver, TouchSampleFrame, TouchStatus},
};

use super::{
    state::{authoritative_touch_count, ContactSamplingState},
    TouchAcquisitionCommand, TOUCH_ACQUISITION_COMMANDS,
};
use crate::firmware::touch::tasks::{push_touch_input_sample, request_touch_pipeline_reset};

const CLASSIFIER_PROBE_MS: u64 = 8;

pub(super) enum ProbeResult {
    Complete,
    Command(TouchAcquisitionCommand),
    Fault,
}

pub(super) enum AcquisitionWake {
    Command(TouchAcquisitionCommand),
    Asserted,
    RecoveryDue,
}

pub(super) async fn wait_for_assertion_or_recovery(
    gpio36: &mut Gpio36InputPin,
    poll_interval_ms: u64,
) -> AcquisitionWake {
    match select3(
        TOUCH_ACQUISITION_COMMANDS.receive(),
        gpio36.wait_for_low(),
        Timer::after_millis(poll_interval_ms),
    )
    .await
    {
        Either3::First(command) => AcquisitionWake::Command(command),
        Either3::Second(_) => AcquisitionWake::Asserted,
        Either3::Third(_) => AcquisitionWake::RecoveryDue,
    }
}

pub(super) async fn probe_asserted_line(
    touch: &mut InkplateTouchDriver,
    gpio36: &Gpio36InputPin,
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) -> ProbeResult {
    loop {
        if read_and_publish(
            touch,
            Instant::now().as_millis(),
            classifier,
            contact_sampling,
        )
        .await
        .is_err()
        {
            return ProbeResult::Fault;
        }
        if !classifier.is_pending() || !gpio36.is_low() {
            return ProbeResult::Complete;
        }
        // A held-low WAKE press has no touch report. Probe only while source
        // classification is pending; normal touch motion remains IRQ-driven.
        match select(
            TOUCH_ACQUISITION_COMMANDS.receive(),
            Timer::after_millis(CLASSIFIER_PROBE_MS),
        )
        .await
        {
            Either::First(command) => return ProbeResult::Command(command),
            Either::Second(_) => {}
        }
    }
}

pub(super) async fn read_and_publish(
    touch: &mut InkplateTouchDriver,
    t_ms: u64,
    classifier: &mut Gpio36Classifier,
    contact_sampling: &mut ContactSamplingState,
) -> Result<(), ()> {
    let sample = touch.read_sample(0).await.map_err(|_| ())?;
    let authoritative_count = authoritative_touch_count(sample.raw[0], sample.touch_count);
    publish_action(
        classifier.observe_touch_probe(t_ms, authoritative_count.is_some_and(|count| count > 0)),
    )
    .await;
    if let Some(touch_count) = authoritative_count {
        contact_sampling.record_authoritative_count(touch_count);
        scheduling::record_sample(t_ms, touch_count);
        let frame = TouchSampleFrame { t_ms, sample };
        push_touch_input_sample(frame).await;
    }
    Ok(())
}

pub(super) async fn handle_fault(
    touch: &mut InkplateTouchDriver,
    ready: &mut bool,
    retry_at: &mut Instant,
) {
    *ready = false;
    let _ = touch.shutdown().await;
    request_touch_pipeline_reset();
    *retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
    publish_touch_status(TouchStatus::Fault).await;
    esp_println::println!("touch: read_error; retrying");
}

pub(super) async fn publish_touch_status(status: TouchStatus) {
    TOUCH_IMU_STATUS.signal(status);
    APP_EVENTS.send(AppEvent::TouchStatus(status)).await;
}

pub(super) async fn publish_action(action: Option<Gpio36Action>) {
    if let Some(action @ (Gpio36Action::WakeButtonPressed | Gpio36Action::WakeButtonReleased)) =
        action
    {
        APP_EVENTS.send(AppEvent::Gpio36Action(action)).await;
    }
}
