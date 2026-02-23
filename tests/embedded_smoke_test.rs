//! Minimal async embedded-test harness for xtensa/ESP32.
//! This validates test runtime wiring without touching SD-card hardware yet.

#![no_std]
#![no_main]

#[cfg(test)]
#[embedded_test::tests(executor = esp_rtos::embassy::Executor::new())]
mod tests {
    #[init]
    fn init() {
        let peripherals = esp_hal::init(esp_hal::Config::default());
        let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
        esp_rtos::start(timg0.timer0);
    }

    #[test]
    async fn harness_smoke_async() {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(10)).await;
        assert_eq!(2 + 2, 4);
    }
}
