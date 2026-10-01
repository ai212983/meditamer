use super::bounded_control;
use super::{battery, environment, imu, touch::tasks};

const KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM: bool =
    option_env!("MEDITAMER_TOUCH_CLOSED_DURING_PANEL_WAVEFORM").is_some();

pub(crate) const fn touch_waveform_window_mode() -> &'static str {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        "closed"
    } else {
        "open"
    }
}

/// Establish an acknowledged quiet window for all five shared-bus clients.
/// Each correlated wait has a 2-second budget. Cleanup failure, timeout,
/// supersession or identity exhaustion rejects the protected operation.
/// Always call `resume_clients` after either result so every participant has
/// retained Running intent even when another participant is unavailable.
pub(crate) async fn suspend_clients() -> bool {
    let imu_ack = imu::suspend_imu_acquisition().await;
    let environment_ack = environment::suspend_environment_acquisition().await;
    let battery_ack = battery::suspend_battery_acquisition().await;
    let touch_acquisition_ack = tasks::suspend_touch_acquisition().await;
    // Acquisition can already have queued a contact frame. Pause the
    // higher-priority pipeline as well so no touch work runs between the
    // timing-sensitive full-frame scan passes.
    let touch_pipeline_ack = tasks::suspend_touch_pipeline().await;

    let acks = [
        imu_ack,
        environment_ack,
        battery_ack,
        touch_acquisition_ack,
        touch_pipeline_ack,
    ];
    let quiesced = bounded_control::all_permit_shared_bus_access(acks);
    if !quiesced {
        console::println!(
            "PANEL_BUS_SUSPEND imu={:?} environment={:?} battery={:?} touch_acquisition={:?} touch_pipeline={:?}",
            imu_ack,
            environment_ack,
            battery_ack,
            touch_acquisition_ack,
            touch_pipeline_ack,
        );
    }
    quiesced
}

/// Restore the pipeline before acquisition can enqueue a post-resume sample.
/// Each participant receives persistent Running intent before its independent
/// bounded wait. Timeout stops waiting, but recovery remains pending until the
/// participant runs or a newer control request supersedes it. Touch reset
/// acknowledgement confirms enqueue, not consumption, by the pipeline.
pub(crate) async fn resume_clients(reset_touch_pipeline: bool) -> bool {
    let touch_pipeline_ok = tasks::resume_touch_pipeline().await;
    let touch_acquisition_ok = tasks::resume_touch_acquisition(reset_touch_pipeline).await;
    let environment_ok = environment::resume_environment_acquisition().await;
    let battery_ok = battery::resume_battery_acquisition().await;
    let imu_ok = imu::resume_imu_acquisition().await;
    if !(touch_pipeline_ok && touch_acquisition_ok && environment_ok && battery_ok && imu_ok) {
        console::println!(
            "PANEL_BUS_RESUME touch_pipeline={} touch_acquisition={} environment={} battery={} imu={}",
            touch_pipeline_ok,
            touch_acquisition_ok,
            environment_ok,
            battery_ok,
            imu_ok,
        );
    }
    touch_pipeline_ok && touch_acquisition_ok && environment_ok && battery_ok && imu_ok
}

/// Release a long-running non-panel transaction without making its command
/// response wait for the first post-resume touch-controller sample.
pub(crate) fn try_request_clients_resume(reset_touch_pipeline: bool) -> bool {
    let pipeline = tasks::try_request_touch_pipeline_resume();
    let acquisition = tasks::try_request_touch_acquisition_resume(reset_touch_pipeline);
    let imu = imu::try_request_imu_acquisition_resume();
    let environment = environment::try_request_environment_acquisition_resume();
    let battery = battery::try_request_battery_acquisition_resume();
    pipeline && acquisition && imu && environment && battery
}

/// Reopen touch processing only while the panel runs its GPIO waveform. The
/// shared-I2C mutex serializes inter-frame touch reads with vscan setup.
/// This window is opportunistic, not gating, for the panel scan already
/// underway, so a bounded timeout here is logged rather than propagated.
pub(crate) async fn open_touch_waveform_window() {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        return;
    }
    if !tasks::resume_touch_pipeline().await {
        console::println!("PANEL_BUS_WAVEFORM_WINDOW client=touch_pipeline status=timeout");
    }
    if !tasks::request_touch_acquisition_resume(false).await {
        console::println!("PANEL_BUS_WAVEFORM_WINDOW client=touch_acquisition status=timeout");
    }
}

/// Close touch before panel finalization accesses the shared PMIC/expander
/// bus. Failure requires GPIO-only isolation and deferred rail cleanup; it does
/// not authorize normal finalization on a bus whose owner may still be active.
pub(crate) async fn close_touch_waveform_window() -> bool {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        return true;
    }
    let acquisition = tasks::suspend_touch_acquisition().await;
    let pipeline = tasks::suspend_touch_pipeline().await;
    let quiesced = acquisition.permits_shared_bus_access() && pipeline.permits_shared_bus_access();
    if !quiesced {
        console::println!(
            "PANEL_BUS_WAVEFORM_CLOSE touch_acquisition={:?} touch_pipeline={:?} status=failed cleanup=pending",
            acquisition,
            pipeline,
        );
    }
    quiesced
}
