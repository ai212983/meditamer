//! CPU0 request handles and the CPU1-local transport service.
use embassy_embedded_hal::shared_bus::I2cDeviceError;
use embedded_hal_async::i2c::{I2c, Operation};
use esp_hal::i2c::master::Error;
use inkplate_tempera::bus_proxy::{BusDevice, BusProxy, BusRate, OwnerBus, TimeoutDiagnostic};

type BusError = I2cDeviceError<Error>;
static PROXY: BusProxy<BusError> = BusProxy::with_timeout_sink(Some(timeout_diagnostic));
pub type RemoteI2cDevice = BusDevice<'static, BusError>;

fn timeout_diagnostic(diagnostic: TimeoutDiagnostic) {
    let (bus_address, bus_age_us) =
        super::i2c::holder_snapshot(embassy_time::Instant::now().as_micros() as u32);
    console::println!(
        "I2C_PROXY_TIMEOUT phase={} id={} addr=0x{:02x} elapsed_us={} claimed={}",
        diagnostic.phase.label(),
        diagnostic.id,
        diagnostic.address,
        diagnostic.elapsed_us,
        diagnostic.claimed
    );
    console::println!(
        "I2C_PROXY_OWNER id={} phase={} last_id={} last_addr=0x{:02x} age_us={} received={} discarded={} bus_addr=0x{:02x} bus_age_us={}",
        diagnostic.id, diagnostic.owner_phase.label(), diagnostic.owner_id,
        diagnostic.owner_address, diagnostic.owner_age_us, diagnostic.received,
        diagnostic.discarded, bus_address, bus_age_us
    );
}

pub fn panel_device() -> RemoteI2cDevice {
    PROXY.device(BusRate::Panel)
}
pub fn rtc_device() -> RemoteI2cDevice {
    PROXY.device(BusRate::Standard)
}

struct LocalBus {
    standard: super::SharedI2cDevice,
    panel: super::i2c::ConfiguredI2cDevice<400>,
}
impl OwnerBus for LocalBus {
    type Error = BusError;
    async fn transaction(
        &mut self,
        rate: BusRate,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        match rate {
            BusRate::Standard => self.standard.transaction(address, operations).await,
            BusRate::Panel => self.panel.transaction(address, operations).await,
        }
    }
}

#[embassy_executor::task]
pub async fn owner_task(bus: &'static super::SharedI2cBus) {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    let mut local = LocalBus {
        standard: super::shared_i2c_device(bus),
        panel: super::panel_i2c_device(bus),
    };
    PROXY.serve(&mut local).await
}

/// CPU0 panel initialization and exclusive sensor bootstrap have completed.
pub fn begin_runtime_timing() {
    PROXY.reset_queue_timing();
    super::i2c::begin_runtime_timing();
}
pub(crate) fn queue_timing() -> (u32, u32) {
    (super::i2c::runtime_queue_max_us(), PROXY.queue_max_us())
}
