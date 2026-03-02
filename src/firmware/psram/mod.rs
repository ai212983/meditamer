#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
#[cfg(feature = "psram-alloc")]
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    slice,
};

include!("core.rs");
include!("buffer.rs");
include!("init.rs");
include!("status.rs");
