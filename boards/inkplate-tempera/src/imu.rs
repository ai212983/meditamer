use embassy_time::{Instant, Timer};

use super::{
    I2cOps, INT1_LSM, INT2_LSM, IO_INT_ADDR, LSM6DS3_ADDR, LSM6DS3_REG_CTRL1_XL,
    LSM6DS3_REG_CTRL2_G, LSM6DS3_REG_INT_DUR2, LSM6DS3_REG_MD1_CFG, LSM6DS3_REG_OUTX_L_G,
    LSM6DS3_REG_TAP_CFG1, LSM6DS3_REG_TAP_SRC, LSM6DS3_REG_TAP_THS_6D, LSM6DS3_REG_WAKE_UP_THS,
    LSM6DS3_REG_WHO_AM_I, LSM6DS3_WHO_AM_I_VALUE, TPS65186_ADDR,
};

const PCAL_INPUT_PORT1: u8 = 0x01;
const TPS65186_POWER_GOOD: u8 = 0x0F;

#[derive(Clone, Copy, Debug, Default)]
pub struct RawImuSample {
    pub tap_src: u8,
    pub int1: bool,
    pub int2: bool,
    pub gx: i16,
    pub gy: i16,
    pub gz: i16,
    pub ax: i16,
    pub ay: i16,
    pub az: i16,
    pub read_timing: ImuReadTiming,
}

/// End-to-end elapsed time for each I²C request, including bus admission and retry.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImuReadTiming {
    pub interrupt_port_us: u32,
    /// TAP_SRC and the 12 axis bytes under one I²C bus admission.
    pub sensor_us: u32,
}

pub struct InkplateImu<I2C> {
    i2c: I2C,
}

pub enum ImuPairError<E> {
    Tap(E),
    Axes(E),
}

impl<E> ImuPairError<E> {
    fn into_inner(self) -> E {
        match self {
            Self::Tap(error) | Self::Axes(error) => error,
        }
    }
}

/// Two ordinary addressed reads under one bus admission. Each read retains
/// its own STOP, avoiding a read-to-write repeated-start sequence on ESP32.
#[allow(async_fn_in_trait)]
pub trait ImuSensorBus: I2cOps {
    async fn read_sensor_pair(
        &mut self,
        address: u8,
        tap_register: u8,
        tap_source: &mut [u8; 1],
        axes_register: u8,
        axes: &mut [u8; 12],
    ) -> Result<(), ImuPairError<Self::Error>>;
}

impl<I2C> InkplateImu<I2C>
where
    I2C: ImuSensorBus,
{
    pub const fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    pub async fn init(&mut self, sensor_odr_hz: u16) -> Result<bool, I2C::Error> {
        if self
            .read_register(LSM6DS3_ADDR, LSM6DS3_REG_WHO_AM_I)
            .await?
            != LSM6DS3_WHO_AM_I_VALUE
        {
            return Ok(false);
        }

        let odr = odr_register_bits(sensor_odr_hz);
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_CTRL1_XL, odr)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_CTRL2_G, odr)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_TAP_CFG1, 0x0F)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_TAP_THS_6D, 0x09)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_INT_DUR2, 0x76)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_WAKE_UP_THS, 0x80)
            .await?;
        self.write_register(LSM6DS3_ADDR, LSM6DS3_REG_MD1_CFG, 0x48)
            .await?;

        let _ = self.read_register(LSM6DS3_ADDR, LSM6DS3_REG_TAP_SRC).await;
        Ok(true)
    }

    pub async fn read_latest(&mut self) -> Result<RawImuSample, I2C::Error> {
        // Sample the expander port before TAP_SRC. LIR is enabled in `init`, so
        // reading TAP_SRC is what de-asserts INT1; the reverse order always
        // observes a low pin and `int1` is dead.
        let started = Instant::now();
        let interrupt_port = self.read_register(IO_INT_ADDR, PCAL_INPUT_PORT1).await?;
        let interrupt_port_us = elapsed_us(started);
        let mut tap_source = [0u8; 1];
        let mut raw = [0u8; 12];
        let started = Instant::now();
        self.read_sensor_with_retry(&mut tap_source, &mut raw)
            .await?;
        let sensor_us = elapsed_us(started);

        Ok(RawImuSample {
            tap_src: tap_source[0],
            int1: interrupt_port & pin_mask(INT1_LSM) != 0,
            int2: interrupt_port & pin_mask(INT2_LSM) != 0,
            gx: i16::from_le_bytes([raw[0], raw[1]]),
            gy: i16::from_le_bytes([raw[2], raw[3]]),
            gz: i16::from_le_bytes([raw[4], raw[5]]),
            ax: i16::from_le_bytes([raw[6], raw[7]]),
            ay: i16::from_le_bytes([raw[8], raw[9]]),
            az: i16::from_le_bytes([raw[10], raw[11]]),
            read_timing: ImuReadTiming {
                interrupt_port_us,
                sensor_us,
            },
        })
    }

    pub async fn read_power_good(&mut self) -> Result<u8, I2C::Error> {
        self.read_register(TPS65186_ADDR, TPS65186_POWER_GOOD).await
    }

    async fn write_register(
        &mut self,
        addr: u8,
        register: u8,
        value: u8,
    ) -> Result<(), I2C::Error> {
        self.write_with_retry(addr, &[register, value]).await
    }

    async fn read_register(&mut self, addr: u8, register: u8) -> Result<u8, I2C::Error> {
        let mut value = [0u8; 1];
        self.write_read_with_retry(addr, &[register], &mut value)
            .await?;
        Ok(value[0])
    }

    async fn write_with_retry(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        if self.i2c.write(addr, bytes).await.is_ok() {
            return Ok(());
        }
        let _ = self.i2c.reset().await;
        Timer::after_millis(1).await;
        self.i2c.write(addr, bytes).await
    }

    async fn write_read_with_retry(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2C::Error> {
        if self.i2c.write_read(addr, bytes, buffer).await.is_ok() {
            return Ok(());
        }
        let _ = self.i2c.reset().await;
        Timer::after_millis(1).await;
        self.i2c.write_read(addr, bytes, buffer).await
    }

    async fn read_sensor_with_retry(
        &mut self,
        tap_source: &mut [u8; 1],
        axes: &mut [u8; 12],
    ) -> Result<(), I2C::Error> {
        match self.read_sensor_transaction(tap_source, axes).await {
            Ok(()) => Ok(()),
            Err(ImuPairError::Tap(_)) => {
                let _ = self.i2c.reset().await;
                Timer::after_millis(1).await;
                self.read_sensor_transaction(tap_source, axes)
                    .await
                    .map_err(ImuPairError::into_inner)
            }
            Err(ImuPairError::Axes(_)) => {
                // TAP_SRC was already read (and may have cleared INT1). Keep
                // that value and retry only axes, as the original path did.
                let _ = self.i2c.reset().await;
                Timer::after_millis(1).await;
                self.i2c
                    .write_read(LSM6DS3_ADDR, &[LSM6DS3_REG_OUTX_L_G], axes)
                    .await
            }
        }
    }

    async fn read_sensor_transaction(
        &mut self,
        tap_source: &mut [u8; 1],
        axes: &mut [u8; 12],
    ) -> Result<(), ImuPairError<I2C::Error>> {
        // Keep TAP_SRC's interrupt-clear semantics and both STOP boundaries.
        // A contiguous burst would read unrelated intervening registers.
        self.i2c
            .read_sensor_pair(
                LSM6DS3_ADDR,
                LSM6DS3_REG_TAP_SRC,
                tap_source,
                LSM6DS3_REG_OUTX_L_G,
                axes,
            )
            .await
    }
}

fn elapsed_us(started: Instant) -> u32 {
    started.elapsed().as_micros().min(u32::MAX as u64) as u32
}

const fn pin_mask(pin: u8) -> u8 {
    1u8 << (pin & 0x07)
}

const fn odr_register_bits(hz: u16) -> u8 {
    match hz {
        13 => 0x10,
        26 => 0x20,
        52 => 0x30,
        104 => 0x40,
        208 => 0x50,
        416 => 0x60,
        833 => 0x70,
        1660 => 0x80,
        _ => 0x60,
    }
}
