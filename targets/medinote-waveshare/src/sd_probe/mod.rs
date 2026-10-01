//! Single-owner probe: all DMA buffers stay owned until HAL cancellation and
//! target quiescence checks finish. This is not the shared storage service.
mod fault;
pub mod policy;
mod recovery;

use aligned::{Aligned, A4};
use block_device_driver::BlockDevice as _;
use core::{
    cell::Cell,
    future::{poll_fn, Future},
};
use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Delay, Duration, Instant, Timer};
use esp_hal::{sdmmc::Slot, Async};
use policy::{fill_pattern, fresh_pair, fresh_seed, verify_pattern, Config, Fault};

const CLOCK_HZ: u32 = 20_000_000;
type Blocks = [Aligned<A4, [u8; 512]>; 2];
type Card<'a> = sdio::BlockDevice<sdio::sd::Card, &'a mut Slot<'static, 1, Async>, Delay, 512>;

#[derive(Debug)]
enum Error {
    Deadline,
    Card(sdio::MmcError),
    Config(policy::ConfigError),
    Pattern(policy::PatternMismatch),
    Recovery(recovery::RecoveryError),
    ReadChanged,
    FaultNotObserved,
    UnsupportedCapacity,
}

pub fn halt() -> ! {
    loop {
        esp_hal::delay::Delay::new().delay_millis(1000);
    }
}

/// This function owns the timed future. Its drop (including HAL's DropGuard)
/// finishes before quiesce observes the peripheral or the caller regains buffers.
async fn bounded<T>(
    phase: &str,
    millis: u64,
    future: impl Future<Output = Result<T, sdio::MmcError>>,
) -> Result<T, Error> {
    bounded_fault(phase, millis, future, Fault::None).await
}

async fn bounded_fault<T>(
    phase: &str,
    millis: u64,
    future: impl Future<Output = Result<T, sdio::MmcError>>,
    fault: Fault,
) -> Result<T, Error> {
    let mut injection = fault::Injection::new(fault);
    let start = Instant::now();
    let max_late = Cell::new(0_u64);
    let max_poll = Cell::new(0_u64);
    let min_stack = Cell::new(usize::MAX);
    let operation = async {
        let mut future = core::pin::pin!(future);
        poll_fn(|cx| {
            min_stack.set(min_stack.get().min(sample_stack_free()));
            let poll_start = Instant::now();
            let result = future.as_mut().poll(cx);
            if result.is_pending() {
                injection.poll_pending();
            }
            max_poll.set(max_poll.get().max(poll_start.elapsed().as_micros()));
            result
        })
        .await
    };
    let ticker = async {
        loop {
            let due = Instant::now() + Duration::from_millis(1);
            Timer::at(due).await;
            max_late.set(
                max_late
                    .get()
                    .max(Instant::now().as_micros().saturating_sub(due.as_micros())),
            );
        }
    };
    let mut result =
        match with_timeout(Duration::from_millis(millis), select(operation, ticker)).await {
            Ok(Either::First(result)) => result.map_err(Error::Card),
            Ok(Either::Second(())) => unreachable!(),
            Err(_) => Err(Error::Deadline),
        };
    // A cooperative timer cannot interrupt a synchronous poll. Reject a late
    // Ready result as well, and report poll duration separately from the timer.
    if start.elapsed() > Duration::from_millis(millis) {
        result = Err(Error::Deadline);
    }
    if let Err(error) = recovery::quiesce() {
        // Never return DMA storage to callers if stopped ownership is uncertain.
        console::println!(
            "SD_PROBE state=done result=error phase={} quiescence={:?}",
            phase,
            error
        );
        halt();
    }
    if !injection.finish() {
        result = Err(Error::FaultNotObserved);
    }
    console::println!(
        "SD_PROBE phase={} elapsed_us={} max_pending_timer_late_us={} max_poll_us={} sampled_stack_free={} result={:?}",
        phase,
        start.elapsed().as_micros(),
        max_late.get(),
        max_poll.get(),
        min_stack.get(),
        result.as_ref().map(|_| ())
    );
    result
}

#[embassy_executor::task]
pub async fn run(
    mut slot: Slot<'static, 1, Async>,
    blocks: &'static mut Blocks,
    config: Config,
    fault: Fault,
) {
    console::println!(
        "SD_PROBE state=start scratch={:?} fault={:?}",
        config.scratch,
        fault
    );
    console::println!("SD_PROBE state=limits legacy_cmd8=unsupported electrical_noise=unqualified other_cards=unqualified");
    let result = qualify(&mut slot, blocks, config, fault).await;
    match result {
        Ok(()) => console::println!(
            "SD_PROBE state=done result=ok writes={} hardware_qualification=partial",
            config.scratch.is_some()
        ),
        Err(error) => {
            report_error(error);
            console::println!("SD_PROBE state=done result=error");
        }
    }
}

fn report_error(error: Error) {
    // Keep each cause visible (including command/data CRC variants) rather than
    // flattening transport faults into a generic init failure.
    match error {
        Error::Card(e) => {
            console::println!("SD_PROBE error=card detail={:?}", e)
        }
        Error::Config(e) => console::println!("SD_PROBE error=config detail={:?}", e),
        Error::Pattern(e) => console::println!("SD_PROBE error=pattern detail={:?}", e),
        Error::Recovery(e) => console::println!("SD_PROBE error=recovery detail={:?}", e),
        e => console::println!("SD_PROBE error={:?}", e),
    }
}

async fn qualify(
    slot: &mut Slot<'static, 1, Async>,
    blocks: &mut Blocks,
    config: Config,
    fault: Fault,
) -> Result<(), Error> {
    slot.set_bus_low_level(esp_hal::sdmmc::BusWidth::Bit1, 400_000)
        .map_err(|_| Error::Recovery(recovery::RecoveryError::BusConfiguration))?;
    let initial = {
        let mut card = Card::new_uninit_sd_card(&mut *slot, Delay);
        match bounded("enumerate", 5000, card.reacquire(CLOCK_HZ)).await {
            Ok(()) => inspect(&mut card, blocks, config).await,
            Err(error) => Err(error),
        }
    };
    // A fresh BlockDevice clears sdio's private failed-IO state. Controller
    // reset keeps the always-powered card and GPIO routing; it is no rail cycle.
    recovery::reset(slot).map_err(Error::Recovery)?;
    let evidence = {
        let mut card = Card::new_uninit_sd_card(&mut *slot, Delay);
        bounded("reinitialize", 5000, card.reacquire(CLOCK_HZ)).await?;
        bounded("recovery_read", 2000, card.read(0, &mut blocks[..1])).await?;
        let evidence = match initial {
            Ok(evidence) => evidence,
            Err(error) => {
                console::println!(
                    "SD_PROBE state=recovery result=read_ok original_failure=retained"
                );
                return Err(error);
            }
        };
        verify_recovery(&mut card, blocks, &evidence).await?;
        evidence
    };
    if fault != Fault::None {
        exercise_fault(slot, blocks, &evidence, fault).await?;
        recovery::reset(slot).map_err(Error::Recovery)?;
        let mut fresh = Card::new_uninit_sd_card(&mut *slot, Delay);
        bounded("fault_reinitialize", 5000, fresh.reacquire(CLOCK_HZ)).await?;
        bounded("fault_recovery_read", 2000, fresh.read(0, &mut blocks[..1])).await?;
        verify_recovery(&mut fresh, blocks, &evidence).await?;
        // Change the verified contents so a stale/no-op write cannot pass.
        if let Some((start, _)) = evidence.scratch {
            let seed = fresh_seed(&blocks[0], start);
            fill_pattern(&mut blocks[0], seed);
            bounded("post_fault_write", 2000, fresh.write(start, &blocks[..1])).await?;
            blocks[0].fill(0);
            bounded(
                "post_fault_readback",
                2000,
                fresh.read(start, &mut blocks[..1]),
            )
            .await?;
            verify_pattern(&blocks[0], seed).map_err(Error::Pattern)?;
            console::println!(
                "SD_PROBE state=post_fault_write_verified changed=true start={}",
                start
            );
        }
        console::println!("SD_PROBE state=fault_recovery result=ok fault={:?}", fault);
    }
    Ok(())
}

async fn exercise_fault(
    slot: &mut Slot<'static, 1, Async>,
    blocks: &mut Blocks,
    evidence: &Evidence,
    fault: Fault,
) -> Result<(), Error> {
    recovery::reset(slot).map_err(Error::Recovery)?;
    let mut card = Card::new_uninit_sd_card(&mut *slot, Delay);
    let clock = if fault == Fault::ReadCrc {
        1_000_000
    } else {
        CLOCK_HZ
    };
    bounded("fault_prepare", 5000, card.reacquire(clock)).await?;
    console::println!(
        "SD_PROBE state=fault_start mode={:?} clock_hz={}",
        fault,
        card.freq()
    );
    let outcome = match fault {
        Fault::WriteBusy => {
            let (start, seeds) = evidence.scratch.ok_or(Error::FaultNotObserved)?;
            fill_pattern(&mut blocks[0], seeds[0]);
            bounded_fault("write_busy", 100, card.write(start, &blocks[..1]), fault).await
        }
        Fault::ReadCrc => {
            bounded_fault("read_crc", 100, card.read(0, &mut blocks[..1]), fault).await
        }
        Fault::MissingCompletion => {
            bounded_fault(
                "missing_completion",
                100,
                card.read(0, &mut blocks[..1]),
                fault,
            )
            .await
        }
        Fault::None => return Ok(()),
    };
    let expected = matches!(
        (fault, &outcome),
        (
            Fault::WriteBusy | Fault::MissingCompletion,
            Err(Error::Deadline)
        ) | (Fault::ReadCrc, Err(Error::Card(sdio::MmcError::Crc)))
    );
    if !expected {
        if let Err(error) = outcome {
            report_error(error);
        }
        return Err(Error::FaultNotObserved);
    }
    Ok(())
}

struct Evidence {
    boot_hash: u32,
    scratch: Option<(u32, [u32; 2])>,
}

// Caller has just read sector 0 after reinitialization. Validate its contents,
// then the final scratch patterns, not merely successful command completion.
async fn verify_recovery(
    card: &mut Card<'_>,
    blocks: &mut Blocks,
    evidence: &Evidence,
) -> Result<(), Error> {
    if digest(&blocks[0][..]) != evidence.boot_hash {
        return Err(Error::ReadChanged);
    }
    if let Some((start, seeds)) = evidence.scratch {
        bounded("recovery_scratch_read", 2000, card.read(start, blocks)).await?;
        for (block, seed) in blocks.iter().zip(seeds) {
            verify_pattern(block, seed).map_err(Error::Pattern)?;
        }
    }
    console::println!("SD_PROBE state=recovery_verified result=ok");
    Ok(())
}

async fn inspect(
    card: &mut Card<'_>,
    blocks: &mut Blocks,
    config: Config,
) -> Result<Evidence, Error> {
    let info = card.card();
    let capacity = info.csd.block_count();
    console::println!("SD_PROBE state=card manufacturer={} oem={} product={} serial={} csd_version={} sectors={} high_capacity={} clock_hz={} cmd23={}", info.cid.manufacturer_id(), info.cid.oem_id(), info.cid.product_name(), info.cid.serial(), info.csd.version(), capacity, info.ocr.high_capacity(), card.freq(), info.scr.supports_cmd23());
    // sdio uses u32 sector addresses (and u32 byte addresses for SDSC).
    if capacity < 2
        || capacity > u64::from(u32::MAX)
        || (!info.ocr.high_capacity() && capacity > u64::from(u32::MAX) / 512 + 1)
    {
        return Err(Error::UnsupportedCapacity);
    }
    let scratch = config
        .scratch
        .map(|region| region.validate_capacity(capacity))
        .transpose()
        .map_err(Error::Config)?;
    bounded("boot_read", 2000, card.read(0, &mut blocks[..1])).await?;
    let boot_hash = digest(&blocks[0][..]);
    console::println!(
        "SD_PROBE state=boot_sector digest={:08x} signature={:02x}{:02x}",
        boot_hash,
        blocks[0][510],
        blocks[0][511]
    );
    bounded("multi_read", 2000, card.read(0, blocks)).await?;
    if digest(&blocks[0][..]) != boot_hash {
        return Err(Error::ReadChanged);
    }
    console::println!(
        "SD_PROBE state=sectors start=0 count=2 digest0={:08x} digest1={:08x}",
        digest(&blocks[0][..]),
        digest(&blocks[1][..])
    );
    let mut evidence = Evidence {
        boot_hash,
        scratch: None,
    };
    if let Some(region) = scratch {
        bounded("scratch_before", 2000, card.read(region.start, blocks)).await?;
        let single_seed = fresh_seed(&blocks[0], region.start);
        fill_pattern(&mut blocks[0], single_seed);
        bounded("single_write", 2000, card.write(region.start, &blocks[..1])).await?;
        blocks[0].fill(0);
        bounded(
            "single_readback",
            2000,
            card.read(region.start, &mut blocks[..1]),
        )
        .await?;
        verify_pattern(&blocks[0], single_seed).map_err(Error::Pattern)?;
        // Sector 0 contains the single-write pattern and sector 1 still contains
        // its pre-test data. Change both, including on repeated probe runs.
        let seeds = fresh_pair(&blocks[0], &blocks[1], region.start);
        for (block, seed) in blocks.iter_mut().zip(seeds) {
            fill_pattern(block, seed);
        }
        bounded("multi_write", 2000, card.write(region.start, blocks)).await?;
        for block in blocks.iter_mut() {
            block.fill(0);
        }
        bounded("multi_readback", 2000, card.read(region.start, blocks)).await?;
        for (block, seed) in blocks.iter().zip(seeds) {
            verify_pattern(block, seed).map_err(Error::Pattern)?;
        }
        evidence.scratch = Some((region.start, seeds));
        console::println!(
            "SD_PROBE state=write_verified start={} count=2",
            region.start
        );
    } else {
        console::println!("SD_PROBE state=write_skipped reason=no_scratch_authorization");
    }
    Ok(evidence)
}

fn sample_stack_free() -> usize {
    extern "C" {
        static _stack_end_cpu0: u8;
    }
    let sp: usize;
    // SAFETY: reads the Xtensa stack register only; the linker symbol is used
    // solely as an address. This sample is not a deepest-call high-water mark.
    unsafe {
        core::arch::asm!("mov {0}, a1", out(reg) sp, options(nomem, nostack));
    }
    sp.saturating_sub(core::ptr::addr_of!(_stack_end_cpu0) as usize)
}

fn digest(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}
