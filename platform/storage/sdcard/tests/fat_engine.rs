#![cfg(feature = "host-tests")]

// Integration-test crate root. `mod` children of a `tests/*.rs` root resolve
// next to the root (`tests/`), so each module carries an explicit `#[path]`
// pointing at the `fat_engine/` directory tree. No `include!`, no part-NN.
#[path = "fat_engine/core.rs"]
mod core;
#[path = "fat_engine/mutation.rs"]
mod mutation;
#[path = "fat_engine/range.rs"]
mod range;
#[path = "fat_engine/read.rs"]
mod read;
#[path = "fat_engine/support.rs"]
mod support;
#[path = "fat_engine/upload.rs"]
mod upload;
