//! The existing UI owner serializes admission, application and repaint. Normal
//! rendering resumes after this bounded diagnostic operation completes.
use crate::firmware::types::DisplayContext;
use crate::firmware::{battery, environment, observation_fixture as fixture};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Instant, Timer};
use observation::{fixture::FixtureStatus, periodic::DemandControl, time::Instant as ObsInstant};

use super::super::state::DisplayLoopState;

async fn apply<S: fixture::FixtureSample, const N: usize>(
    control: &DemandControl<CriticalSectionRawMutex, S::Fields, N>,
    request: fixture::FixtureRequest,
    mode: fixture::FixtureMode,
    epoch: u32,
    once_pending: bool,
) -> Result<(), FixtureStatus> {
    let now = ObsInstant(Instant::now().as_millis());
    if now >= request.expires_at {
        return Err(FixtureStatus::Expired);
    }
    if !matches!(mode, fixture::FixtureMode::Periodic { .. }) {
        return Err(FixtureStatus::Invalid);
    }
    control.command(mode, request, S::fields(), now, epoch, once_pending)?;
    fixture::panel_wait::wait_for_application(
        control,
        &fixture::PERIODIC_CHANGED,
        request.id,
        Timer::at(Instant::from_millis(request.expires_at.0)),
    )
    .await?;
    if Instant::now().as_millis() >= request.expires_at.0 {
        return Err(FixtureStatus::Expired);
    }
    Ok(())
}

pub(super) async fn handle(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    let Some((provider, request, mode)) = fixture::try_receive_panel() else {
        return;
    };
    let applied = match provider {
        fixture::FixtureProvider::Battery => {
            apply::<battery::BatteryStateSnapshot, 1>(
                &battery::BATTERY_DEMAND,
                request,
                mode,
                battery::BATTERY_REQUESTS.close_generation(),
                state.battery_delivery.fixture_pending(),
            )
            .await
        }
        fixture::FixtureProvider::Bme688 => {
            apply::<environment::EnvironmentStateSnapshot, 2>(
                &environment::ENVIRONMENT_DEMAND,
                request,
                mode,
                environment::ENVIRONMENT_REQUESTS.close_generation(),
                state.environment_delivery.fixture_pending(),
            )
            .await
        }
    };
    if let Err(status) = applied {
        console::println!("PANEL_FIXTURE END id={} status={:?}", request.id, status);
        return;
    }
    super::repaint::handle_force_repaint_event(context, state, Some(request.id)).await;
}
