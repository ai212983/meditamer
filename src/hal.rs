use crate::types::PortMutexInkplate;
use esp_idf_svc::hal::{delay::Delay, i2c, i2c::I2cDriver, io::Write};
use port_expander::Pcal6416a;

pub struct Frontlight<'a, I2C> {
    i2c: I2C,
    pins_internal: Pcal6416a<PortMutexInkplate<'a>>,
}
const BRIGHTNESS_ADDRESS: u8 = 0x2E; // 0x5C >> 1

impl<'a, I2C> Frontlight<'a, I2C>
where
    I2C: embedded_hal::i2c::I2c,
{
    pub fn new(i2c: I2C, pins_internal: Pcal6416a<PortMutexInkplate<'a>>) -> Self {
        Frontlight { i2c, pins_internal }
    }

    pub fn set_brightness(&mut self, brightness: u8) {
        self.on();
        self.i2c
            .write(BRIGHTNESS_ADDRESS, &[0x00, 63 - (brightness & 0b00111111)])
            .unwrap();
    }

    // #define FRONTLIGHT_EN 10
    pub fn on(&mut self) {
        self.pins_internal
            .split()
            .io1_2
            .into_output()
            .unwrap()
            .set_high()
            .unwrap();
    }

    pub fn off(&mut self) {
        self.pins_internal
            .split()
            .io1_2
            .into_output()
            .unwrap()
            .set_low()
            .unwrap();
    }
}

const DEVICE_ADDRESS: u8 = 0x48;

pub struct Power<'a, I2C> {
    i2c: I2C,
    pins_internal: Pcal6416a<PortMutexInkplate<'a>>,
}
impl<'a, I2C> Power<'a, I2C>
where
    I2C: embedded_hal::i2c::I2c,
{
    pub fn new(i2c: I2C, pins_internal: Pcal6416a<PortMutexInkplate<'a>>) -> Self {
        Power { i2c, pins_internal }
    }

    pub fn init(&mut self) {
        {
            let pins = self.pins_internal.split();
            pins.io0_5.into_output().unwrap(); // VCOM 5 // GPIOA6
            pins.io0_4.into_output().unwrap(); // PWRUP 4 // GPIOA4
            let mut wakeup = pins.io0_3.into_output().unwrap(); // WAKEUP 3 // GPIOA3
            let mut io1_0 = pins.io1_0.into_output().unwrap(); //  GPIO0_ENABLE  8
            io1_0.set_high().unwrap();

            // Board initialized, sending power up sequence
            wakeup.set_high().unwrap();
        }
        let delay: Delay = Default::default();
        delay.delay_ms(1);
        self.i2c.write(
            DEVICE_ADDRESS,
            &[0x09, 0b00011011, 0b00000000, 0b00011011, 0b00000000],
        );

        delay.delay_ms(1);
        self.pins_internal
            .split()
            .io0_3
            .into_output()
            .unwrap()
            .set_low()
            .unwrap();
    }

    pub fn eink_on(&mut self) {
        self.pins_internal
            .split()
            .io0_3
            .into_output()
            .unwrap()
            .set_high()
            .unwrap(); // WAKEUP 3 // GPIOA3
        let delay: Delay = Default::default();
        delay.delay_ms(5);
        // Modify power up sequence  (VEE and VNEG are swapped)
        self.i2c.write(DEVICE_ADDRESS, &[0x09, 0b11100001]).unwrap();
        // Enable all rails
        self.i2c.write(DEVICE_ADDRESS, &[0x01, 0b00111111]).unwrap();

        self.pins_internal
            .split()
            .io0_4
            .into_output()
            .unwrap()
            .set_high()
            .unwrap(); // PWRUP 4 // GPIOA4
    }

    fn read_power_good(&mut self) -> u8 {
        let mut buffer = [0u8; 1];
        self.i2c
            .write_read(DEVICE_ADDRESS, &[0x0F], &mut buffer)
            .unwrap();
        buffer[0]
    }
}
