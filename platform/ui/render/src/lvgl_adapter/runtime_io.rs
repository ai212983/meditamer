//! Safe display, pointer-input, and flush-publication API for one LVGL epoch.

#![forbid(unsafe_code)]

use core::cell::RefCell;
use core::marker::PhantomData;

use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};

use super::{raw, AccessFault, RuntimeSession, StaticL8DrawBuffer, UiAccessToken};
use crate::DirtyArea;

#[cfg(feature = "lvgl-gestures")]
type CallbackStore = Mutex<CriticalSectionRawMutex, RefCell<Option<PointerRegistration>>>;
type FlushBlitter = fn(DirtyArea, &[u8], &mut [u8]) -> bool;
type BlitterStore = Mutex<CriticalSectionRawMutex, RefCell<Option<FlushRegistration>>>;

#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
struct PointerRegistration {
    runtime_id: u32,
    callbacks: PointerInputCallbacks,
}

#[derive(Clone, Copy)]
struct FlushRegistration {
    runtime_id: u32,
    blit: FlushBlitter,
}

#[cfg(feature = "lvgl-gestures")]
static POINTER_CALLBACKS: CallbackStore = Mutex::new(RefCell::new(None));
static FLUSH_BLITTER: BlitterStore = Mutex::new(RefCell::new(None));

/// A display registered in the session's LVGL runtime.
pub struct Display {
    raw: raw::Display,
    runtime_id: u32,
    _not_send_or_sync: PhantomData<*const ()>,
}

/// Why a display could not be registered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayCreateError {
    InvalidDimensions,
    InvalidBuffer,
    ObjectCreation,
}

/// A pointer input registered in the session's LVGL runtime.
#[cfg(feature = "lvgl-gestures")]
pub struct PointerInput {
    raw: raw::Input,
    runtime_id: u32,
    _not_send_or_sync: PhantomData<*const ()>,
}

/// Copyable checked access to the input retained by its owning
/// [`PointerInput`]. It cannot unregister or outlive the runtime epoch.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
pub struct PointerInputAccess {
    raw: raw::Input,
    runtime_id: u32,
    _not_send_or_sync: PhantomData<*const ()>,
}

/// Why a pointer input could not be registered.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerInputCreateError {
    InvalidThreshold,
    AlreadyRegistered,
    ObjectCreation,
}

/// One single-touch state reported to LVGL.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerState {
    Released,
    Pressed,
}

/// One contact in a multi-touch sample.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerContact {
    pub point: (i32, i32),
    pub state: PointerState,
    pub id: u8,
    pub timestamp: u32,
}

#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
pub(super) enum PointerSampleKind {
    Single {
        point: (i32, i32),
        state: PointerState,
    },
    Multi {
        contacts: [Option<PointerContact>; 4],
    },
}

/// One complete pointer sample, converted to LVGL's C representation only at
/// the private gateway.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
pub struct PointerSample {
    pub(super) kind: PointerSampleKind,
}

#[cfg(feature = "lvgl-gestures")]
impl PointerSample {
    pub const fn single(x: i32, y: i32, state: PointerState) -> Self {
        Self {
            kind: PointerSampleKind::Single {
                point: (x, y),
                state,
            },
        }
    }

    pub const fn multi(contacts: [Option<PointerContact>; 4]) -> Self {
        Self {
            kind: PointerSampleKind::Multi { contacts },
        }
    }
}

/// Direction decoded from a completed LVGL multi-touch gesture.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureDirection {
    Left,
    Right,
    Up,
    Down,
    Unknown,
}

/// Completed gesture decoded by the private LVGL callback boundary.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureEvent {
    Pinch {
        scale: f32,
    },
    Rotation {
        radians: f32,
    },
    TwoFingerSwipe {
        direction: GestureDirection,
        distance_px: f32,
    },
}

/// Safe product callbacks installed behind LVGL's pointer-input ABI.
#[cfg(feature = "lvgl-gestures")]
#[derive(Clone, Copy)]
pub struct PointerInputCallbacks {
    pub read: fn() -> PointerSample,
    pub gesture: fn(GestureEvent),
}

/// Why a framebuffer could not be published for a synchronous LVGL refresh.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushFrameError {
    Access(AccessFault),
    AlreadyPublished,
}

/// Scoped publication of the framebuffer targeted by LVGL's flush callback.
/// Dropping the guard always clears the raw address before the borrowed slice
/// can expire, including during unwinding in host tests.
pub struct FlushFrame<'frame> {
    active: bool,
    runtime_id: u32,
    _framebuffer: PhantomData<&'frame mut [u8]>,
    _not_send_or_sync: PhantomData<*const ()>,
}

impl FlushFrame<'_> {
    pub fn finish(mut self) {
        self.clear();
    }

    fn clear(&mut self) {
        if self.active {
            raw::clear_flush_target(self.runtime_id);
            clear_flush_blitter(self.runtime_id);
            self.active = false;
        }
    }
}

impl Drop for FlushFrame<'_> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl RuntimeSession {
    /// Register a display without a draw buffer, primarily for checked widget
    /// tests that do not render pixels.
    pub fn create_display(&self, width: i32, height: i32) -> Result<Display, DisplayCreateError> {
        if width <= 0 || height <= 0 {
            return Err(DisplayCreateError::InvalidDimensions);
        }
        let token = self.access_token();
        let raw = raw::create_display(
            &token.capability().expect("live runtime session"),
            width,
            height,
        )
        .ok_or(DisplayCreateError::ObjectCreation)?;
        Ok(Display {
            raw,
            runtime_id: token.runtime_id,
            _not_send_or_sync: PhantomData,
        })
    }

    /// Register an L8 partial-render display backed by a static aligned draw
    /// buffer. LVGL retains the buffer for the complete runtime epoch.
    pub fn create_l8_display<const WORDS: usize>(
        &self,
        width: i32,
        height: i32,
        draw_buffer: &'static StaticL8DrawBuffer<WORDS>,
        buffer_bytes: usize,
    ) -> Result<Display, DisplayCreateError> {
        if width <= 0 || height <= 0 {
            return Err(DisplayCreateError::InvalidDimensions);
        }
        if buffer_bytes == 0
            || buffer_bytes < width as usize
            || buffer_bytes > draw_buffer.capacity_bytes()
            || u32::try_from(buffer_bytes).is_err()
        {
            return Err(DisplayCreateError::InvalidBuffer);
        }
        let token = self.access_token();
        let raw = raw::create_l8_display(
            &token.capability().expect("live runtime session"),
            width,
            height,
            draw_buffer.as_mut_ptr(),
            buffer_bytes as u32,
        )
        .ok_or(DisplayCreateError::ObjectCreation)?;
        Ok(Display {
            raw,
            runtime_id: token.runtime_id,
            _not_send_or_sync: PhantomData,
        })
    }

    /// Register the sole pointer input for this runtime epoch.
    #[cfg(feature = "lvgl-gestures")]
    pub fn create_pointer_input(
        &self,
        rotation_threshold_radians: f32,
        callbacks: PointerInputCallbacks,
    ) -> Result<PointerInput, PointerInputCreateError> {
        if !rotation_threshold_radians.is_finite() || rotation_threshold_radians <= 0.0 {
            return Err(PointerInputCreateError::InvalidThreshold);
        }
        let token = self.access_token();
        let installed = POINTER_CALLBACKS.lock(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                false
            } else {
                *slot = Some(PointerRegistration {
                    runtime_id: token.runtime_id,
                    callbacks,
                });
                true
            }
        });
        if !installed {
            return Err(PointerInputCreateError::AlreadyRegistered);
        }

        let Some(raw) = raw::create_pointer_input(
            &token.capability().expect("live runtime session"),
            token.runtime_id,
            rotation_threshold_radians,
        ) else {
            clear_pointer_callbacks(token.runtime_id);
            return Err(PointerInputCreateError::ObjectCreation);
        };
        Ok(PointerInput {
            raw,
            runtime_id: token.runtime_id,
            _not_send_or_sync: PhantomData,
        })
    }
}

impl Display {
    fn check(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        token.check_current()?;
        (self.runtime_id == token.runtime_id)
            .then_some(())
            .ok_or(AccessFault::WrongRuntime)
    }

    /// Make this display LVGL's default display.
    pub fn make_default(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::set_default_display(&token.capability()?, self.raw);
        Ok(())
    }
}

#[cfg(feature = "lvgl-gestures")]
impl PointerInput {
    pub fn access(&self) -> PointerInputAccess {
        PointerInputAccess {
            raw: self.raw,
            runtime_id: self.runtime_id,
            _not_send_or_sync: PhantomData,
        }
    }

    /// Force one immediate read so queued Down/Up samples cannot collapse.
    pub fn read(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.access().read(token)
    }

    /// Clear LVGL's state for this input without resetting other inputs.
    pub fn reset(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.access().reset(token)
    }
}

#[cfg(feature = "lvgl-gestures")]
impl PointerInputAccess {
    fn check(&self, token: &UiAccessToken) -> Result<(), AccessFault> {
        token.check_current()?;
        if self.runtime_id != token.runtime_id {
            return Err(AccessFault::WrongRuntime);
        }
        raw::pointer_input_is_registered(self.runtime_id, self.raw)
            .then_some(())
            .ok_or(AccessFault::Stale)
    }

    pub fn read(self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::read_pointer_input(&token.capability()?, self.raw);
        Ok(())
    }

    pub fn reset(self, token: &UiAccessToken) -> Result<(), AccessFault> {
        self.check(token)?;
        raw::reset_pointer_input(&token.capability()?, self.raw);
        Ok(())
    }
}

#[cfg(feature = "lvgl-gestures")]
impl Drop for PointerInput {
    fn drop(&mut self) {
        raw::delete_pointer_input(self.runtime_id, self.raw);
        clear_pointer_callbacks(self.runtime_id);
    }
}

impl UiAccessToken {
    /// Publish a mutable panel framebuffer for one synchronous LVGL refresh.
    pub fn publish_l8_frame<'frame>(
        &self,
        framebuffer: &'frame mut [u8],
        blit: fn(DirtyArea, &[u8], &mut [u8]) -> bool,
    ) -> Result<FlushFrame<'frame>, FlushFrameError> {
        self.check_current().map_err(FlushFrameError::Access)?;
        if !raw::publish_flush_target(
            &self.capability().map_err(FlushFrameError::Access)?,
            self.runtime_id,
            framebuffer,
        ) {
            return Err(FlushFrameError::AlreadyPublished);
        }
        FLUSH_BLITTER.lock(|slot| {
            *slot.borrow_mut() = Some(FlushRegistration {
                runtime_id: self.runtime_id,
                blit,
            })
        });
        Ok(FlushFrame {
            active: true,
            runtime_id: self.runtime_id,
            _framebuffer: PhantomData,
            _not_send_or_sync: PhantomData,
        })
    }
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn pointer_sample(runtime_id: u32) -> PointerSample {
    let callbacks = POINTER_CALLBACKS.lock(|slot| *slot.borrow());
    callbacks
        .filter(|entry| entry.runtime_id == runtime_id)
        .map_or(
            PointerSample::single(0, 0, PointerState::Released),
            |entry| (entry.callbacks.read)(),
        )
}

#[cfg(feature = "lvgl-gestures")]
pub(super) fn emit_gesture(runtime_id: u32, event: GestureEvent) {
    if let Some(entry) = POINTER_CALLBACKS
        .lock(|slot| *slot.borrow())
        .filter(|entry| entry.runtime_id == runtime_id)
    {
        (entry.callbacks.gesture)(event);
    }
}

pub(super) fn blit_flush(
    runtime_id: u32,
    area: DirtyArea,
    pixels: &[u8],
    framebuffer: &mut [u8],
) -> bool {
    FLUSH_BLITTER
        .lock(|slot| *slot.borrow())
        .filter(|entry| entry.runtime_id == runtime_id)
        .is_some_and(|entry| (entry.blit)(area, pixels, framebuffer))
}

#[cfg(feature = "lvgl-gestures")]
fn clear_pointer_callbacks(runtime_id: u32) {
    POINTER_CALLBACKS.lock(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some_and(|entry| entry.runtime_id == runtime_id) {
            *slot = None;
        }
    });
}

fn clear_flush_blitter(runtime_id: u32) {
    FLUSH_BLITTER.lock(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some_and(|entry| entry.runtime_id == runtime_id) {
            *slot = None;
        }
    });
}

pub(super) fn clear_runtime_publications(runtime_id: u32) {
    raw::clear_flush_target(runtime_id);
    clear_flush_blitter(runtime_id);
    raw::delete_active_pointer_input(runtime_id);
    #[cfg(feature = "lvgl-gestures")]
    clear_pointer_callbacks(runtime_id);
}
