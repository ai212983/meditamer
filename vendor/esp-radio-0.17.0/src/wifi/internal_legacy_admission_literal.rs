use alloc::vec::Vec;
use core::mem::{self, MaybeUninit};

use esp_wifi_sys::include::{
    self,
    wifi_active_scan_time_t,
    wifi_scan_channel_bitmap_t,
    wifi_scan_config_t,
    wifi_scan_time_t,
    wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE,
    wifi_scan_type_t_WIFI_SCAN_TYPE_PASSIVE,
};

use crate::binary::include::{wifi_interface_t_WIFI_IF_AP, wifi_interface_t_WIFI_IF_STA};

use super::{
    convert_ap_info, esp_wifi_result, AccessPointInfo, FreeApListOnDrop, ScanConfig,
    ScanTypeConfig, WifiController, WifiError, WifiMode,
};

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    unsafe {
        esp_wifi_result!(include::esp_wifi_start())?;

        let mode = WifiMode::current()?;

        if mode.is_ap() {
            esp_wifi_result!(include::esp_wifi_set_inactive_time(
                wifi_interface_t_WIFI_IF_AP,
                controller.ap_beacon_timeout,
            ))?;
        }
        if mode.is_sta() {
            esp_wifi_result!(include::esp_wifi_set_inactive_time(
                wifi_interface_t_WIFI_IF_STA,
                controller.beacon_timeout,
            ))?;
        }
    }

    Ok(())
}

pub(crate) fn stop(_controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_wifi_result!(unsafe { include::esp_wifi_stop() })
}

pub(crate) fn scan_with_config(
    _controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    let max = config.max.unwrap_or(usize::MAX);
    let use_legacy_scan_n = matches!(
        config,
        ScanConfig {
            ssid: None,
            bssid: None,
            channel: None,
            show_hidden: false,
            scan_type: ScanTypeConfig::Active { .. },
            ..
        }
    );

    if use_legacy_scan_n {
        return scan_n(max);
    }

    scan_with_config_sync_max(config, max)
}

pub(crate) fn scan_n(max: usize) -> Result<Vec<AccessPointInfo>, WifiError> {
    scan_with_config_sync_max(Default::default(), max)
}

pub(crate) fn scan_with_config_sync_max(
    config: ScanConfig<'_>,
    max: usize,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    esp_wifi_result!(wifi_start_scan(true, config))?;
    scan_results(max)
}

fn scan_results(max: usize) -> Result<Vec<AccessPointInfo>, WifiError> {
    let mut scanned = Vec::<AccessPointInfo>::new();
    let mut bss_total: u16 = max as u16;

    let guard = FreeApListOnDrop;

    unsafe { esp_wifi_result!(include::esp_wifi_scan_get_ap_num(&mut bss_total))? };

    guard.defuse();

    let mut record: MaybeUninit<include::wifi_ap_record_t> = MaybeUninit::uninit();
    for _ in 0..usize::min(bss_total as usize, max) {
        let record = unsafe { MaybeUninit::assume_init_mut(&mut record) };
        unsafe { esp_wifi_result!(include::esp_wifi_scan_get_ap_record(record))? };
        scanned.push(convert_ap_info(record));
    }

    unsafe { esp_wifi_result!(include::esp_wifi_clear_ap_list())? };

    Ok(scanned)
}

pub(crate) fn wifi_start_scan(
    block: bool,
    ScanConfig {
        ssid,
        mut bssid,
        channel,
        show_hidden,
        scan_type,
        ..
    }: ScanConfig<'_>,
) -> i32 {
    scan_type.validate();
    let (scan_time, scan_type) = match scan_type {
        ScanTypeConfig::Active { min, max } => (
            wifi_scan_time_t {
                active: wifi_active_scan_time_t {
                    min: min.as_millis() as u32,
                    max: max.as_millis() as u32,
                },
                passive: 0,
            },
            wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE,
        ),
        ScanTypeConfig::Passive(dur) => (
            wifi_scan_time_t {
                active: wifi_active_scan_time_t { min: 0, max: 0 },
                passive: dur.as_millis() as u32,
            },
            wifi_scan_type_t_WIFI_SCAN_TYPE_PASSIVE,
        ),
    };

    let mut ssid_buf = ssid.map(|value| {
        let mut buf = alloc::vec::Vec::from_iter(value.bytes());
        buf.push(b'\0');
        buf
    });

    let ssid = ssid_buf
        .as_mut()
        .map(|buf| buf.as_mut_ptr())
        .unwrap_or_else(core::ptr::null_mut);
    let bssid = bssid
        .as_mut()
        .map(|buf| buf.as_mut_ptr())
        .unwrap_or_else(core::ptr::null_mut);

    let mut scan_config: wifi_scan_config_t = unsafe { mem::zeroed() };
    scan_config.ssid = ssid;
    scan_config.bssid = bssid;
    scan_config.channel = channel.unwrap_or(0);
    scan_config.show_hidden = show_hidden;
    scan_config.scan_type = scan_type;
    scan_config.scan_time = scan_time;
    scan_config.home_chan_dwell_time = 0;
    scan_config.channel_bitmap = wifi_scan_channel_bitmap_t {
        ghz_2_channels: 0,
        ghz_5_channels: 0,
    };

    unsafe { include::esp_wifi_scan_start(&scan_config, block) }
}
