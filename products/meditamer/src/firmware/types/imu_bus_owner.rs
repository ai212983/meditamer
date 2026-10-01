use core::sync::atomic::{AtomicU32, Ordering};

use embassy_embedded_hal::shared_bus::I2cDeviceError;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex, signal::Signal,
};
use embassy_time::{with_deadline, Duration, Instant};
use esp_hal::i2c::master::Error;
use inkplate_tempera::imu::{InkplateImu, RawImuSample};

use super::i2c::{self, ConfiguredI2cDevice};

type BusError = I2cDeviceError<Error>;
const CLAIMED: u32 = 1 << 31;
const QUEUE_DEADLINE: Duration = Duration::from_millis(40);
const TRANSFER_DEADLINE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
enum Command {
    Init(u16),
    Sample,
    PowerGood,
}

struct Request {
    id: u32,
    started: Instant,
    expires: Instant,
    command: Command,
}

enum Response {
    Init(bool),
    Sample(RawImuSample),
    PowerGood(u8),
}

struct Reply {
    id: u32,
    result: Result<Response, ImuOwnerError>,
}

struct Protocol {
    requests: Channel<CriticalSectionRawMutex, Request, 1>,
    replies: Signal<CriticalSectionRawMutex, Reply>,
    client: Mutex<CriticalSectionRawMutex, ()>,
    sequence: AtomicU32,
    active: AtomicU32,
}

impl Protocol {
    const fn new() -> Self {
        Self {
            requests: Channel::new(),
            replies: Signal::new(),
            client: Mutex::new(()),
            sequence: AtomicU32::new(0),
            active: AtomicU32::new(0),
        }
    }
}

static PROTOCOL: Protocol = Protocol::new();

#[derive(Debug)]
pub enum ImuOwnerError {
    Bus(BusError),
    QueueTimeout,
    TransferTimeout,
    Protocol,
}

struct Active<'a>(&'a AtomicU32);

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
pub struct ImuClient;

impl ImuClient {
    async fn request(&mut self, command: Command) -> Result<Response, ImuOwnerError> {
        let started = Instant::now();
        let expires = started + QUEUE_DEADLINE;
        let _client = with_deadline(expires, PROTOCOL.client.lock())
            .await
            .map_err(|_| ImuOwnerError::QueueTimeout)?;
        let id = PROTOCOL
            .sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                Some(if id >= CLAIMED - 1 { 1 } else { id + 1 })
            })
            .unwrap();
        let id = if id >= CLAIMED - 1 { 1 } else { id + 1 };
        PROTOCOL.active.store(id, Ordering::Release);
        let _active = Active(&PROTOCOL.active);
        with_deadline(
            expires,
            PROTOCOL.requests.send(Request {
                id,
                started,
                expires,
                command,
            }),
        )
        .await
        .map_err(|_| ImuOwnerError::QueueTimeout)?;
        let mut deadline = expires;
        loop {
            match with_deadline(deadline, PROTOCOL.replies.wait()).await {
                Ok(reply) if reply.id == id => return reply.result,
                Ok(_) => continue,
                Err(_) if deadline == expires => {
                    if PROTOCOL
                        .active
                        .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        return Err(ImuOwnerError::QueueTimeout);
                    }
                    deadline = expires + TRANSFER_DEADLINE;
                }
                Err(_) => return Err(ImuOwnerError::TransferTimeout),
            }
        }
    }

    pub async fn init(&mut self, sensor_odr_hz: u16) -> Result<bool, ImuOwnerError> {
        match self.request(Command::Init(sensor_odr_hz)).await? {
            Response::Init(ready) => Ok(ready),
            _ => Err(ImuOwnerError::Protocol),
        }
    }

    pub async fn read_latest(&mut self) -> Result<RawImuSample, ImuOwnerError> {
        match self.request(Command::Sample).await? {
            Response::Sample(sample) => Ok(sample),
            _ => Err(ImuOwnerError::Protocol),
        }
    }

    pub async fn read_power_good(&mut self) -> Result<u8, ImuOwnerError> {
        match self.request(Command::PowerGood).await? {
            Response::PowerGood(value) => Ok(value),
            _ => Err(ImuOwnerError::Protocol),
        }
    }
}

#[embassy_executor::task]
pub async fn owner_task(bus: ConfiguredI2cDevice<400, 1>) {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    let mut imu = InkplateImu::new(bus);
    loop {
        let request = PROTOCOL.requests.receive().await;
        if Instant::now() >= request.expires
            || PROTOCOL
                .active
                .compare_exchange(
                    request.id,
                    request.id | CLAIMED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
        {
            continue;
        }
        i2c::begin_imu_owner_request(request.started.as_micros() as u32);
        let result = match with_deadline(request.expires + TRANSFER_DEADLINE, async {
            match request.command {
                Command::Init(hz) => imu.init(hz).await.map(Response::Init),
                Command::Sample => imu.read_latest().await.map(Response::Sample),
                Command::PowerGood => imu.read_power_good().await.map(Response::PowerGood),
            }
        })
        .await
        {
            Ok(result) => result.map_err(ImuOwnerError::Bus),
            Err(_) => Err(ImuOwnerError::TransferTimeout),
        };
        i2c::end_imu_owner_request();
        PROTOCOL.replies.signal(Reply {
            id: request.id,
            result,
        });
    }
}
