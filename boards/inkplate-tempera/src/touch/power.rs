use super::*;
use crate::expander::{PCAL_CFGPORT0_ARRAY, PCAL_OUTPORT0_ARRAY, PCAL_REG_ADDRS};

impl<I2C> InkplateTouch<I2C>
where
    I2C: I2cOps,
{
    async fn initialize_external_expander(&mut self) -> Result<(), I2C::Error> {
        if self.external_ready {
            return Ok(());
        }
        let mut regs = [0u8; 23];
        self.i2c
            .write_read(IO_EXT_ADDR, &[0x00], &mut regs)
            .await
            .map_err(InkplateHalError::I2c)?;
        self.io_regs_ext = regs;
        self.external_ready = true;
        Ok(())
    }

    async fn set_output(&mut self, pin: u8, high: bool) -> Result<(), I2C::Error> {
        self.initialize_external_expander().await?;
        let bit = 1u8 << pin;
        self.io_regs_ext[PCAL_CFGPORT0_ARRAY] &= !bit;
        if high {
            self.io_regs_ext[PCAL_OUTPORT0_ARRAY] |= bit;
        } else {
            self.io_regs_ext[PCAL_OUTPORT0_ARRAY] &= !bit;
        }
        // Failed or cancelled writes may already have reached the expander.
        // Re-read hardware before the next dependent power/reset transition.
        self.external_ready = false;
        self.i2c_write(
            IO_EXT_ADDR,
            &[
                PCAL_REG_ADDRS[PCAL_OUTPORT0_ARRAY],
                self.io_regs_ext[PCAL_OUTPORT0_ARRAY],
            ],
        )
        .await?;
        self.i2c_write(
            IO_EXT_ADDR,
            &[
                PCAL_REG_ADDRS[PCAL_CFGPORT0_ARRAY],
                self.io_regs_ext[PCAL_CFGPORT0_ARRAY],
            ],
        )
        .await?;
        self.external_ready = true;
        Ok(())
    }

    pub async fn set_power_enabled(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        // Touchscreen power-enable is active-low on Inkplate 4 TEMPERA.
        self.set_output(TOUCHSCREEN_EN, !enabled).await
    }

    #[cfg(target_os = "none")]
    pub(super) async fn hardware_reset(&mut self) -> Result<(), I2C::Error> {
        self.set_output(TOUCHSCREEN_RST, false).await?;
        embassy_time::Timer::after_millis(30).await;
        self.set_output(TOUCHSCREEN_RST, true).await?;
        embassy_time::Timer::after_millis(30).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::{
        future::{pending, Future},
        task::{Context, Poll, Waker},
    };
    use std::{cell::RefCell, rc::Rc};

    struct Bus {
        state: Rc<RefCell<State>>,
    }
    struct State {
        regs: [u8; 23],
        reads: usize,
        interrupt: bool,
        fail: bool,
    }
    impl I2cOps for Bus {
        type Error = ();
        async fn probe(&mut self, _: u8) -> core::result::Result<bool, ()> {
            Ok(true)
        }
        async fn reset(&mut self) -> core::result::Result<(), ()> {
            Ok(())
        }
        async fn read(&mut self, _: u8, _: &mut [u8]) -> core::result::Result<(), ()> {
            unreachable!()
        }
        async fn write_read(
            &mut self,
            _: u8,
            _: &[u8],
            data: &mut [u8],
        ) -> core::result::Result<(), ()> {
            let mut state = self.state.borrow_mut();
            data.copy_from_slice(&state.regs);
            state.reads += 1;
            Ok(())
        }
        async fn write(&mut self, _: u8, data: &[u8]) -> core::result::Result<(), ()> {
            let interrupt = {
                let mut state = self.state.borrow_mut();
                let index = PCAL_REG_ADDRS
                    .iter()
                    .position(|reg| *reg == data[0])
                    .unwrap();
                state.regs[index] = data[1];
                state.interrupt
            };
            if interrupt {
                if self.state.borrow().fail {
                    return Err(());
                }
                pending::<()>().await;
            }
            Ok(())
        }
    }

    #[test]
    fn interrupted_power_write_reconciles_hardware_before_next_transition() {
        let _clock = crate::TEST_CLOCK.lock().unwrap();
        for fail in [false, true] {
            let state = Rc::new(RefCell::new(State {
                regs: [0xff; 23],
                reads: 0,
                interrupt: true,
                fail,
            }));
            let mut touch = InkplateTouch::new(Bus {
                state: state.clone(),
            });
            {
                let mut transition = core::pin::pin!(touch.set_power_enabled(true));
                // Either the write stalls after taking effect, or retry waits
                // after an error. Dropping either future must invalidate cache.
                assert!(transition
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending());
            }
            assert!(!touch.external_ready);
            {
                let mut state = state.borrow_mut();
                state.interrupt = false;
                // A hardware change to an unrelated output must survive retry.
                state.regs[PCAL_OUTPORT0_ARRAY] &= !0x10;
            }
            let mut transition = core::pin::pin!(touch.set_power_enabled(false));
            assert!(matches!(
                transition
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Ok(()))
            ));
            assert_eq!(state.borrow().reads, 2);
            assert_eq!(state.borrow().regs[PCAL_OUTPORT0_ARRAY] & 0x10, 0);
        }
    }
}
