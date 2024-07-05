use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_svc::hal::{
    delay::{Delay, BLOCK},
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
    prelude::*,
};
use lazy_static::lazy_static;
use port_expander::{dev::pcal6416a, Pcal6416a};
use std::sync::{Arc, Mutex};

const DEVICE_ADDRESS: u8 = 0x48;
const BRIGHTNESS_ADDRESS: u8 = 0x2E; // 0x5C >> 1;

type I2cBus<'a> = MutexDevice<'a, I2cDriver<'a>>;
type PortMutexInkplate<'a> = Mutex<pcal6416a::Driver<I2cBus<'a>>>;

pub struct Inkplate {
    i2c: Arc<Mutex<I2cDriver<'static>>>,
    pins: Pcal6416a<PortMutexInkplate<'static>>,
}

lazy_static! {
    static ref I2C_MUTEX: Arc<Mutex<I2cDriver<'static>>> = {
        let peripherals = Peripherals::take().unwrap();
        let sda = peripherals.pins.gpio21;
        let scl = peripherals.pins.gpio22;
        let config = I2cConfig::new().baudrate(100.kHz().into());
        let i2c_driver = I2cDriver::new(peripherals.i2c0, sda, scl, &config).unwrap();
        Arc::new(Mutex::new(i2c_driver))
    };
    static ref INKPLATE_INSTANCE: Arc<Mutex<Inkplate>> = {
        let i2c_bus = MutexDevice::new(&*I2C_MUTEX);

        Arc::new(Mutex::new(Inkplate {
            i2c: Arc::clone(&I2C_MUTEX),
            pins: Pcal6416a::with_mutex(i2c_bus, false),
        }))
    };
}

impl Inkplate {
    pub fn instance() -> Arc<Mutex<Inkplate>> {
        INKPLATE_INSTANCE.clone()
    }

    pub fn init(&mut self) {
        {
            let pins = self.pins.split();
            pins.io0_5.into_output().unwrap(); // VCOM 5 // GPIOA6
            pins.io0_4.into_output().unwrap(); // PWRUP 4 // GPIOA4
            let mut wakeup = pins.io0_3.into_output().unwrap(); // WAKEUP 3 // GPIOA3
            let mut io1_0 = pins.io1_0.into_output().unwrap(); //  GPIO0_ENABLE  8
            io1_0.set_high().unwrap();

            // Board initialized, sending power up sequence
            wakeup.set_high().unwrap();
        }
        let delay: Delay = Default::default();
        delay.delay_ms(5);
        self.i2c
            .lock()
            .unwrap()
            .write(
                DEVICE_ADDRESS,
                &[0x09, 0b00011011, 0b00000000, 0b00011011, 0b00000000],
                BLOCK,
            )
            .unwrap();
        delay.delay_ms(5);
        self.pins
            .split()
            .io0_3
            .into_output()
            .unwrap()
            .set_low()
            .unwrap();
    }

    pub fn eink_on(&mut self) {
        self.pins
            .split()
            .io0_3
            .into_output()
            .unwrap()
            .set_high()
            .unwrap(); // WAKEUP 3 // GPIOA3
        let delay: Delay = Default::default();
        delay.delay_ms(5);
        self.i2c
            .lock()
            .unwrap() // Modify power up sequence  (VEE and VNEG are swapped)
            .write(DEVICE_ADDRESS, &[0x09, 0b11100001], BLOCK)
            .unwrap();
        self.i2c
            .lock()
            .unwrap() // Enable all rails
            .write(DEVICE_ADDRESS, &[0x01, 0b00111111], BLOCK)
            .unwrap();

        // TODO: PWRUP_SET
    }

    pub fn set_brightness(&mut self, brightness: u8) {
        self.frontlight_on();
        let mut i2c = self.i2c.lock().unwrap();
        i2c.write(
            BRIGHTNESS_ADDRESS,
            &[0x00, 63 - (brightness & 0b00111111)],
            BLOCK,
        )
        .unwrap();
    }

    // #define FRONTLIGHT_EN 10
    pub fn frontlight_on(&mut self) {
        self.pins
            .split()
            .io1_2
            .into_output()
            .unwrap()
            .set_high()
            .unwrap();
    }

    pub fn frontlight_off(&mut self) {
        self.pins
            .split()
            .io1_2
            .into_output()
            .unwrap()
            .set_low()
            .unwrap();
    }

    fn read_power_good(&self) -> u8 {
        let mut i2c = self.i2c.lock().unwrap();
        let mut buffer = [0u8; 1];
        i2c.write_read(DEVICE_ADDRESS, &[0x0F], &mut buffer, BLOCK)
            .unwrap();
        buffer[0]
    }

    //  sets all tps pins as outputs
    fn pins_as_outputs(&self) {
        todo!("pins_as_outputs")
    }
}
