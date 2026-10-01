use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use critical_section::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embedded_storage::{
    nor_flash::{NorFlash, ReadNorFlash},
    ReadStorage, Storage,
};
use esp_storage::{FlashStorage, FlashStorageError};
#[cfg(feature = "asset-upload-http")]
use netstack::config::{
    flash_store::{self, CredentialFlash, FlashStoreError},
    WifiConfigResultCode, WifiCredentials,
};

static FLASH: Mutex<RefCell<Option<FlashStorage<'static>>>> = Mutex::new(RefCell::new(None));
static QUIESCE_WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static QUIESCE_INSTALLED: AtomicBool = AtomicBool::new(false);
static QUIESCE_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUIESCE_ACKNOWLEDGED: AtomicBool = AtomicBool::new(false);
const QUIESCE_TIMEOUT_US: u64 = 5_000_000;

/// Last write-path stage reached on CPU0. Single producer: the write path
/// runs to completion without awaits while holding the exclusive flash
/// owner, and CPU1 never writes here; `Relaxed` is sufficient because
/// consumers only need the last stored value, not ordering with other data.
const WRITE_STAGE_MAGIC: u32 = 0xf175_0000;
#[cfg_attr(target_os = "none", link_section = ".rtc_slow.persistent")]
static WRITE_STAGE: AtomicU32 = AtomicU32::new(FlashWriteStage::Idle as u32);

/// Write-path breadcrumb values. Numeric order follows the persist sequence;
/// `ProfilePauseEnter/Paused` never appear when `cpu-load` is disabled, and
/// `QuiesceSkipped` marks boot writes before the second core starts.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FlashWriteStage {
    Idle = 0,
    WriteEnter = 1,
    #[cfg(feature = "cpu-load")]
    ProfilePauseEnter = 2,
    #[cfg(feature = "cpu-load")]
    ProfilePaused = 3,
    QuiesceRequested = 4,
    QuiesceSkipped = 5,
    QuiesceAcknowledged = 6,
    PhysicalEnter = 7,
    PhysicalReturn = 8,
    QuiesceReleased = 9,
    WriteDone = 10,
}

fn note_write_stage(stage: FlashWriteStage) {
    WRITE_STAGE.store(WRITE_STAGE_MAGIC | stage as u32, Ordering::Relaxed);
}

/// Last write-path stage reached, for stall triage via status surfaces.
pub(crate) fn write_stage() -> u8 {
    WRITE_STAGE.load(Ordering::Relaxed) as u8
}

/// Consume the last boot's diagnostic boundaries before flash access starts.
pub fn take_persistence_trace() -> (Option<u8>, Option<u8>) {
    let raw = WRITE_STAGE.load(Ordering::Relaxed);
    WRITE_STAGE.store(0, Ordering::Relaxed);
    let write = (raw & !0xff == WRITE_STAGE_MAGIC && raw as u8 <= FlashWriteStage::WriteDone as u8)
        .then_some(raw as u8);
    (write, crate::firmware::app_state::store::take_save_stage())
}

#[cfg(feature = "asset-upload-http")]
const WIFI_CONFIG_PARTITION_OFFSET: u32 = 0x14000;

pub fn initialize(peripheral: esp_hal::peripherals::FLASH<'static>) {
    critical_section::with(|cs| {
        let mut slot = FLASH.borrow_ref_mut(cs);
        assert!(slot.is_none(), "flash owner initialized twice");
        *slot = Some(FlashStorage::new(peripheral).multicore_auto_park());
    });
}

/// Register CPU1's safe-point task after it has been spawned. Boot flash
/// access before the second core starts does not need a rendezvous.
pub fn register_other_core_quiescer() {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    QUIESCE_INSTALLED.store(true, Ordering::Release);
}

/// A cooperative CPU1 safe point before CPU0 hardware-parks it for a flash
/// write. Do not hold a cross-core lock while acknowledged: CPU0 must be able
/// to complete the ROM flash operation before it can unpark this core.
pub async fn serve_other_core_quiesce() {
    assert_eq!(
        esp_hal::system::Cpu::current(),
        esp_hal::system::Cpu::AppCpu
    );
    loop {
        QUIESCE_WAKE.wait().await;
        if !QUIESCE_REQUESTED.load(Ordering::Acquire) {
            continue;
        }
        esp_hal::xtensa_lx::interrupt::free(|| {
            QUIESCE_ACKNOWLEDGED.store(true, Ordering::Release);
            while QUIESCE_REQUESTED.load(Ordering::Acquire) {
                core::hint::spin_loop();
            }
            QUIESCE_ACKNOWLEDGED.store(false, Ordering::Release);
        });
    }
}

struct OtherCoreQuiesce(bool);

impl OtherCoreQuiesce {
    fn enter() -> Self {
        note_write_stage(FlashWriteStage::QuiesceRequested);
        if !QUIESCE_INSTALLED.load(Ordering::Acquire) {
            note_write_stage(FlashWriteStage::QuiesceSkipped);
            console::println!("FLASH_WRITE stage=quiesce_skipped");
            return Self(false);
        }
        assert_eq!(
            esp_hal::system::Cpu::current(),
            esp_hal::system::Cpu::ProCpu
        );
        assert!(!QUIESCE_REQUESTED.swap(true, Ordering::AcqRel));
        QUIESCE_WAKE.signal(());
        let started = esp_hal::time::Instant::now();
        while !QUIESCE_ACKNOWLEDGED.load(Ordering::Acquire) {
            if started.elapsed().as_micros() > QUIESCE_TIMEOUT_US {
                QUIESCE_REQUESTED.store(false, Ordering::Release);
                panic!("flash quiesce: CPU1 did not acknowledge");
            }
            core::hint::spin_loop();
        }
        note_write_stage(FlashWriteStage::QuiesceAcknowledged);
        console::println!("FLASH_WRITE stage=quiesce_acknowledged");
        Self(true)
    }
}

impl Drop for OtherCoreQuiesce {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        QUIESCE_REQUESTED.store(false, Ordering::Release);
        let started = esp_hal::time::Instant::now();
        while QUIESCE_ACKNOWLEDGED.load(Ordering::Acquire) {
            assert!(
                started.elapsed().as_micros() <= QUIESCE_TIMEOUT_US,
                "flash quiesce: CPU1 did not resume"
            );
            core::hint::spin_loop();
        }
    }
}

fn with_access<R>(mutating: bool, operation: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
    struct Lease(Option<FlashStorage<'static>>);

    impl Drop for Lease {
        fn drop(&mut self) {
            if let Some(flash) = self.0.take() {
                critical_section::with(|cs| {
                    let mut slot = FLASH.borrow_ref_mut(cs);
                    assert!(slot.is_none(), "flash owner replaced while leased");
                    *slot = Some(flash);
                });
            }
        }
    }

    // Stage markers below run on CPU0 with the instruction cache enabled and
    // no cross-core lock held; `console::println!` is lock-free and drops
    // rather than blocks on UART contention, so marking while CPU1 is
    // quiesced cannot deadlock. Nothing is marked from inside the
    // CPU1 interrupts-disabled spin or the cache-disabled ROM window.
    if mutating {
        note_write_stage(FlashWriteStage::WriteEnter);
        console::println!("FLASH_WRITE stage=write_enter");
    }

    // AutoPark hardware-stalls the other CPU during writes. Drain profiler
    // hooks, then rendezvous with CPU1 at a task safe point before the driver
    // parks it. Resume CPU1 and profiling only after the driver returns.
    #[cfg(feature = "cpu-load")]
    let _profile_pause = mutating.then(|| {
        note_write_stage(FlashWriteStage::ProfilePauseEnter);
        let guard = cpu_load::pause_for_flash();
        note_write_stage(FlashWriteStage::ProfilePaused);
        console::println!("FLASH_WRITE stage=profile_paused");
        guard
    });
    let _other_core_quiesce = mutating.then(OtherCoreQuiesce::enter);

    // The flash driver handles the interrupt/cache restrictions of each
    // physical operation. Hold our critical section only to transfer its
    // exclusive owner; a sector erase and verification must not extend it.
    let flash = critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .take()
            .expect("flash owner absent or already leased")
    });
    let mut lease = Lease(Some(flash));
    if mutating {
        note_write_stage(FlashWriteStage::PhysicalEnter);
        console::println!("FLASH_WRITE stage=physical_enter");
    }
    let result = operation(lease.0.as_mut().expect("flash lease is held"));
    if mutating {
        note_write_stage(FlashWriteStage::PhysicalReturn);
        console::println!("FLASH_WRITE stage=physical_return");
    }
    // Return the owner before releasing CPU1, as before: a resumed CPU1 task
    // may take the owner while it is still leased out.
    drop(lease);
    drop(_other_core_quiesce);
    if mutating {
        note_write_stage(FlashWriteStage::QuiesceReleased);
        console::println!("FLASH_WRITE stage=quiesce_released");
        note_write_stage(FlashWriteStage::WriteDone);
    }
    result
}

pub(crate) fn with_read<R>(operation: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
    with_access(false, operation)
}

pub(crate) fn with_write<R>(operation: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
    with_access(true, operation)
}

pub(crate) fn read(offset: u32, bytes: &mut [u8]) -> Result<(), FlashStorageError> {
    with_read(|flash| ReadStorage::read(flash, offset, bytes))
}

pub fn replace(offset: u32, bytes: &[u8]) -> Result<(), FlashStorageError> {
    with_write(|flash| Storage::write(flash, offset, bytes))
}

// Only targets/meditamer-inkplate/src/updater/install.rs (factory-updater, minus its
// sd-qual-push bench variant, which stages onto SD instead of writing raw flash) erases/writes/
// reads flash directly now that ADR-0014 Phase 5 removed the default binary's own signed-image
// write path; other feature combinations legitimately never call these. The `factory-updater`/
// `sd-qual-push` features that used to gate a local `allow(dead_code)` here live on the target
// crate now (product and target axis completion plan, Phase 3) -- these functions are `pub`,
// cross-crate API the updater binary calls, which rustc's dead-code analysis does not flag
// regardless of this crate's own feature state, so the attribute was not carried over.
pub fn erase(from: u32, to: u32) -> Result<(), FlashStorageError> {
    with_write(|flash| NorFlash::erase(flash, from, to))
}

pub fn write(offset: u32, bytes: &[u8]) -> Result<(), FlashStorageError> {
    with_write(|flash| NorFlash::write(flash, offset, bytes))
}

pub fn read_aligned(offset: u32, bytes: &mut [u8]) -> Result<(), FlashStorageError> {
    with_read(|flash| ReadNorFlash::read(flash, offset, bytes))
}

#[cfg(feature = "asset-upload-http")]
struct WifiConfigPartition<'a> {
    flash: &'a mut FlashStorage<'static>,
}

#[cfg(feature = "asset-upload-http")]
impl WifiConfigPartition<'_> {
    fn absolute(&self, offset: u32, len: u32) -> Option<(u32, u32)> {
        let end = offset.checked_add(len)?;
        if end > flash_store::PARTITION_SIZE {
            return None;
        }
        let from = WIFI_CONFIG_PARTITION_OFFSET.checked_add(offset)?;
        Some((from, from.checked_add(len)?))
    }
}

#[cfg(feature = "asset-upload-http")]
impl CredentialFlash for WifiConfigPartition<'_> {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        let Some((from, _)) = self.absolute(offset, bytes.len() as u32) else {
            return false;
        };
        ReadNorFlash::read(self.flash, from, bytes).is_ok()
    }

    fn erase(&mut self, offset: u32, len: u32) -> bool {
        let Some((from, to)) = self.absolute(offset, len) else {
            return false;
        };
        NorFlash::erase(self.flash, from, to).is_ok()
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> bool {
        let Some((from, _)) = self.absolute(offset, bytes.len() as u32) else {
            return false;
        };
        NorFlash::write(self.flash, from, bytes).is_ok()
    }
}

/// Load the newest valid credential generation from the dedicated internal
/// flash partition. An erased or corrupt partition is unprovisioned.
#[cfg(feature = "asset-upload-http")]
pub fn load_wifi_credentials() -> Result<Option<WifiCredentials>, FlashStoreError> {
    with_read(|flash| flash_store::load(&mut WifiConfigPartition { flash }))
}

/// Store credentials through the same flash singleton used by all other
/// Meditamer durable state. `Ok` is returned only after read-back verification.
#[cfg(feature = "asset-upload-http")]
pub fn store_wifi_credentials(credentials: &WifiCredentials) -> WifiConfigResultCode {
    let result =
        with_write(|flash| flash_store::store(&mut WifiConfigPartition { flash }, credentials));
    match result {
        Ok(()) => WifiConfigResultCode::Ok,
        Err(FlashStoreError::InvalidCredentials) => WifiConfigResultCode::InvalidData,
        Err(error) => {
            console::println!("WIFI_CREDENTIAL_STORE status=error phase={error:?}");
            WifiConfigResultCode::OperationFailed
        }
    }
}
