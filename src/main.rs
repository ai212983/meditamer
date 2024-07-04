use embassy_time::{Duration, Timer};
use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_svc::hal::{
    delay::{Delay, BLOCK},
    i2c::{I2cConfig, I2cDriver, Operation},
    peripherals::Peripherals,
    prelude::*,
    task::block_on,
};
use std::sync::Mutex;

// see https://pg3.dev/post/13

// default Inkplate Arduino library uses I2C to set up the display
// https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/d33b0e7797eb42fdec34faf164216b547d32cbe3/src/boards/Inkplate4TEMPERA.cpp#L91

// Here's Espressif docs on I2C: https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/peripherals/i2c.html
// ESP-IDF Inkplate library is using i2c.h https://github.com/turgu1/ESP-IDF-InkPlate/blob/12aca9a26494a74b72b7c4014a05271c7be252f7/src/services/wire.cpp#L10

const DEVICE_ADDRESS: u8 = 0x48;
const BRIGHTNESS_ADDRESS: u8 = 0x2E; // 0x5C >> 1;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting");

    let peripherals = Peripherals::take().unwrap();
    let sda = peripherals.pins.gpio21;
    let scl = peripherals.pins.gpio22;
    let config = I2cConfig::new().baudrate(100.kHz().into());
    let i2c_driver = I2cDriver::new(peripherals.i2c0, sda, scl, &config).unwrap();
    let i2c_mutex = Mutex::new(i2c_driver);
    let i2c_bus = MutexDevice::new(&i2c_mutex);

    let mut pcal6416a = port_expander::Pcal6416a::new(i2c_bus, false);
    let pca_pins = pcal6416a.split();

    // prepare the board
    // VCOM 5 // GPIOA6
    pca_pins.io0_5.into_output().unwrap();
    // PWRUP 4 // GPIOA4
    pca_pins.io0_4.into_output().unwrap();
    // WAKEUP 3 // GPIOA3
    let mut wakeup = pca_pins.io0_3.into_output().unwrap();
    //  GPIO0_ENABLE  8
    let mut io1_0 = pca_pins.io1_0.into_output().unwrap();
    io1_0.set_high().unwrap();
    log::info!("Board initialized, sending power up sequence");

    let delay: Delay = Default::default();
    wakeup.set_high().unwrap();
    delay.delay_ms(5);
    {
        let mut i2c = i2c_mutex.lock().unwrap();
        log::info!("i2c acquired, starting up...");
        // Wire.beginTransmission(0x38); is Arduino protocol to communicate with I2C -https://www.arduino.cc/reference/en/language/functions/communication/wire/
        // https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/d33b0e7797eb42fdec34faf164216b547d32cbe3/src/boards/Inkplate4TEMPERA.cpp#L11
        //    0x09,       // Register address
        //0b00011011, // Power up sequence
        //0b00000000, // Power up delay (3ms per rail)
        //0b00011011, // Power down sequence
        //0b00000000, // Power down delay (6ms per rail)
        //
        i2c.transaction(
            DEVICE_ADDRESS,
            &mut [Operation::Write(&[
                0x09, 0b00011011, 0b00000000, 0b00011011, 0b00000000,
            ])],
            BLOCK,
        )
        .unwrap();
        log::info!("start up complete");
    }
    delay.delay_ms(5);
    wakeup.set_low().unwrap();

    // #define FRONTLIGHT_EN 10
    let mut frontlight = pca_pins.io1_2.into_output().unwrap();
    frontlight.set_high().unwrap();
    {
        let mut i2c = i2c_mutex.lock().unwrap();
        log::info!("i2c acquired, setting up brightness...");
        // https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/d33b0e7797eb42fdec34faf164216b547d32cbe3/src/include/Frontlight.cpp#L34

        let brightness: u8 = 32;
        // this only should be invoked when the frontlight is on
        i2c.transaction(
            BRIGHTNESS_ADDRESS,
            &mut [Operation::Write(&[0x00, 63 - (brightness & 0b00111111)])],
            BLOCK,
        )
        .unwrap();
        log::info!("brightness set up");
    }
    delay.delay_ms(1500);
    frontlight.set_low().unwrap();
    block_on(async_main());
}
// TODO(df): continue from here https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/d33b0e7797eb42fdec34faf164216b547d32cbe3/src/Inkplate.cpp#L211
// we need to implement readPowerGood
//
// TPS65186 eink display is used
// https://docs.rs/embedded-graphics-core/latest/embedded_graphics_core/draw_target/trait.DrawTarget.html
//

async fn async_main() {
    task().await;
}

async fn task() {
    loop {
        println!("Hello from a task");
        Timer::after(Duration::from_secs(1)).await;
    }
}
