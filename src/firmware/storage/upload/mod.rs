mod http;
mod sd_bridge;
mod wifi;

use embassy_net::{Runner, Stack, StackResources};
use esp_hal::rng::Rng;
use esp_radio::wifi::{InternalWifiError, WifiController, WifiDevice, WifiError};
use static_cell::StaticCell;

use super::super::types::WifiCredentials;

pub(crate) struct UploadHttpRuntime {
    pub(crate) wifi_controller: WifiController<'static>,
    pub(crate) initial_credentials: Option<WifiCredentials>,
    pub(crate) net_runner: Runner<'static, WifiDevice<'static>>,
    pub(crate) stack: Stack<'static>,
}

pub(crate) fn setup(
    wifi: esp_hal::peripherals::WIFI<'static>,
) -> Result<UploadHttpRuntime, &'static str> {
    let initial_credentials = wifi::compiled_wifi_credentials();

    static RADIO_CTRL: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
    static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

    let radio_ctrl = esp_radio::init().map_err(|err| {
        esp_println::println!("asset-upload-http: esp_radio::init err={:?}", err);
        "asset-upload-http: esp_radio::init failed"
    })?;
    let radio_ctrl = RADIO_CTRL.init(radio_ctrl);
    let (wifi_controller, ifaces) =
        esp_radio::wifi::new(radio_ctrl, wifi, wifi::wifi_runtime_config()).map_err(
            |err| match err {
                WifiError::InvalidArguments => "asset-upload-http: wifi init failed invalid_args",
                WifiError::Unsupported => "asset-upload-http: wifi init failed unsupported",
                WifiError::NotInitialized => "asset-upload-http: wifi init failed not_initialized",
                WifiError::InternalError(InternalWifiError::NoMem) => {
                    "asset-upload-http: wifi init failed no_mem"
                }
                _ => "asset-upload-http: wifi init failed other",
            },
        )?;

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, net_runner) = embassy_net::new(
        ifaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        STACK_RESOURCES.init(StackResources::<3>::new()),
        seed,
    );

    Ok(UploadHttpRuntime {
        wifi_controller,
        initial_credentials,
        net_runner,
        stack,
    })
}

#[embassy_executor::task]
pub(crate) async fn wifi_connection_task(
    controller: WifiController<'static>,
    credentials: Option<WifiCredentials>,
) {
    wifi::run_wifi_connection_task(controller, credentials).await;
}

#[embassy_executor::task]
pub(crate) async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
pub(crate) async fn http_server_task(stack: Stack<'static>) {
    http::run_http_server(stack).await;
}
