#![no_std]
#![allow(dead_code)]

#[path = "../../../src/firmware/ui/shell/mod.rs"]
pub mod shell;

#[cfg(test)]
mod lvgl_overlay_semantics;
