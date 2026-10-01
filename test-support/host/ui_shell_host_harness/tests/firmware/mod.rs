//! Host stand-in for the strict-PSRAM buffer: owns its bytes and optionally
//! counts drops so tests can observe buffer frees. Exists only so the narrow
//! request/completion channels and ownership flow compile and run on host;
//! it proves channel aliasing and drop/ownership behavior, not device PSRAM
//! allocation.

pub mod psram {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    /// Owned host buffer for the intake channels.
    #[derive(Debug)]
    pub struct LargeByteBuffer {
        buf: Vec<u8>,
        drops: Option<Arc<AtomicUsize>>,
    }

    impl LargeByteBuffer {
        pub fn from_vec(buf: Vec<u8>) -> Self {
            Self { buf, drops: None }
        }

        pub fn counting(buf: Vec<u8>, drops: &Arc<AtomicUsize>) -> Self {
            Self {
                buf,
                drops: Some(Arc::clone(drops)),
            }
        }

        pub fn as_slice(&self) -> &[u8] {
            &self.buf
        }
    }

    impl Drop for LargeByteBuffer {
        fn drop(&mut self) {
            if let Some(counter) = &self.drops {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
} // mod psram

pub mod storage;
