//! Host regression tests for the supervisor SD Abort fence. See
//! `fence_harness/firmware.rs` for the mirrored tree and
//! `fence_harness/fence_tests.rs` for the tests.

extern crate self as console;

#[macro_export]
macro_rules! println {
    ($($argument:tt)*) => {{
        let _ = ::core::format_args!($($argument)*);
    }};
}

#[path = "fence_harness/firmware.rs"]
pub mod firmware;
