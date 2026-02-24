#![no_std]
#![no_main]

mod app;
mod pirata_clock_font;

use esp_backtrace as _;

#[esp_hal::main]
fn main() -> ! {
    app::run()
}
