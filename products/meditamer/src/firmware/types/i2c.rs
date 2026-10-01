mod admission;
static ADMISSION: admission::Admission = admission::Admission::new();
mod client_timing;
mod wait_trace;
pub(crate) use client_timing::{
    imu_snapshot, reset_imu, reset_touch, touch_snapshot, touch_wait_snapshot, BusTiming,
    ImuBusTiming, TouchWait, TouchWaits, WaitDetail,
};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_embedded_hal::{shared_bus::I2cDeviceError, SetConfig};
use embedded_hal_async::i2c::{ErrorType, I2c as AsyncI2c, Operation};
use esp_hal::{
    i2c::master::{Config as I2cConfig, Error, I2c as EspI2c, SoftwareTimeout},
    time::{Duration as HalDuration, Rate},
    Async, Blocking,
};
use inkplate_tempera::imu::{ImuPairError, ImuSensorBus};
use inkplate_tempera::startup_i2c::{Diagnostics, StartupI2c};
pub(crate) use wait_trace::holder_snapshot;
pub(crate) use wait_trace::snapshot as wait_snapshot;

// Cumulative completed waits/transfers. Dropped futures are excluded, so these
// observations never prove cancellation latency or an upper bound on a stuck wait.
static RUNTIME: AtomicBool = AtomicBool::new(false);
static RUNTIME_QUEUE_MAX: AtomicU32 = AtomicU32::new(0);
static IMU_OWNER_REQUEST_AT: AtomicU32 = AtomicU32::new(0);
static IMU_OWNER_REQUEST_PENDING: AtomicBool = AtomicBool::new(false);
static IMU_OWNER_QUEUE_OFFSET: AtomicU32 = AtomicU32::new(0);

pub(super) fn begin_imu_owner_request(requested_at_us: u32) {
    IMU_OWNER_REQUEST_AT.store(requested_at_us, Ordering::Relaxed);
    IMU_OWNER_REQUEST_PENDING.store(true, Ordering::Release);
}

pub(super) fn end_imu_owner_request() {
    IMU_OWNER_REQUEST_PENDING.store(false, Ordering::Release);
    IMU_OWNER_QUEUE_OFFSET.store(0, Ordering::Relaxed);
}

pub(crate) fn begin_runtime_timing() {
    wait_trace::reset();
    RUNTIME_QUEUE_MAX.store(0, Ordering::Relaxed);
    RUNTIME.store(true, Ordering::Relaxed);
}
pub(crate) fn runtime_queue_max_us() -> u32 {
    RUNTIME_QUEUE_MAX.load(Ordering::Relaxed)
}

static QUEUE_COUNT: AtomicU32 = AtomicU32::new(0);
static QUEUE_MAX_US: AtomicU32 = AtomicU32::new(0);
static TRANSFER_COUNT: AtomicU32 = AtomicU32::new(0);
static TRANSFER_MAX_US: AtomicU32 = AtomicU32::new(0);
static TRANSFER_TIMEOUTS: AtomicU32 = AtomicU32::new(0);

pub(crate) fn timing_snapshot() -> [u32; 5] {
    [
        &QUEUE_COUNT,
        &QUEUE_MAX_US,
        &TRANSFER_COUNT,
        &TRANSFER_MAX_US,
        &TRANSFER_TIMEOUTS,
    ]
    .map(|value| value.load(Ordering::Relaxed))
}

pub struct StartupDiagnostics;

impl Diagnostics<Error> for StartupDiagnostics {
    fn failed(&mut self, address: u8, operations: &[Operation<'_>], error: &Error) {
        // Log only metadata, never returned sensor bytes or arbitrary payloads.
        // The original error is still returned to the owning driver's policy.
        for (index, operation) in operations.iter().enumerate() {
            let (kind, len, first_write_byte) = match operation {
                Operation::Write(bytes) => ("write", bytes.len(), bytes.first().copied()),
                Operation::Read(bytes) => ("read", bytes.len(), None),
            };
            console::println!(
                "I2C_STARTUP_ERROR at_ms={} address=0x{:02x} operation={}/{} kind={} len={} first_write_byte={:?} error={:?}",
                embassy_time::Instant::now().as_millis(), address, index + 1,
                operations.len(), kind, len, first_write_byte, error
            );
        }
    }
}

pub type SharedI2cTransport = StartupI2c<EspI2c<'static, Async>, StartupDiagnostics>;

/// Zero-cost shared-bus handle that applies its compile-time rate while holding
/// the bus lock.
///
/// Unlike `I2cDeviceWithConfig`, this stores only the bus pointer. The full HAL
/// configuration is constructed and consumed synchronously before the transfer
/// future is polled, keeping it out of long-lived Embassy task state.
#[derive(Clone, Copy)]
pub struct ConfiguredI2cDevice<const RATE_KHZ: u32, const CLIENT: u8 = 0> {
    bus: &'static super::SharedI2cBus,
}

impl<const RATE_KHZ: u32, const CLIENT: u8> ConfiguredI2cDevice<RATE_KHZ, CLIENT> {
    fn new(bus: &'static super::SharedI2cBus) -> Self {
        Self { bus }
    }

    async fn lock(&self, address: u8) -> Result<TracedGuard, I2cDeviceError<Error>> {
        let started = embassy_time::Instant::now();
        let started_us = started.as_micros() as u32;
        let owner_queue_offset =
            if CLIENT == 1 && IMU_OWNER_REQUEST_PENDING.swap(false, Ordering::AcqRel) {
                let offset = started_us.wrapping_sub(IMU_OWNER_REQUEST_AT.load(Ordering::Relaxed));
                IMU_OWNER_QUEUE_OFFSET.store(offset, Ordering::Relaxed);
                offset
            } else {
                0
            };
        let ticket = wait_trace::begin(started_us);
        let bus = embassy_time::with_timeout(embassy_time::Duration::from_millis(40), async {
            let permit = ADMISSION.acquire(address).await.map_err(|_| {
                console::println!("I2C_ADMISSION_ERROR reason=full");
                I2cDeviceError::Config
            })?;
            let admitted_us = embassy_time::Instant::now().as_micros() as u32;
            let guard = self.bus.lock().await;
            Ok((guard, permit, admitted_us))
        })
        .await;
        #[cfg(feature = "cpu-load")]
        let tail_work = if matches!(&bus, Ok(Ok(_))) {
            cpu_load::tail_end().map(|work| wait_trace::TailWork {
                kind: work.kind,
                id: work.id,
                us: work.us,
                pipeline_frames: work.pipeline_frames,
                pipeline_branches: work.pipeline_branches,
                pipeline_poll_us: work.pipeline_poll_us,
                touch_phase: work.touch_phase,
                touch_phase_us: work.touch_phase_us,
                task_total_us: work.task_total_us,
                irq_total_us: work.irq_total_us,
                pipeline_prep_us: work.pipeline_prep_us,
                pipeline_engine_us: work.pipeline_engine_us,
                pipeline_events_us: work.pipeline_events_us,
            })
        } else {
            None
        };
        #[cfg(not(feature = "cpu-load"))]
        let tail_work = None;
        let acquired_at = embassy_time::Instant::now().as_micros() as u32;
        let elapsed_us = acquired_at.wrapping_sub(started_us);
        let (admitted_us, woke_at, queue_trace) = match &bus {
            Ok(Ok((_, permit, admitted))) => {
                (*admitted, permit.woke_at(), Some(permit.queue_trace()))
            }
            _ => (acquired_at, None, None),
        };
        let wait = wait_trace::finish(
            ticket,
            wait_trace::WaitCompletion {
                admitted_us,
                completed_us: acquired_at,
                address,
                acquired: matches!(&bus, Ok(Ok(_))),
                woke_at,
                tail_work,
                queue_trace,
            },
        );
        let mut client_wait = wait;
        if CLIENT == 1 {
            client_wait.wait_us = client_wait.wait_us.saturating_add(owner_queue_offset);
            client_wait.admission_us = client_wait.admission_us.saturating_add(owner_queue_offset);
        }
        client_timing::record_touch_wait(CLIENT, client_wait);
        QUEUE_COUNT.fetch_add(1, Ordering::Relaxed);
        let total_queue_us = elapsed_us.saturating_add(owner_queue_offset);
        QUEUE_MAX_US.fetch_max(total_queue_us, Ordering::Relaxed);
        if RUNTIME.load(Ordering::Relaxed) {
            RUNTIME_QUEUE_MAX.fetch_max(total_queue_us, Ordering::Relaxed);
        }
        bus.map_err(|_| {
            console::println!(
                "I2C_TIMEOUT source=queue elapsed_us={}",
                started.elapsed().as_micros()
            );
            I2cDeviceError::I2c(Error::Timeout)
        })?
        .map(|(guard, permit, _)| {
            wait_trace::acquired(address, acquired_at);
            TracedGuard {
                bus: Some(guard),
                _permit: permit,
            }
        })
    }

    fn configure(&self, bus: &mut SharedI2cTransport) -> Result<(), I2cDeviceError<Error>> {
        SetConfig::set_config(
            bus,
            &i2c_config(RATE_KHZ).with_software_timeout(SoftwareTimeout::None),
        )
        .map_err(|_| I2cDeviceError::Config)
    }

    fn configure_timed(
        &self,
        bus: &mut SharedI2cTransport,
    ) -> (u32, Result<(), I2cDeviceError<Error>>) {
        let started = embassy_time::Instant::now();
        let result = self.configure(bus);
        (elapsed_us(started), result)
    }

    fn record(
        &self,
        address: u8,
        register: Option<u8>,
        queue_us: u32,
        config_us: u32,
        transfer_us: u32,
        error: bool,
    ) {
        if CLIENT != 0 {
            let queue_us = if CLIENT == 1 {
                queue_us.saturating_add(IMU_OWNER_QUEUE_OFFSET.swap(0, Ordering::Relaxed))
            } else {
                queue_us
            };
            client_timing::record(
                CLIENT,
                address,
                register,
                queue_us,
                config_us,
                transfer_us,
                error,
            );
        }
    }
}

impl ImuSensorBus for ConfiguredI2cDevice<400, 1> {
    async fn read_sensor_pair(
        &mut self,
        address: u8,
        tap_register: u8,
        tap_source: &mut [u8; 1],
        axes_register: u8,
        axes: &mut [u8; 12],
    ) -> Result<(), ImuPairError<I2cDeviceError<Error>>> {
        let started = embassy_time::Instant::now();
        let mut bus = match self.lock(address).await {
            Ok(bus) => bus,
            Err(error) => {
                self.record(address, Some(tap_register), elapsed_us(started), 0, 0, true);
                return Err(ImuPairError::Tap(error));
            }
        };
        let queue_us = elapsed_us(started);
        let (config_us, config_result) = self.configure_timed(&mut bus);
        if let Err(error) = config_result {
            drop(bus);
            self.record(address, Some(tap_register), queue_us, config_us, 0, true);
            return Err(ImuPairError::Tap(error));
        }

        let transfer_started = embassy_time::Instant::now();
        let tap_result = timed_transfer(
            address,
            "imu_tap_source",
            AsyncI2c::write_read(&mut *bus, address, &[tap_register], tap_source),
        )
        .await
        .map_err(I2cDeviceError::I2c);
        let tap_us = elapsed_us(transfer_started);
        if let Err(error) = tap_result {
            drop(bus);
            self.record(
                address,
                Some(tap_register),
                queue_us,
                config_us,
                tap_us,
                true,
            );
            return Err(ImuPairError::Tap(error));
        }

        let transfer_started = embassy_time::Instant::now();
        let axes_result = timed_transfer(
            address,
            "imu_axes",
            AsyncI2c::write_read(&mut *bus, address, &[axes_register], axes),
        )
        .await
        .map_err(I2cDeviceError::I2c);
        let axes_us = elapsed_us(transfer_started);
        drop(bus);
        self.record(
            address,
            Some(tap_register),
            queue_us,
            config_us,
            tap_us,
            false,
        );
        self.record(
            address,
            Some(axes_register),
            0,
            0,
            axes_us,
            axes_result.is_err(),
        );
        axes_result.map_err(ImuPairError::Axes)
    }
}

impl<const RATE_KHZ: u32, const CLIENT: u8> ErrorType for ConfiguredI2cDevice<RATE_KHZ, CLIENT> {
    type Error = I2cDeviceError<Error>;
}

impl<const RATE_KHZ: u32, const CLIENT: u8> AsyncI2c for ConfiguredI2cDevice<RATE_KHZ, CLIENT> {
    async fn read(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let started = embassy_time::Instant::now();
        let mut bus = match self.lock(address).await {
            Ok(bus) => bus,
            Err(error) => {
                self.record(address, None, elapsed_us(started), 0, 0, true);
                return Err(error);
            }
        };
        let queue_us = elapsed_us(started);
        let (config_us, config_result) = self.configure_timed(&mut bus);
        if let Err(error) = config_result {
            drop(bus);
            self.record(address, None, queue_us, config_us, 0, true);
            return Err(error);
        }
        let transfer_started = embassy_time::Instant::now();
        let result = timed_transfer(address, "read", AsyncI2c::read(&mut *bus, address, bytes))
            .await
            .map_err(I2cDeviceError::I2c);
        let transfer_us = elapsed_us(transfer_started);
        drop(bus);
        self.record(
            address,
            None,
            queue_us,
            config_us,
            transfer_us,
            result.is_err(),
        );
        result
    }

    async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        let started = embassy_time::Instant::now();
        let register = bytes.first().copied();
        let mut bus = match self.lock(address).await {
            Ok(bus) => bus,
            Err(error) => {
                self.record(address, register, elapsed_us(started), 0, 0, true);
                return Err(error);
            }
        };
        let queue_us = elapsed_us(started);
        let (config_us, config_result) = self.configure_timed(&mut bus);
        if let Err(error) = config_result {
            drop(bus);
            self.record(address, register, queue_us, config_us, 0, true);
            return Err(error);
        }
        let transfer_started = embassy_time::Instant::now();
        let result = timed_transfer(address, "write", AsyncI2c::write(&mut *bus, address, bytes))
            .await
            .map_err(I2cDeviceError::I2c);
        let transfer_us = elapsed_us(transfer_started);
        drop(bus);
        self.record(
            address,
            register,
            queue_us,
            config_us,
            transfer_us,
            result.is_err(),
        );
        result
    }

    async fn write_read(
        &mut self,
        address: u8,
        written: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        let started = embassy_time::Instant::now();
        let register = written.first().copied();
        let mut bus = match self.lock(address).await {
            Ok(bus) => bus,
            Err(error) => {
                self.record(address, register, elapsed_us(started), 0, 0, true);
                return Err(error);
            }
        };
        let queue_us = elapsed_us(started);
        let (config_us, config_result) = self.configure_timed(&mut bus);
        if let Err(error) = config_result {
            drop(bus);
            self.record(address, register, queue_us, config_us, 0, true);
            return Err(error);
        }
        let transfer_started = embassy_time::Instant::now();
        let result = timed_transfer(
            address,
            "write_read",
            AsyncI2c::write_read(&mut *bus, address, written, read),
        )
        .await
        .map_err(I2cDeviceError::I2c);
        let transfer_us = elapsed_us(transfer_started);
        drop(bus);
        self.record(
            address,
            register,
            queue_us,
            config_us,
            transfer_us,
            result.is_err(),
        );
        result
    }

    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        let started = embassy_time::Instant::now();
        let register = operations.iter().find_map(|op| match op {
            Operation::Write(bytes) => bytes.first().copied(),
            Operation::Read(_) => None,
        });
        let mut bus = match self.lock(address).await {
            Ok(bus) => bus,
            Err(error) => {
                self.record(address, register, elapsed_us(started), 0, 0, true);
                return Err(error);
            }
        };
        let queue_us = elapsed_us(started);
        let (config_us, config_result) = self.configure_timed(&mut bus);
        if let Err(error) = config_result {
            drop(bus);
            self.record(address, register, queue_us, config_us, 0, true);
            return Err(error);
        }
        let transfer_started = embassy_time::Instant::now();
        let result = timed_transfer(
            address,
            "transaction",
            AsyncI2c::transaction(&mut *bus, address, operations),
        )
        .await
        .map_err(I2cDeviceError::I2c);
        let transfer_us = elapsed_us(transfer_started);
        drop(bus);
        self.record(
            address,
            register,
            queue_us,
            config_us,
            transfer_us,
            result.is_err(),
        );
        result
    }
}

fn elapsed_us(started: embassy_time::Instant) -> u32 {
    started.elapsed().as_micros().min(u32::MAX as u64) as u32
}

// All physical clients run on CPU1. Release and attribution contain no await;
// another CPU1 task cannot acquire between the unlock and the timestamp update.
struct TracedGuard {
    bus: Option<
        embassy_sync::mutex::MutexGuard<
            'static,
            embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
            SharedI2cTransport,
        >,
    >,
    // Released after the physical lock and its attribution, including cancellation.
    _permit: admission::Permit<'static>,
}
impl core::ops::Deref for TracedGuard {
    type Target = SharedI2cTransport;
    fn deref(&self) -> &Self::Target {
        self.bus.as_deref().unwrap()
    }
}
impl core::ops::DerefMut for TracedGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.bus.as_deref_mut().unwrap()
    }
}
impl Drop for TracedGuard {
    fn drop(&mut self) {
        drop(self.bus.take());
        wait_trace::release(embassy_time::Instant::now().as_micros() as u32);
        #[cfg(feature = "cpu-load")]
        cpu_load::tail_begin();
    }
}

const STANDARD_I2C_KHZ: u32 = 100;
const PANEL_I2C_KHZ: u32 = 400;

pub fn i2c_config(rate_khz: u32) -> I2cConfig {
    I2cConfig::default()
        .with_frequency(Rate::from_khz(rate_khz))
        .with_software_timeout(SoftwareTimeout::Transaction(HalDuration::from_millis(40)))
}

pub fn shared_i2c_bus(i2c: EspI2c<'static, Blocking>) -> super::SharedI2cBus {
    embassy_sync::mutex::Mutex::new(StartupI2c::new(i2c.into_async(), StartupDiagnostics))
}

pub fn configured_i2c_device<const RATE_KHZ: u32>(
    bus: &'static super::SharedI2cBus,
) -> ConfiguredI2cDevice<RATE_KHZ> {
    ConfiguredI2cDevice::new(bus)
}

pub fn shared_i2c_device(bus: &'static super::SharedI2cBus) -> super::SharedI2cDevice {
    configured_i2c_device::<STANDARD_I2C_KHZ>(bus)
}

// The IMU's active path addresses only the PCAL6416A and LSM6DS3, both
// specified for 400 kHz fast-mode I2C. Other sensor clients stay at 100 kHz.
pub fn imu_i2c_device(bus: &'static super::SharedI2cBus) -> ConfiguredI2cDevice<PANEL_I2C_KHZ, 1> {
    ConfiguredI2cDevice::new(bus)
}

pub fn touch_i2c_device(bus: &'static super::SharedI2cBus) -> ConfiguredI2cDevice<100, 2> {
    ConfiguredI2cDevice::new(bus)
}

pub fn panel_i2c_device(bus: &'static super::SharedI2cBus) -> ConfiguredI2cDevice<PANEL_I2C_KHZ> {
    configured_i2c_device::<PANEL_I2C_KHZ>(bus)
}

// HAL software deadlines self-wake on every poll; use a timer instead so the
// CPU sleeps until the I2C interrupt or the same 40 ms transaction deadline.
async fn timed_transfer(
    address: u8,
    operation: &str,
    future: impl core::future::Future<Output = Result<(), Error>>,
) -> Result<(), Error> {
    let started = embassy_time::Instant::now();
    let result = embassy_time::with_timeout(embassy_time::Duration::from_millis(40), future).await;
    TRANSFER_COUNT.fetch_add(1, Ordering::Relaxed);
    TRANSFER_MAX_US.fetch_max(
        started.elapsed().as_micros().min(u32::MAX as u64) as u32,
        Ordering::Relaxed,
    );
    if matches!(result, Err(_) | Ok(Err(Error::Timeout))) {
        TRANSFER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    }
    match result {
        Ok(result) => {
            if matches!(result, Err(Error::Timeout)) {
                console::println!(
                    "I2C_TIMEOUT source=hal address=0x{:02x} operation={} elapsed_us={}",
                    address,
                    operation,
                    started.elapsed().as_micros()
                );
            }
            result
        }
        Err(_) => {
            console::println!(
                "I2C_TIMEOUT source=deadline address=0x{:02x} operation={} elapsed_us={}",
                address,
                operation,
                started.elapsed().as_micros()
            );
            Err(Error::Timeout)
        }
    }
}
