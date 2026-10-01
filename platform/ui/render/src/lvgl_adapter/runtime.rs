//! Checked operations on the LVGL runtime itself rather than on one widget.

#![forbid(unsafe_code)]

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

use super::{
    invalidate_runtime, raw, registry_contains_addr, AccessFault, UiAccessToken,
    UNMANAGED_SCREEN_CAPTURED, WIDGET_REGISTRY,
};

static RUNTIME_CLAIMED: AtomicBool = AtomicBool::new(false);
static ADOPTED_ONCE: AtomicBool = AtomicBool::new(false);

/// Why an LVGL runtime session could not be claimed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSessionError {
    /// Another [`RuntimeSession`] still owns the process-global runtime.
    AlreadyClaimed,
    /// [`RuntimeSession::initialize`] requires an uninitialized runtime.
    AlreadyInitialized,
    /// [`RuntimeSession::adopt_initialized`] requires board code to have
    /// initialized LVGL immediately before handing ownership over.
    NotInitialized,
    /// A board-owned runtime was already adopted during this process.
    AlreadyAdopted,
}

/// Exclusive ownership of one process-global LVGL runtime epoch.
///
/// The session is deliberately non-cloneable and non-Send/non-Sync. Dropping
/// it invalidates every token from the epoch. Sessions created with
/// [`Self::initialize`] also deinitialize LVGL; adopted board-owned runtimes
/// are only released, because their display bridge owns the shutdown policy.
pub struct RuntimeSession {
    token: UiAccessToken,
    deinit_on_drop: bool,
    _not_send_or_sync: PhantomData<*const ()>,
}

impl RuntimeSession {
    /// Initialize and exclusively claim a fresh LVGL runtime.
    pub fn initialize() -> Result<Self, RuntimeSessionError> {
        Self::claim(false)
    }

    /// Claim an LVGL runtime initialized by a board-specific display bridge.
    ///
    /// The adapter does not deinitialize an adopted runtime on drop. This
    /// keeps board-owned callback and panel lifetimes under the board's policy
    /// while still giving product code a checked epoch token.
    pub fn adopt_initialized() -> Result<Self, RuntimeSessionError> {
        Self::claim(true)
    }

    fn claim(adopt: bool) -> Result<Self, RuntimeSessionError> {
        RUNTIME_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| RuntimeSessionError::AlreadyClaimed)?;

        let initialized = raw::is_initialized();
        if adopt != initialized {
            RUNTIME_CLAIMED.store(false, Ordering::Release);
            return Err(if adopt {
                RuntimeSessionError::NotInitialized
            } else {
                RuntimeSessionError::AlreadyInitialized
            });
        }
        if adopt
            && ADOPTED_ONCE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            RUNTIME_CLAIMED.store(false, Ordering::Release);
            return Err(RuntimeSessionError::AlreadyAdopted);
        }
        if !adopt {
            raw::init();
        }
        Ok(Self {
            token: UiAccessToken::issue(),
            deinit_on_drop: !adopt,
            _not_send_or_sync: PhantomData,
        })
    }

    /// Obtain a checked capability for this session's current epoch.
    pub fn access_token(&self) -> UiAccessToken {
        self.token.clone()
    }

    /// Capture LVGL allocator health and usage without exposing its C struct.
    pub fn memory_snapshot(&self) -> LvglMemorySnapshot {
        raw::memory_snapshot(&self.token.capability().expect("live runtime session"))
    }
}

impl Drop for RuntimeSession {
    fn drop(&mut self) {
        super::runtime_io::clear_runtime_publications(self.token.runtime_id);
        invalidate_runtime(self.token.runtime_id);
        if self.deinit_on_drop {
            raw::deinit();
        }
        RUNTIME_CLAIMED.store(false, Ordering::Release);
    }
}

/// Typed copy of LVGL's allocator monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LvglMemorySnapshot {
    pub total_size: usize,
    pub free_count: usize,
    pub free_size: usize,
    pub largest_free_size: usize,
    pub used_count: usize,
    pub max_used: usize,
    pub used_percent: u8,
    pub fragmentation_percent: u8,
    pub integrity_ok: bool,
}

/// Reports LVGL's process-global initialization state.
///
/// Unlike widget operations, LVGL defines this query before initialization
/// and after deinitialization, so it intentionally does not require a
/// [`UiAccessToken`].
pub fn is_initialized() -> bool {
    raw::is_initialized()
}

impl UiAccessToken {
    /// Capture LVGL allocator health and usage for this runtime epoch.
    pub fn memory_snapshot(&self) -> Result<LvglMemorySnapshot, AccessFault> {
        self.check_current()?;
        Ok(raw::memory_snapshot(&self.capability()?))
    }

    /// Advances LVGL's clock and runs one timer-handler pass.
    pub fn run_timer_handler(&self, elapsed_ms: u32) -> Result<u32, AccessFault> {
        self.check_current()?;
        Ok(raw::run_timer_handler(&self.capability()?, elapsed_ms))
    }

    /// Immediately redraws the default display when one is registered.
    pub fn refresh_default_display(&self) -> Result<bool, AccessFault> {
        self.check_current()?;
        Ok(raw::refresh_default_display(&self.capability()?))
    }

    /// Invalidates the active screen when LVGL currently has one.
    pub fn invalidate_active_screen(&self) -> Result<bool, AccessFault> {
        self.check_current()?;
        Ok(raw::invalidate_active_screen(&self.capability()?))
    }

    /// Changes whether LVGL's system layer captures new pointer presses.
    pub fn set_system_layer_capture(&self, enabled: bool) -> Result<(), AccessFault> {
        self.check_current()?;
        raw::set_system_layer_capture(&self.capability()?, enabled);
        Ok(())
    }

    /// Captures LVGL's implicit boot screen before adapter-managed screens are
    /// constructed. A managed active screen is deliberately rejected: this
    /// handle may delete its object without touching the widget registry.
    pub fn capture_unmanaged_active_screen(&self) -> Result<Option<UnmanagedScreen>, AccessFault> {
        self.check_current()?;
        let Some(obj) = raw::active_screen(&self.capability()?) else {
            return Ok(None);
        };
        if registry_contains_addr(&WIDGET_REGISTRY, obj.addr()) {
            return Ok(None);
        }
        if UNMANAGED_SCREEN_CAPTURED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(UnmanagedScreen {
            obj,
            runtime_id: self.runtime_id,
            _not_send_or_sync: PhantomData,
        }))
    }
}

/// Opaque ownership of LVGL's implicit, pre-adapter boot screen.
pub struct UnmanagedScreen {
    obj: raw::Object,
    runtime_id: u32,
    _not_send_or_sync: PhantomData<*const ()>,
}

impl UnmanagedScreen {
    fn check(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        token.check_current()?;
        if self.runtime_id != token.runtime_id {
            return Err(AccessFault::WrongRuntime);
        }
        Ok(())
    }

    /// Restores the boot screen while initialization rolls back.
    pub fn activate(&self, token: &UiAccessToken) -> Result<bool, AccessFault> {
        self.check(token)?;
        Ok(raw::activate(&token.capability()?, self.obj))
    }

    /// Deletes the boot screen after an adapter-managed root is active.
    pub fn delete(self, token: &UiAccessToken) -> Result<(), UnmanagedScreenDeleteFailure> {
        if let Err(fault) = self.check(token) {
            return Err(UnmanagedScreenDeleteFailure::Access(fault));
        }
        if !raw::delete(
            &token
                .capability()
                .map_err(UnmanagedScreenDeleteFailure::Access)?,
            self.obj,
        ) {
            return Err(UnmanagedScreenDeleteFailure::StillValid(self));
        }
        Ok(())
    }
}

pub enum UnmanagedScreenDeleteFailure {
    Access(AccessFault),
    StillValid(UnmanagedScreen),
}
