use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_svc::hal::i2c::I2cDriver;
use port_expander::dev::pcal6416a::Driver;
use std::sync::Mutex;

pub type I2cBus<'a> = MutexDevice<'a, I2cDriver<'a>>;
pub type PortMutexInkplate<'a> = Mutex<Driver<I2cBus<'a>>>;
