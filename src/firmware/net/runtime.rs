//! Network stack setup and its runner task.
//!
//! Brings up the STA device and the Embassy network stack over it. Nothing here
//! knows what the stack will carry -- the upload HTTP server is one consumer,
//! not the owner.

use embassy_net::{Runner, Stack, StackResources};
use esp_hal::rng::Rng;
use esp_println::println;
use static_cell::StaticCell;

use super::wifi;
use super::wifi::{WifiController, WifiDevice};
use crate::firmware::types::WifiCredentials;

const NET_STACK_SOCKETS: usize = 4;

fn wifi_setup_stage_trace_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_SETUP_STAGE_TRACE") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(option_env!("WIFI_SETUP_STAGE_TRACE"), Some(raw) if raw != "0"),
    }
}

fn wifi_setup_stage_trace(stage: &str) {
    if wifi_setup_stage_trace_enabled() {
        println!("upload_http: wifi_setup_stage stage={stage}");
    }
}

pub(crate) struct NetRuntime {
    pub(crate) wifi_controller: WifiController<'static>,
    pub(crate) initial_credentials: Option<WifiCredentials>,
    pub(crate) net_runner: Runner<'static, WifiDevice>,
    pub(crate) stack: Stack<'static>,
}

pub(crate) fn setup(
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
) -> Result<NetRuntime, &'static str> {
    wifi_setup_stage_trace("setup.begin");
    println!("upload_http: wifi_backend name={}", wifi::backend_name());
    let initial_credentials = wifi::compiled_wifi_credentials();

    static STACK_RESOURCES: StaticCell<StackResources<NET_STACK_SOCKETS>> = StaticCell::new();

    let (wifi_controller, sta_device) = wifi::initialize_runtime_sta(wifi_peripheral)?;
    wifi::apply_runtime_setup_overrides_and_log();
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    wifi_setup_stage_trace("embassy_net_new.before");
    let (stack, net_runner) = embassy_net::new(
        sta_device,
        embassy_net::Config::dhcpv4(Default::default()),
        STACK_RESOURCES.init(StackResources::<NET_STACK_SOCKETS>::new()),
        seed,
    );
    wifi_setup_stage_trace("embassy_net_new.after");

    Ok(NetRuntime {
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
    stack: Stack<'static>,
) {
    wifi::run_wifi_connection_task(controller, credentials, stack).await;
}

#[embassy_executor::task]
pub(crate) async fn net_task(mut runner: Runner<'static, WifiDevice>) {
    runner.run().await
}
