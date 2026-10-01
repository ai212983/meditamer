use super::*;
use embedded_hal::i2c::ErrorKind;
use std::convert::Infallible;
use std::{cell::RefCell, rc::Rc, vec::Vec};

extern crate std;

type Trace = Rc<RefCell<Vec<(u8, usize, u8, usize, ErrorKind)>>>;

struct Bus {
    calls: Rc<RefCell<usize>>,
    fail: bool,
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        *self.calls.borrow_mut() += 1;
        assert_eq!(address, 0x76);
        assert_eq!(operations.len(), 2);
        assert!(matches!(&operations[0], Operation::Write(bytes) if *bytes == [0xd0]));
        match &mut operations[1] {
            Operation::Read(bytes) => bytes.copy_from_slice(&[0x61]),
            Operation::Write(_) => panic!("read must remain in the same transaction"),
        }
        if self.fail {
            Err(ErrorKind::Other)
        } else {
            Ok(())
        }
    }
}

impl embedded_hal_async::i2c::I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        let mut pending = true;
        core::future::poll_fn(|cx| {
            if core::mem::take(&mut pending) {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            } else {
                core::task::Poll::Ready(())
            }
        })
        .await;
        I2c::transaction(self, address, operations)
    }
}

struct Sink(Trace);
impl Diagnostics<ErrorKind> for Sink {
    fn failed(&mut self, address: u8, operations: &[Operation<'_>], error: &ErrorKind) {
        let Operation::Write(written) = &operations[0] else {
            panic!()
        };
        let Operation::Read(read) = &operations[1] else {
            panic!()
        };
        self.0
            .borrow_mut()
            .push((address, operations.len(), written[0], read.len(), *error));
    }
}

struct ConfigBus(Rc<RefCell<Vec<u32>>>);

impl SetConfig for ConfigBus {
    type Config = u32;
    type ConfigError = Infallible;

    fn set_config(&mut self, config: &Self::Config) -> Result<(), Self::ConfigError> {
        self.0.borrow_mut().push(*config);
        Ok(())
    }
}

#[test]
fn configuration_is_forwarded_to_the_wrapped_bus() {
    let configs = Rc::new(RefCell::new(Vec::new()));
    let mut bus = StartupI2c::new(ConfigBus(configs.clone()), ());

    SetConfig::set_config(&mut bus, &400).unwrap();

    assert_eq!(*configs.borrow(), [400]);
}

#[test]
fn failed_transaction_is_reported_once_without_retry_and_runtime_tracing_stops() {
    let calls = Rc::new(RefCell::new(0));
    let trace = Trace::default();
    let mut bus = StartupI2c::new(
        Bus {
            calls: calls.clone(),
            fail: true,
        },
        Sink(trace.clone()),
    );
    let mut read = [0];
    assert_eq!(
        bus.write_read(0x76, &[0xd0], &mut read),
        Err(ErrorKind::Other)
    );
    assert_eq!(*calls.borrow(), 1);
    assert_eq!(*trace.borrow(), [(0x76, 2, 0xd0, 1, ErrorKind::Other)]);
    bus.finish_startup();
    assert_eq!(
        bus.write_read(0x76, &[0xd0], &mut read),
        Err(ErrorKind::Other)
    );
    assert_eq!(*calls.borrow(), 2);
    assert_eq!(trace.borrow().len(), 1);
    bus.inner.fail = false;
    assert_eq!(bus.write_read(0x76, &[0xd0], &mut read), Ok(()));
    assert_eq!(read, [0x61]);
    assert_eq!(trace.borrow().len(), 1);
}

#[test]
fn successful_startup_transaction_preserves_data_without_diagnostics() {
    let calls = Rc::new(RefCell::new(0));
    let trace = Trace::default();
    let mut bus = StartupI2c::new(
        Bus {
            calls: calls.clone(),
            fail: false,
        },
        Sink(trace.clone()),
    );
    let mut read = [0];
    assert_eq!(bus.write_read(0x76, &[0xd0], &mut read), Ok(()));
    assert_eq!(read, [0x61]);
    assert_eq!(*calls.borrow(), 1);
    assert!(trace.borrow().is_empty());
}

#[test]
fn async_adapter_yields_and_preserves_error_and_success() {
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    for fail in [false, true] {
        let calls = Rc::new(RefCell::new(0));
        let trace = Trace::default();
        let mut bus = StartupI2c::new(
            Bus {
                calls: calls.clone(),
                fail,
            },
            Sink(trace.clone()),
        );
        let mut read = [0];
        let result = {
            let mut future = pin!(embedded_hal_async::i2c::I2c::write_read(
                &mut bus,
                0x76,
                &[0xd0],
                &mut read
            ));
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(future.as_mut().poll(&mut cx), Poll::Pending);
            assert_eq!(*calls.borrow(), 0);
            assert!(trace.borrow().is_empty());
            future.as_mut().poll(&mut cx)
        };
        assert_eq!(
            result,
            Poll::Ready(if fail { Err(ErrorKind::Other) } else { Ok(()) })
        );
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(trace.borrow().len(), usize::from(fail));
        assert_eq!(read, [0x61]);
    }
}
