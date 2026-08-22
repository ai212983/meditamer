#![no_std]
#![allow(dead_code)]

pub mod firmware;

// ADR-0015: the shell is its own crate; re-export it under the path the
// harness tests already use.
pub use shell;
