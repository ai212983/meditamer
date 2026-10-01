use embassy_time::Timer;

pub const SD_POWER_SETTLE_MS: u64 = 50;

pub async fn power_on_for_io<E, F>(mut power_on: F) -> Result<(), E>
where
    F: FnMut() -> Result<(), E>,
{
    let result = power_on();
    drop(power_on);
    result?;
    Timer::after_millis(SD_POWER_SETTLE_MS).await;
    Ok(())
}

pub fn power_off<E, F>(mut power_off: F) -> Result<(), E>
where
    F: FnMut() -> Result<(), E>,
{
    power_off()
}
