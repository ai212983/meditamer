#![no_std]
#![no_main]

mod app;
mod pirata_clock_font;
mod sd_probe;

use esp_backtrace as _;

#[esp_hal::main]
fn main() -> ! {
    app::run()
}
