//! Standalone BQ27441 state-of-charge read, usable without a full
//! `InkplateHal` instance.
//!
//! Typed observation subscriptions plan, Phase 5: the battery provider task
//! owns its own I2C device handle and reads the fuel gauge directly through
//! it, rather than through `InkplateHal`. The one step it still depends on
//! `InkplateHal` for is waking the part first
//! (`InkplateHal::wake_fuel_gauge`, in `control::sensors`) -- that toggles a
//! PCAL6416A expander pin shared with panel/frontlight control, which stays
//! exactly where it was rather than growing a second, racing owner; see the
//! plan's Phase 5 evidence for how the caller bridges the two rather than
//! this crate.
//!
//! Generic over `embedded_hal_async::i2c::I2c`, like `environment`, so it
//! stays host-testable.

use embedded_hal_async::i2c::I2c;

/// 7-bit I2C address. Fixed on this part, matching `hardware::FUEL_GAUGE_ADDR`.
pub const FUEL_GAUGE_ADDR: u8 = 0x55;
const BQ27441_COMMAND_SOC: u8 = 0x1C;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuelGaugeError<E>(pub E);

impl<E> From<E> for FuelGaugeError<E> {
    fn from(value: E) -> Self {
        Self(value)
    }
}

/// Reads the state-of-charge register (percent, 0-100 on a healthy part;
/// the caller decides what an out-of-range value means).
pub async fn read_state_of_charge<I2C: I2c>(
    i2c: &mut I2C,
) -> Result<u16, FuelGaugeError<I2C::Error>> {
    let mut buf = [0u8; 2];
    i2c.write_read(FUEL_GAUGE_ADDR, &[BQ27441_COMMAND_SOC], &mut buf)
        .await?;
    Ok(((buf[1] as u16) << 8) | (buf[0] as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;

    struct FakeI2c {
        response: [u8; 2],
        last_write: Option<u8>,
    }

    impl embedded_hal_async::i2c::ErrorType for FakeI2c {
        type Error = Infallible;
    }

    impl I2c for FakeI2c {
        async fn transaction(
            &mut self,
            _address: u8,
            operations: &mut [embedded_hal_async::i2c::Operation<'_>],
        ) -> Result<(), Self::Error> {
            for op in operations {
                match op {
                    embedded_hal_async::i2c::Operation::Write(bytes) => {
                        self.last_write = bytes.first().copied();
                    }
                    embedded_hal_async::i2c::Operation::Read(buffer) => {
                        buffer.copy_from_slice(&self.response);
                    }
                }
            }
            Ok(())
        }
    }

    /// No-op-waker poll loop: `FakeI2c` never actually pends, so no executor
    /// dependency is needed (`platform/time/wall-clock`'s `tests/support` pattern).
    fn block_on<F: core::future::Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        loop {
            if let core::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    #[test]
    fn decodes_little_endian_and_addresses_the_soc_register() {
        let mut i2c = FakeI2c {
            response: [84, 0], // 84 percent, high byte zero
            last_write: None,
        };
        let soc = block_on(read_state_of_charge(&mut i2c)).unwrap();
        assert_eq!(soc, 84);
        assert_eq!(i2c.last_write, Some(BQ27441_COMMAND_SOC));
    }
}
