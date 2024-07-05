use embassy_time::{Duration, Timer};
use esp_idf_svc::hal::delay::Delay;
use meditamer::Inkplate;
// see https://pg3.dev/post/13

// default Inkplate Arduino library uses I2C to set up the display
// https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/d33b0e7797eb42fdec34faf164216b547d32cbe3/src/boards/Inkplate4TEMPERA.cpp#L91

// Here's Espressif docs on I2C: https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/peripherals/i2c.html
// ESP-IDF Inkplate library is using i2c.h https://github.com/turgu1/ESP-IDF-InkPlate/blob/12aca9a26494a74b72b7c4014a05271c7be252f7/src/services/wire.cpp#L10

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting");

    let inkplate_instance = Inkplate::instance();
    let mut inkplate = inkplate_instance.lock().unwrap();

    inkplate.init();

    log::info!("Initialization complete, turning on the lights..");
    inkplate.set_brightness(32);

    let delay: Delay = Default::default();
    delay.delay_ms(1500);

    inkplate.frontlight_off();
    log::info!("Lights off");
    //block_on(async_main());
}

async fn async_main() {
    task().await;
}

async fn task() {
    loop {
        println!("Hello from a task");
        Timer::after(Duration::from_secs(1)).await;
    }
}
