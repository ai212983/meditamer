//! Fake I2C bus for host-testing [`rtc::driver::Pcf85063a`] without hardware.
//!
//! Models the PCF85063A's auto-incrementing register pointer: the first byte
//! of a `Write` operation sets the pointer, subsequent bytes in that `Write`
//! land in consecutive registers, and a following `Read` continues from
//! wherever the pointer was left — exactly the `write_read` pattern
//! `Pcf85063a` uses for every register access.

use embedded_hal::i2c::{Error as I2cErrorTrait, ErrorKind, ErrorType, Operation};
use embedded_hal_async::i2c::I2c;
use rtc::registers;

/// Polls a future to completion. `FakeI2c`'s `transaction` never actually
/// pends (it resolves on first poll), so a trivial no-op-waker poll loop is
/// all host tests need — no executor dependency required.
pub fn block_on<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    loop {
        if let core::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeI2cError;

impl I2cErrorTrait for FakeI2cError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// One recorded transaction: the register the pointer started at, and
/// whether it was a write (with its written bytes) or a read (with its byte
/// count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedTransaction {
    Write { start: u8, bytes: std::vec::Vec<u8> },
    Read { start: u8, len: usize },
}

pub struct FakeI2c {
    registers: [u8; 256],
    pub trace: std::vec::Vec<RecordedTransaction>,
    /// 0-based transaction index (in call order) that should fail with
    /// [`FakeI2cError`] instead of executing. `None` means never fail.
    pub fail_at_transaction: Option<usize>,
    next_transaction_index: usize,
    /// Overrides the bytes returned by the `Read` operation at this 0-based
    /// index (counting only `Read` operations, in order), instead of the
    /// bus's real register contents. Consumed after use. Lets a test make a
    /// specific readback (e.g. `TIMESET`'s post-write verify read) disagree
    /// with what was actually written, without disturbing every other read.
    pub read_override: Option<(usize, std::vec::Vec<u8>)>,
    next_read_index: usize,
}

impl FakeI2c {
    pub fn new() -> Self {
        Self {
            registers: [0u8; 256],
            trace: std::vec::Vec::new(),
            fail_at_transaction: None,
            next_transaction_index: 0,
            read_override: None,
            next_read_index: 0,
        }
    }

    pub fn register(&self, address: u8) -> u8 {
        self.registers[address as usize]
    }

    pub fn set_register(&mut self, address: u8, value: u8) {
        self.registers[address as usize] = value;
    }

    pub fn set_block(&mut self, start: u8, bytes: &[u8]) {
        for (index, byte) in bytes.iter().enumerate() {
            self.registers[start as usize + index] = *byte;
        }
    }
}

impl Default for FakeI2c {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorType for FakeI2c {
    type Error = FakeI2cError;
}

impl I2c for FakeI2c {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), FakeI2cError> {
        if address != registers::I2C_ADDRESS {
            return Err(FakeI2cError);
        }

        let transaction_index = self.next_transaction_index;
        self.next_transaction_index += 1;
        if self.fail_at_transaction == Some(transaction_index) {
            return Err(FakeI2cError);
        }

        let mut pointer: u8 = 0;
        for operation in operations.iter_mut() {
            match operation {
                Operation::Write(bytes) => {
                    let Some((&reg, data)) = bytes.split_first() else {
                        continue;
                    };
                    self.trace.push(RecordedTransaction::Write {
                        start: reg,
                        bytes: data.to_vec(),
                    });
                    let mut addr = reg;
                    for &byte in data {
                        self.registers[addr as usize] = byte;
                        addr = addr.wrapping_add(1);
                    }
                    pointer = reg.wrapping_add(data.len() as u8);
                }
                Operation::Read(buf) => {
                    self.trace.push(RecordedTransaction::Read {
                        start: pointer,
                        len: buf.len(),
                    });
                    let read_index = self.next_read_index;
                    self.next_read_index += 1;
                    if self
                        .read_override
                        .as_ref()
                        .is_some_and(|(idx, _)| *idx == read_index)
                    {
                        let (_, bytes) = self.read_override.take().expect("checked above");
                        assert_eq!(
                            bytes.len(),
                            buf.len(),
                            "read_override byte count must match the overridden read's length"
                        );
                        buf.copy_from_slice(&bytes);
                        // Keep the register pointer consistent with a real
                        // device even though the returned bytes are faked.
                        pointer = pointer.wrapping_add(buf.len() as u8);
                        continue;
                    }
                    let mut addr = pointer;
                    for slot in buf.iter_mut() {
                        *slot = self.registers[addr as usize];
                        addr = addr.wrapping_add(1);
                    }
                    pointer = addr;
                }
            }
        }
        Ok(())
    }
}
