use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

#[derive(Clone, Copy)]
pub(super) enum ImuAcquisitionCommand {
    Suspend,
    Resume,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImuAcquisitionState {
    Suspended,
    Running,
}

static COMMANDS: Channel<CriticalSectionRawMutex, ImuAcquisitionCommand, 2> = Channel::new();
static STATE: Signal<CriticalSectionRawMutex, ImuAcquisitionState> = Signal::new();

pub(crate) async fn suspend_imu_acquisition() {
    COMMANDS.send(ImuAcquisitionCommand::Suspend).await;
    while STATE.wait().await != ImuAcquisitionState::Suspended {}
}

pub(crate) async fn resume_imu_acquisition() {
    COMMANDS.send(ImuAcquisitionCommand::Resume).await;
    while STATE.wait().await != ImuAcquisitionState::Running {}
}

pub(super) async fn receive_command() -> ImuAcquisitionCommand {
    COMMANDS.receive().await
}

pub(super) async fn handle_control_command(command: ImuAcquisitionCommand) {
    if !matches!(command, ImuAcquisitionCommand::Suspend) {
        return;
    }

    // Acknowledge only after any in-flight IMU I2C transaction has completed.
    // The display task waits for this state before powering the panel.
    STATE.signal(ImuAcquisitionState::Suspended);
    while let ImuAcquisitionCommand::Suspend = COMMANDS.receive().await {
        STATE.signal(ImuAcquisitionState::Suspended);
    }
    STATE.signal(ImuAcquisitionState::Running);
}
