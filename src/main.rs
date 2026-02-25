#![no_std]
#![no_main]

use esp_backtrace as _;

#[esp_hal::main]
fn main() -> ! {
    meditamer::firmware::run()
}
