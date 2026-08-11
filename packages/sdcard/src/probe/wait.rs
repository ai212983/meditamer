use super::{SdCardProbe, SdProbeError, SdSpiBus, SD_SECTOR_SIZE, SD_WRITE_PREAMBLE_BYTES};
use embassy_time::Instant;

const SD_READY_POLL_BURST_BYTES: usize = 16;
const SD_PROTOCOL_WAIT_DEADLINE_MS: u32 = 250;

pub(super) struct WaitReadyDiag {
    pub(super) released: bool,
    pub(super) elapsed_ms: u32,
    pub(super) polls: u32,
}

impl WaitReadyDiag {
    fn new(released: bool, elapsed_ms: u32, polls: u32) -> Self {
        Self {
            released,
            elapsed_ms,
            polls,
        }
    }
}

impl<'d, SPI> SdCardProbe<'d, SPI>
where
    SPI: SdSpiBus,
{
    pub(super) async fn transfer_write_frame(
        &mut self,
        token: u8,
        data: &[u8],
    ) -> Result<(u8, WaitReadyDiag), SdProbeError> {
        if data.len() != SD_SECTOR_SIZE {
            return Err(SdProbeError::WriteLengthInvalid(data.len()));
        }
        self.cached_sector_lba = None;
        self.cached_sector[0] = 0xFF;
        self.cached_sector[1] = token;
        self.cached_sector[SD_WRITE_PREAMBLE_BYTES..SD_WRITE_PREAMBLE_BYTES + SD_SECTOR_SIZE]
            .copy_from_slice(data);
        self.cached_sector[SD_WRITE_PREAMBLE_BYTES + SD_SECTOR_SIZE..].fill(0xFF);

        let started_at = Instant::now();
        self.spi
            .transfer_in_place_with_deadline(&mut self.cached_sector)
            .await?;

        // After the preamble and payload, two bytes clock the ignored CRC16.
        // The following byte receives the data-response token and the rest
        // clocks the card's busy phase.
        let finish_start = SD_WRITE_PREAMBLE_BYTES + SD_SECTOR_SIZE;
        let response = self.cached_sector[finish_start + 2] & 0x1F;
        let ready_bytes = &self.cached_sector[finish_start + 3..];
        if let Some(index) = ready_bytes.iter().position(|byte| *byte == 0xFF) {
            return Ok((
                response,
                WaitReadyDiag::new(true, elapsed_ms_u32(started_at), index as u32 + 1),
            ));
        }

        let mut ready = self
            .wait_write_ready_from(started_at, ready_bytes.len() as u32)
            .await?;
        ready.elapsed_ms = elapsed_ms_u32(started_at);
        Ok((response, ready))
    }

    pub(super) async fn wait_data_token(&mut self, cmd: u8) -> Result<u8, SdProbeError> {
        let started_at = Instant::now();
        loop {
            let token = self.transfer_byte(0xFF).await?;
            if token != 0xFF {
                return Ok(token);
            }
            if elapsed_ms_u32(started_at) >= SD_PROTOCOL_WAIT_DEADLINE_MS {
                return Err(SdProbeError::DataTokenTimeout(cmd));
            }
            embassy_futures::yield_now().await;
        }
    }

    pub(super) async fn wait_write_ready_timed(&mut self) -> Result<WaitReadyDiag, SdProbeError> {
        self.wait_write_ready_from(Instant::now(), 0).await
    }

    async fn wait_write_ready_from(
        &mut self,
        started_at: Instant,
        mut polls: u32,
    ) -> Result<WaitReadyDiag, SdProbeError> {
        let mut response = [0xFF; SD_READY_POLL_BURST_BYTES];

        loop {
            response.fill(0xFF);
            self.spi
                .transfer_in_place_with_deadline(&mut response)
                .await?;
            if let Some(index) = response.iter().position(|byte| *byte == 0xFF) {
                polls = polls.saturating_add(index as u32 + 1);
                return Ok(WaitReadyDiag::new(true, elapsed_ms_u32(started_at), polls));
            }

            polls = polls.saturating_add(SD_READY_POLL_BURST_BYTES as u32);
            if elapsed_ms_u32(started_at) >= SD_PROTOCOL_WAIT_DEADLINE_MS {
                return Ok(WaitReadyDiag::new(false, elapsed_ms_u32(started_at), polls));
            }
            // Small SPI ready-poll transfers may complete without producing a
            // useful executor handoff. Explicitly yield so a busy card cannot
            // starve touch acquisition until the protocol deadline.
            embassy_futures::yield_now().await;
        }
    }
}

pub(super) fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
