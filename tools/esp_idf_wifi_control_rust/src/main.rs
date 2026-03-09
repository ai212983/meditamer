use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::info;

fn cc_char(byte: u8) -> char {
    if byte.is_ascii_graphic() { byte as char } else { '.' }
}

fn log_driver_state(label: &str) {
    unsafe {
        let mut mode = 0u32;
        let mut primary = 0u8;
        let mut second = 0u32;
        let mut ps = 0u32;
        let mut max_tx_power = 0i8;
        let mut event_mask = 0u32;
        let mut protocol_bitmap = 0u8;
        let mut scan_defaults = core::mem::zeroed::<esp_idf_svc::sys::wifi_scan_default_params_t>();
        let mut country = core::mem::zeroed::<esp_idf_svc::sys::wifi_country_t>();
        let mode_rc = esp_idf_svc::sys::esp_wifi_get_mode(&mut mode as *mut u32);
        let channel_rc = esp_idf_svc::sys::esp_wifi_get_channel(&mut primary as *mut u8, &mut second as *mut u32);
        let ps_rc = esp_idf_svc::sys::esp_wifi_get_ps(&mut ps as *mut u32);
        let max_tx_power_rc = esp_idf_svc::sys::esp_wifi_get_max_tx_power(&mut max_tx_power as *mut i8);
        let event_mask_rc = esp_idf_svc::sys::esp_wifi_get_event_mask(&mut event_mask as *mut u32);
        let protocol_rc = esp_idf_svc::sys::esp_wifi_get_protocol(esp_idf_svc::sys::wifi_interface_t_WIFI_IF_STA, &mut protocol_bitmap as *mut u8);
        let scan_defaults_rc = esp_idf_svc::sys::esp_wifi_get_scan_parameters(&mut scan_defaults as *mut _);
        let country_rc = esp_idf_svc::sys::esp_wifi_get_country(&mut country as *mut _);
        info!(
            "{label} mode_rc={mode_rc} mode={mode} channel_rc={channel_rc} primary={primary} second={second} ps_rc={ps_rc} ps={ps} max_tx_power_rc={max_tx_power_rc} max_tx_power={max_tx_power} event_mask_rc={event_mask_rc} event_mask=0x{event_mask:08x} protocol_rc={protocol_rc} protocol_bitmap=0x{protocol_bitmap:02x} country_rc={country_rc} cc={}{}{} schan={} nchan={} country_max_tx_power={} policy={} scan_defaults_rc={} scan_active_min={} scan_active_max={} scan_passive={} scan_home_dwell={}",
            cc_char(country.cc[0]),
            cc_char(country.cc[1]),
            cc_char(country.cc[2]),
            country.schan,
            country.nchan,
            country.max_tx_power,
            country.policy,
            scan_defaults_rc,
            scan_defaults.scan_time.active.min,
            scan_defaults.scan_time.active.max,
            scan_defaults.scan_time.passive,
            scan_defaults.home_chan_dwell_time
        );
    }
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    wifi.start()?;
    info!("mode=scan_only started=true");
    log_driver_state("pre_scan_driver_state");

    let aps = wifi.scan()?;
    info!("scan_complete total_ap_count={}", aps.len());
    for (idx, ap) in aps.iter().enumerate() {
        info!(
            "scan_ap idx={} ssid={} rssi={} channel={} auth={:?} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            idx,
            ap.ssid,
            ap.signal_strength,
            ap.channel,
            ap.auth_method,
            ap.bssid[0],
            ap.bssid[1],
            ap.bssid[2],
            ap.bssid[3],
            ap.bssid[4],
            ap.bssid[5],
        );
    }

    wifi.stop()?;
    info!("mode=scan_only stopped=true");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
