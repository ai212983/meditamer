#![no_std]
#![no_main]

mod firmware;

use esp_backtrace as _;

#[esp_hal::main]
fn main() -> ! {
    firmware::run()
}
