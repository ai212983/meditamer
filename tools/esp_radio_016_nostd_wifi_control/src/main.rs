#![no_std]
#![no_main]

mod promisc_diag;

use esp_backtrace as _;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_radio::wifi::{ScanConfig, WifiMode};

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    println!("radio016_nostd_wifi_control: begin=true");

    let radio = match esp_radio::init() {
        Ok(ctrl) => ctrl,
        Err(err) => panic!("radio016_nostd_wifi_control: esp_radio::init err={:?}", err),
    };
    println!("radio016_nostd_wifi_control: esp_radio_init=ok");

    let (mut controller, mut ifaces) =
        match esp_radio::wifi::new(&radio, peripherals.WIFI, Default::default()) {
            Ok(parts) => parts,
            Err(err) => panic!("radio016_nostd_wifi_control: wifi_new err={:?}", err),
        };
    println!("radio016_nostd_wifi_control: wifi_new=ok");

    if let Err(err) = controller.set_mode(WifiMode::Sta) {
        panic!("radio016_nostd_wifi_control: set_mode err={:?}", err);
    }
    println!("radio016_nostd_wifi_control: set_mode=sta");

    if let Err(err) = controller.start() {
        panic!("radio016_nostd_wifi_control: start err={:?}", err);
    }
    println!("radio016_nostd_wifi_control: start=ok");

    promisc_diag::run(&mut ifaces.sniffer);

    match controller.scan_with_config(ScanConfig::default().with_max(16)) {
        Ok(results) => {
            println!("radio016_nostd_wifi_control: scan=ok count={}", results.len());
            for (idx, ap) in results.iter().take(10).enumerate() {
                println!(
                    "radio016_nostd_wifi_control: ap idx={} ssid={} channel={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} rssi={} auth={:?}",
                    idx,
                    ap.ssid,
                    ap.channel,
                    ap.bssid[0],
                    ap.bssid[1],
                    ap.bssid[2],
                    ap.bssid[3],
                    ap.bssid[4],
                    ap.bssid[5],
                    ap.signal_strength,
                    ap.auth_method,
                );
            }
        }
        Err(err) => {
            println!("radio016_nostd_wifi_control: scan=err err={:?}", err);
        }
    }

    match controller.stop() {
        Ok(()) => println!("radio016_nostd_wifi_control: stop=ok"),
        Err(err) => println!("radio016_nostd_wifi_control: stop=err err={:?}", err),
    }

    loop {
        core::hint::spin_loop();
    }
}
