use super::chain::write_fat_value;
use crate::fat::engine::FatEngine;
use crate::fat::{SdFatError, FAT32_EOC_WRITE};

impl FatEngine {
    pub(super) fn prepare_contiguous_allocation_batch(&mut self) -> Result<(), SdFatError> {
        let sector_first = self.allocation.sector_offset.saturating_mul(128);
        let sector_end = sector_first.saturating_add(128);
        let mut candidate = self.allocation.candidate.max(sector_first).max(2);
        let mut run_first = 0u32;
        let mut run_len = 0u32;

        while candidate < sector_end && candidate <= self.allocation.max_cluster {
            let index = ((candidate - sector_first) * 4) as usize;
            let value = u32::from_le_bytes([
                self.workspace.sector[index],
                self.workspace.sector[index + 1],
                self.workspace.sector[index + 2],
                self.workspace.sector[index + 3],
            ]) & 0x0FFF_FFFF;
            if value == 0 {
                if run_len == 0 {
                    run_first = candidate;
                }
                run_len = run_len.saturating_add(1);
                if run_len == self.allocation.remaining {
                    break;
                }
            } else {
                run_first = 0;
                run_len = 0;
            }
            candidate = candidate.saturating_add(1);
        }

        if run_len == self.allocation.remaining {
            for offset in 0..run_len {
                let cluster = run_first.saturating_add(offset);
                let value = if offset + 1 == run_len {
                    FAT32_EOC_WRITE
                } else {
                    cluster.saturating_add(1)
                };
                write_fat_value(
                    &mut self.workspace.sector,
                    ((cluster - sector_first) * 4) as usize,
                    value,
                );
            }
            self.allocation.batch_first = run_first;
            self.allocation.batch_last = run_first.saturating_add(run_len - 1);
            self.allocation.batch_count = run_len;
            self.allocation.batch_next = self.allocation.batch_last.saturating_add(1);
        } else {
            self.allocation.batch_first = 0;
            self.allocation.batch_last = self.allocation.previous;
            self.allocation.batch_count = 0;
            self.allocation.batch_next = sector_end;
        }
        self.allocation.external_previous = 0;
        Ok(())
    }
}
