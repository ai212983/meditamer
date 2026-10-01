//! BLE controller
use core::task::Poll;

use bt_hci::{
    transport::{Transport, WithIndicator},
    ControllerToHostPacket, FromHciBytes, FromHciBytesError, HostToControllerPacket, WriteHci,
};
use bt_hci_transport::{PacketKind, PacketToController, PacketToHost};
use docsplay::Display;
use esp_phy::PhyInitGuard;

use crate::{
    asynch::AtomicWaker,
    ble::{
        have_hci_read_data, read_hci, read_next, send_hci, Config, HciTransportError,
        InvalidConfigError,
    },
    RadioRefGuard,
};

#[derive(Display, Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Error enum for BLE initialization failures.
pub enum BleInitError {
    /// Failure during initial validation of the provided configuration: {0}.
    Config(InvalidConfigError),
}

impl core::error::Error for BleInitError {}

// Implement the From trait for cleaner error mapping
impl From<InvalidConfigError> for BleInitError {
    fn from(err: InvalidConfigError) -> Self {
        BleInitError::Config(err)
    }
}

/// A blocking HCI connector
#[instability::unstable]
pub struct BleConnector<'d> {
    _phy_init_guard: PhyInitGuard<'d>,
    _device: crate::hal::peripherals::BT<'d>,
    _guard: RadioRefGuard,
}

impl Drop for BleConnector<'_> {
    fn drop(&mut self) {
        crate::ble::ble_deinit();
    }
}
impl<'d> BleConnector<'d> {
    /// Create and init a new BLE connector.
    #[instability::unstable]
    pub fn new(
        device: crate::hal::peripherals::BT<'d>,
        config: Config,
    ) -> Result<BleConnector<'d>, BleInitError> {
        let _guard = RadioRefGuard::new();

        config.validate()?;

        Ok(Self {
            _phy_init_guard: crate::ble::ble_init(&config),
            _device: device,
            _guard,
        })
    }

    /// Read the next HCI packet from the BLE controller.
    #[instability::unstable]
    pub fn next(&mut self, buf: &mut [u8]) -> Result<usize, BleConnectorError> {
        Ok(read_next(buf))
    }

    /// Read from HCI.
    #[instability::unstable]
    pub fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, BleConnectorError> {
        let mut total = 0;
        while !buf.is_empty() {
            let len = read_hci(buf);
            if len == 0 {
                break;
            }

            buf = &mut buf[len..];
            total += len;
        }
        Ok(total)
    }

    /// Read from HCI.
    #[instability::unstable]
    pub async fn read_async(&mut self, buf: &mut [u8]) -> Result<usize, BleConnectorError> {
        if buf.is_empty() {
            return Ok(0);
        }

        if !have_hci_read_data() {
            HciReadyEventFuture.await;
        }

        self.read(buf)
    }

    /// Write to HCI without blocking the async executor.
    #[instability::unstable]
    pub async fn write_async(&mut self, buf: &[u8]) -> Result<usize, BleConnectorError> {
        send_hci(buf).await?;
        Ok(buf.len())
    }
}

#[derive(Display, Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Error type for the BLE connector.
#[instability::unstable]
pub enum BleConnectorError {
    /// Unknown BLE error occurred.
    Unknown,
    /// The bounded controller transport rejected or timed out a transfer: {0}.
    Transport(HciTransportError),
}

impl From<HciTransportError> for BleConnectorError {
    fn from(error: HciTransportError) -> Self {
        Self::Transport(error)
    }
}

impl embedded_io_06::Error for BleConnectorError {
    fn kind(&self) -> embedded_io_06::ErrorKind {
        embedded_io_06::ErrorKind::Other
    }
}

impl embedded_io_07::Error for BleConnectorError {
    fn kind(&self) -> embedded_io_07::ErrorKind {
        embedded_io_07::ErrorKind::Other
    }
}

impl core::error::Error for BleConnectorError {}

impl embedded_io_06::ErrorType for BleConnector<'_> {
    type Error = BleConnectorError;
}

impl embedded_io_06::Read for BleConnector<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.read(buf)
    }
}

impl embedded_io_07::ErrorType for BleConnector<'_> {
    type Error = BleConnectorError;
}

impl embedded_io_07::Read for BleConnector<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.read(buf)
    }
}

static HCI_WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn hci_read_data_available() {
    HCI_WAKER.wake();
}

impl embedded_io_async_06::Read for BleConnector<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, BleConnectorError> {
        self.read_async(buf).await
    }
}

impl embedded_io_async_06::Write for BleConnector<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, BleConnectorError> {
        self.write_async(buf).await
    }

    async fn flush(&mut self) -> Result<(), BleConnectorError> {
        // nothing to do
        Ok(())
    }
}

impl embedded_io_async_07::Read for BleConnector<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, BleConnectorError> {
        self.read_async(buf).await
    }
}

impl embedded_io_async_07::Write for BleConnector<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, BleConnectorError> {
        self.write_async(buf).await
    }

    async fn flush(&mut self) -> Result<(), BleConnectorError> {
        // nothing to do
        Ok(())
    }
}

impl From<FromHciBytesError> for BleConnectorError {
    fn from(_e: FromHciBytesError) -> Self {
        BleConnectorError::Unknown
    }
}

impl<E: embedded_io_07::Error> From<bt_hci_transport::ReadHciError<E>> for BleConnectorError {
    fn from(_error: bt_hci_transport::ReadHciError<E>) -> Self {
        BleConnectorError::Unknown
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub(crate) struct HciReadyEventFuture;

impl core::future::Future for HciReadyEventFuture {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        HCI_WAKER.register(cx.waker());

        if have_hci_read_data() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// The largest HCI packet, including the packet type indicator byte.
const MAX_HCI_PACKET_LEN: usize = 259;

/// Zero-sized writer used by both HCI transport generations to serialize
/// directly through the bounded, deadline-enforced controller path.
struct HciWriter;

impl embedded_io_07::ErrorType for HciWriter {
    type Error = BleConnectorError;
}

impl embedded_io_async_07::Write for HciWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        send_hci(buf).await?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Wait for one complete controller packet and copy it into `rx`.
async fn next_packet(rx: &mut [u8]) -> usize {
    if !have_hci_read_data() {
        HciReadyEventFuture.await;
    }

    read_next(rx)
}

fn parse_hci(data: &[u8]) -> Result<Option<ControllerToHostPacket<'_>>, BleConnectorError> {
    match ControllerToHostPacket::from_hci_bytes_complete(data) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            warn!("[hci] error parsing packet: {:?}", e);
            Err(BleConnectorError::Unknown)
        }
    }
}

impl Transport for BleConnector<'_> {
    /// Read a complete HCI packet into the rx buffer
    async fn read<'a>(&self, rx: &'a mut [u8]) -> Result<ControllerToHostPacket<'a>, Self::Error> {
        loop {
            if !have_hci_read_data() {
                HciReadyEventFuture.await;
            }

            // Workaround for borrow checker.
            // Safety: we only return a reference to x once, if parsing is successful.
            let rx =
                unsafe { &mut *core::ptr::slice_from_raw_parts_mut(rx.as_mut_ptr(), rx.len()) };

            let len = crate::ble::read_next(rx);
            if let Some(packet) = parse_hci(&rx[..len])? {
                return Ok(packet);
            }
        }
    }

    /// Write a complete HCI packet from the tx buffer
    async fn write<T: HostToControllerPacket>(&self, val: &T) -> Result<(), Self::Error> {
        let mut buf: [u8; 259] = [0; 259];
        let w = WithIndicator::new(val);
        let len = w.size();
        w.write_hci(&mut buf[..])
            .map_err(|_| BleConnectorError::Unknown)?;
        send_hci(&buf[..len]).await?;
        Ok(())
    }
}

// `bt-hci` 0.10 moved the wire transport contract into the independent
// `bt-hci-transport` crate. Implementing it alongside the retained 0.8 trait
// lets newer hosts share this reviewed controller without coupling the
// controller crate to the host's `bt-hci` release.
impl bt_hci_transport::Transport for BleConnector<'_> {
    async fn read<'a, P: PacketToHost<'a>>(&self, rx: &'a mut [u8]) -> Result<P, Self::Error> {
        // `P::read_hci` writes the decoded packet into `rx`, so the raw HCI
        // bytes need a separate fixed-capacity staging buffer.
        let mut packet = [0; MAX_HCI_PACKET_LEN];
        let len = next_packet(&mut packet).await;

        let mut reader = &packet[..len];
        let kind = PacketKind::read(&mut reader)?;
        Ok(P::read_hci(kind, &mut reader, rx)?)
    }

    async fn write<P: PacketToController>(&self, packet: &P) -> Result<(), Self::Error> {
        bt_hci_transport::WithIndicator::new(packet)
            .write_hci_async(HciWriter)
            .await
    }
}
