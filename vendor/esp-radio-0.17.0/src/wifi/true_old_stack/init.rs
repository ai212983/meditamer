use alloc::vec::Vec;
use core::{ptr::addr_of, sync::atomic::Ordering, task::Context};

#[cfg(all(feature = "sniffer", feature = "unstable"))]
use crate::wifi::PromiscuousPkt;

use super::{control, install, rx, super::{
         g_wifi_default_wpa_crypto_funcs,
    internal_legacy_admission_literal,
    internal_legacy_backend,
    internal_legacy_coex_backend,
    esp_interface_t_ESP_IF_WIFI_AP,
    esp_interface_t_ESP_IF_WIFI_STA,
    esp_supplicant_init,
    esp_wifi_init_internal,
    esp_wifi_internal_reg_rxcb,
    esp_wifi_set_country,
    esp_wifi_set_mode,
    esp_wifi_set_tx_done_cb,
    esp_wifi_tx_done_cb,
    wifi_mode_t_WIFI_MODE_NULL,
    AccessPointInfo,
    Config,
    Interfaces,
    ScanConfig,
    WifiController,
    WifiDevice,
    WifiDeviceMode,
    WifiError,
    WifiRxToken,
    WifiTxToken,
    RX_QUEUE_SIZE,
    TX_QUEUE_SIZE,
}};
use crate::esp_wifi_result;

pub(crate) fn enabled() -> bool {
    rx::enabled()
}

fn validate_config(config: Config) -> Result<(), WifiError> {
    if crate::is_interrupts_disabled() {
        return Err(WifiError::Unsupported);
    }

    config.validate();
    Ok(())
}

unsafe fn wifi_init(
    _wifi: crate::hal::peripherals::WIFI<'_>,
    config: Config,
) -> Result<(), WifiError> {
    install::install_legacy_literal_g_config(config, g_wifi_default_wpa_crypto_funcs);
    RX_QUEUE_SIZE.store(config.rx_queue_size, Ordering::Relaxed);
    TX_QUEUE_SIZE.store(config.tx_queue_size, Ordering::Relaxed);

    #[cfg(coex)]
    {
        esp_println::println!("esp_radio: legacy_port_wifi_init stage=coex_init.before");
        esp_wifi_result!(internal_legacy_coex_backend::coex_init())?;
        esp_println::println!("esp_radio: legacy_port_wifi_init stage=coex_init.after");
    }

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_init_internal.before");
    esp_wifi_result!(esp_wifi_init_internal(addr_of!(install::G_CONFIG)))?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_init_internal.after");

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_set_mode_null.before");
    esp_wifi_result!(esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_NULL))?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_set_mode_null.after");

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_supplicant_init.before");
    esp_wifi_result!(esp_supplicant_init())?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_supplicant_init.after");

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_set_tx_done_cb.before");
    esp_wifi_result!(esp_wifi_set_tx_done_cb(Some(esp_wifi_tx_done_cb)))?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=esp_wifi_set_tx_done_cb.after");

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=reg_rxcb_sta.before");
    esp_wifi_result!(esp_wifi_internal_reg_rxcb(
        esp_interface_t_ESP_IF_WIFI_STA,
        Some(if internal_legacy_backend::enabled() {
            rx::recv_cb_sta
        } else {
            recv_cb_sta
        }),
    ))?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=reg_rxcb_sta.after");

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=reg_rxcb_ap.before");
    esp_wifi_result!(esp_wifi_internal_reg_rxcb(
        esp_interface_t_ESP_IF_WIFI_AP,
        Some(if internal_legacy_backend::enabled() {
            rx::recv_cb_ap
        } else {
            recv_cb_ap
        }),
    ))?;
    esp_println::println!("esp_radio: legacy_port_wifi_init stage=reg_rxcb_ap.after");

    #[cfg(any(esp32, esp32s3))]
    {
        static mut NVS_STRUCT: [u32; 12] = [0; 12];
        crate::common_adapter::__ESP_RADIO_G_MISC_NVS = core::ptr::addr_of_mut!(NVS_STRUCT)
            .cast::<u32>();
    }

    esp_println::println!("esp_radio: legacy_port_wifi_init stage=done");
    Ok(())
}

fn set_country(config: Config) -> Result<(), WifiError> {
    unsafe {
        let country = config.country_code.into_blob();
        esp_wifi_result!(esp_wifi_set_country(&country))?;
    }

    Ok(())
}

fn finish_new<'d>(config: Config) -> Result<(WifiController<'d>, Interfaces<'d>), WifiError> {
    unsafe { esp_hal::rng::TrngSource::increase_entropy_source_counter() };

    let mut controller = WifiController {
        _phantom: Default::default(),
        beacon_timeout: 6,
        ap_beacon_timeout: 100,
    };

    controller.set_power_saving(config.power_save_mode)?;

    Ok((
        controller,
        Interfaces {
            sta: WifiDevice {
                _phantom: Default::default(),
                mode: WifiDeviceMode::Sta,
            },
            ap: WifiDevice {
                _phantom: Default::default(),
                mode: WifiDeviceMode::Ap,
            },
            #[cfg(all(feature = "esp-now", feature = "unstable"))]
            esp_now: crate::esp_now::EspNow::new_internal(),
            #[cfg(all(feature = "sniffer", feature = "unstable"))]
            sniffer: super::Sniffer::new(),
        },
    ))
}

pub(crate) fn wifi_new<'d>(
    device: crate::hal::peripherals::WIFI<'d>,
    config: Config,
) -> Result<(WifiController<'d>, Interfaces<'d>), WifiError> {
    esp_println::println!("esp_radio: legacy_port_wifi_new stage=validate");
    validate_config(config)?;
    esp_println::println!("esp_radio: legacy_port_wifi_new stage=wifi_init");
    unsafe { wifi_init(device, config)? };
    esp_println::println!("esp_radio: legacy_port_wifi_new stage=set_country");
    set_country(config)?;
    esp_println::println!("esp_radio: legacy_port_wifi_new stage=finish_new");
    finish_new(config)
}

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    control::start(controller)
}

pub(crate) fn stop(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    control::stop(controller)
}

pub(crate) fn scan_with_config(
    controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    control::scan_with_config(controller, config)
}

pub(crate) fn tx_can_send() -> bool {
    rx::tx_can_send()
}

pub(crate) fn increase_tx_inflight() {
    rx::increase_tx_inflight();
}

pub(crate) fn tx_token(mode: WifiDeviceMode) -> Option<WifiTxToken> {
    rx::tx_token(mode)
}

pub(crate) fn rx_token(mode: WifiDeviceMode, can_send: bool) -> Option<(WifiRxToken, WifiTxToken)> {
    rx::rx_token(mode, can_send)
}

pub(crate) fn register_receive_waker(mode: WifiDeviceMode, cx: &mut Context<'_>) {
    rx::register_receive_waker(mode, cx);
}

pub(crate) fn consume_rx_token<R, F>(mode: WifiDeviceMode, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    rx::consume_rx_token(mode, f)
}

pub(crate) fn consume_tx_token<R, F>(mode: WifiDeviceMode, len: usize, f: F) -> R
where
    F: FnOnce(&mut [u8]) -> R,
{
    rx::consume_tx_token(mode, len, f)
}

pub(crate) unsafe extern "C" fn recv_cb_sta(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { rx::recv_cb_sta(buffer, len, eb) }
}

pub(crate) unsafe extern "C" fn recv_cb_ap(
    buffer: *mut crate::binary::c_types::c_void,
    len: u16,
    eb: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { rx::recv_cb_ap(buffer, len, eb) }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) unsafe extern "C" fn promiscuous_rx_cb(
    buf: *mut core::ffi::c_void,
    frame_type: u32,
) {
    unsafe { rx::promiscuous_rx_cb(buf, frame_type) }
}

#[cfg(all(feature = "sniffer", feature = "unstable"))]
pub(crate) fn sniffer_set(cb: fn(PromiscuousPkt<'_>)) {
    rx::sniffer_set(cb);
}
