//! Ambient Home surface: a dithered sky, a clipped sun sprite positioned
//! by local time along the configured trajectory, the sumi-e mountain
//! whose snow coverage tracks the selected ambient temperature, and low-churn
//! environmental readings. The tap-to-show clock is a shell-managed
//! overlay and is not part of this screen's widget tree.
//!
//! The sky/sun artwork lives on the SD card (`/assets/AMBIENT/SKY.BIN`,
//! packed by `tools/ambient_sky`) and is composed into a PSRAM L8 canvas
//! at each scheduled movement step. Until the pack lands, the surface
//! shows a loading placeholder; after bounded loader retries it parks on
//! a failure placeholder. Both keep the screen navigable.
//!
//! The mountain has 100% snow at 18 C, its natural 20% snow at the 22 C
//! work target, and 0% snow at 24 C. It uses the same selected, corrected
//! temperature as the footer, before footer rounding. Coverage is checked
//! on the sun journey's scheduled 08:00-20:00 boundaries plus the midnight
//! and 08:00 checks. Until the mountain pack
//! (`/assets/AMBIENT/MOUNTAIN.BIN`) lands, the scene composes the exact
//! sky/sun baseline and tracks coverage without mountain pixels.
//!
//! Wall-clock time comes from the serial task -- the sole owner of RTC I2C
//! access (`docs/references/runtime/serial-control.md#time-synchronization`) -- over the
//! `WALL_CLOCK_REQUESTS`/`WALL_CLOCK_RESPONSES` channel pair
//! (`crate::firmware::display::wall_clock`). This module never caches a
//! clock reading across polls; [`AmbientViewScreen::poll`] only decides
//! *when* a fresh read is due, using the monotonic tick clock, and every
//! application uses whatever snapshot was just fetched.
//!
//! NOTE: this module intentionally carries no `#![forbid(unsafe_code)]`
//! (unlike its siblings): `environment_font` includes the build-generated
//! face, whose `unsafe impl Sync` the generator emits. The overlay slices
//! solve the same tension the same way -- the `forbid` lives in
//! `overlay/clock.rs` while the include lives in the sibling
//! `overlay/clock_font.rs` without one. This module contains no `unsafe`
//! itself.

mod composer;
#[cfg(feature = "cpu-load")]
mod cpu_footer;
mod environment;
mod environment_font;
mod model;
mod shared;

pub(in crate::firmware::ui) use shared::SharedCache;

use render::lvgl_adapter::{StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind};

use crate::firmware::config::{
    INKPLATE_HUMIDITY_SCALE_PERMILLE, INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES,
};
use crate::firmware::environment::{EnvironmentSnapshot, EnvironmentStateSnapshot};
use crate::firmware::psram::{alloc_large_byte_buffer, BufferPlacement, LargeByteBuffer};

use composer::SunSprite;
use model::AmbientHomeConfig;
use render::intent_bridge;
use shared::{
    CANVAS_PIXELS, PLACEHOLDER_BORDER_PX, PLACEHOLDER_FAILED_BORDER, PLACEHOLDER_FAILED_FIELD,
    PLACEHOLDER_LOADING, STAGING_LEN,
};

// Fixed panel dimensions used to resolve fractional geometry.
const SURFACE_WIDTH: f32 = 600.0;
const SURFACE_HEIGHT: f32 = 600.0;

/// What the next Ambient Home tick should do. Returned by the cheap,
/// monotonic-clock-only [`AmbientViewScreen::poll`]/[`AmbientViewScreen::handle_tap`]
/// so the async wall-clock fetch (crossing to the serial task) only happens
/// when actually due.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AmbientHomeAction {
    None,
    /// A scheduled update boundary (or first entry) is due, or a fresh
    /// sky/sun composition is pending.
    FetchForUpdate,
    /// A tap requested (or refreshed) the time-on-tap display.
    FetchForShow,
    /// The clock overlay elapsed; fetch fresh time before dismissing it.
    FetchForReturn,
}

pub(in crate::firmware::ui) struct AmbientViewScreen {
    root: Widget,
    canvas_widget: Widget,
    /// Packed 45 KB composition staging (PSRAM, per activation). The
    /// publisher expands it into the shared L8 canvas in one call.
    staging: Option<LargeByteBuffer>,
    environment: environment::Footer,
    #[cfg(feature = "cpu-load")]
    cpu: cpu_footer::Footer,
    next_check_due_ms: u64,
    /// Next loader retry instant, mirrored from the shared cache each poll
    /// so the executor wakes for it (the shared cache itself is not
    /// visible to the deadline query).
    service_due_ms: u64,
    last_fraction: Option<f32>,
    failure_painted: bool,
    /// Latest raw corrected climate sample (external preferred, corrected
    /// onboard fallback), cached from sensor delivery. Evaluation input
    /// only: delivery never composes and never starts a panel refresh.
    climate: Option<model::AmbientClimate>,
    /// Whether the cached climate came from the external sensor. Tracked
    /// so source switches log even when the corrected
    /// temperature did not move.
    climate_external: bool,
    climate_age_ms: Option<u64>,
    /// Temperature-to-snow policy state. Committed only after a successful canvas
    /// publication; restored on compose failure so the retry re-evaluates.
    snow_policy: mountain_snow::policy::State,
    /// Last *published* mountain coverage and unavailable flag: the
    /// composition cursor alongside `last_fraction`. `None` coverage with
    /// no mountain pack yet means the exact sky/sun baseline, never a
    /// placeholder 0% or 20% mountain.
    mountain_percent: Option<u8>,
    mountain_unavailable: bool,
    /// Last observed temperature and source, for change-gated diagnostics.
    last_temperature_centidegrees: Option<i16>,
    last_temperature_external: bool,
}

/// Configuration is compiled in rather than persisted. Recomputing the small
/// resolved value keeps [`AmbientViewScreen`] compact on lifecycle paths.
fn config() -> AmbientHomeConfig {
    AmbientHomeConfig::DEFAULT.validated()
}

/// The sun path's 4 pixel-space Bezier control points for `config`, resolved
/// against the fixed panel surface size. Recomputed on demand (a handful of
/// multiplications) rather than cached in [`AmbientViewScreen`].
fn curve_points(config: &AmbientHomeConfig) -> [model::PixelPoint; 4] {
    [
        model::to_pixels(config.arc_start, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.control_1, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.control_2, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.arc_end, SURFACE_WIDTH, SURFACE_HEIGHT),
    ]
}

impl AmbientViewScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        // Drain any late loader result even though we navigated away, so
        // its buffer is freed and the single outstanding read is released.
        while crate::firmware::storage::ambient_assets::take_assets().is_some() {}
        while crate::firmware::storage::mountain_assets::take_assets().is_some() {}
        let Self {
            root,
            canvas_widget,
            staging,
            environment,
            #[cfg(feature = "cpu-load")]
            cpu,
            next_check_due_ms,
            service_due_ms,
            last_fraction,
            failure_painted: _,
            climate,
            climate_external,
            climate_age_ms,
            snow_policy,
            mountain_percent,
            mountain_unavailable,
            last_temperature_centidegrees,
            last_temperature_external,
        } = self;
        drop(staging);
        match root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                root,
                canvas_widget,
                staging: None,
                environment,
                #[cfg(feature = "cpu-load")]
                cpu,
                next_check_due_ms,
                service_due_ms,
                last_fraction,
                failure_painted: false,
                climate,
                climate_external,
                climate_age_ms,
                snow_policy,
                mountain_percent,
                mountain_unavailable,
                last_temperature_centidegrees,
                last_temperature_external,
            }),
        }
    }

    /// Cheap, monotonic-clock-only check: does anything need a fresh
    /// wall-clock read right now? Never touches RTC/I2C itself. Drives the
    /// pack loader (channel ops only, no LVGL) and reports a pending first
    /// composition as an update so the apply path renders it.
    pub(in crate::firmware::ui) fn next_deadline_ms(&self) -> u64 {
        self.next_check_due_ms.min(self.service_due_ms)
    }

    pub(in crate::firmware::ui) fn poll(
        &mut self,
        shared: &mut SharedCache,
        now_ms: u64,
    ) -> AmbientHomeAction {
        shared.service(now_ms, self.desired_mountain_percent());
        self.service_due_ms = shared.retry_due_ms();
        // A late pack load (or mountain cursor change) publishes even when
        // the sun position has not changed.
        let compose_needed = shared.assets_ready() && !self.composition_current(shared);
        if now_ms >= self.next_check_due_ms || compose_needed {
            AmbientHomeAction::FetchForUpdate
        } else {
            AmbientHomeAction::None
        }
    }

    /// A tap landed on the screen background.
    pub(in crate::firmware::ui) fn handle_tap(&self) -> AmbientHomeAction {
        if config().tap_to_show_time {
            AmbientHomeAction::FetchForShow
        } else {
            // "When the option is disabled, surface taps retain the ambient
            // view."
            AmbientHomeAction::None
        }
    }

    /// Snow percent the loader should currently have composed (`None`
    /// while unavailable: no climate yet, or climate lost). The overlay
    /// cache queues only this percent and merges only its overlay.
    fn desired_mountain_percent(&self) -> Option<u8> {
        if self.mountain_unavailable {
            None
        } else {
            self.mountain_percent
        }
    }

    /// Whether the shared cache already holds this screen's current
    /// composition: sun position plus mountain coverage and unavailable
    /// state. Used by both the cheap poll and the compose gate so a late
    /// pack load publishes even when the sun has not moved.
    fn composition_current(&self, shared: &SharedCache) -> bool {
        shared.composition_valid
            && shared.composed_fraction == self.last_fraction
            && shared.composed_mountain_percent == self.mountain_percent
            && shared.composed_mountain_unavailable == self.mountain_unavailable
            && (self.mountain_percent.is_none()
                || self.mountain_unavailable
                || shared.composed_mountain_loaded == shared.mountain_ready())
    }

    /// Applies one fresh wall-clock query to the sun position and the
    /// mountain, then recomposes the scene when the artwork is ready and
    /// the composition (or first composition) is due. Clock overlay
    /// admission and dismissal are owned by the backend compositor.
    pub(in crate::firmware::ui) fn apply_snapshot(
        &mut self,
        token: &UiAccessToken,
        shared: &mut SharedCache,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        let snapshot = snapshot.filter(|snapshot| snapshot.valid);
        let Some(snapshot) = snapshot else {
            let changed = self.last_fraction.is_some();
            if changed {
                self.last_fraction = None;
            }
            // Without a valid clock there is no parent movement boundary.
            // Hold the last mountain image; stale climate still clears it.
            self.note_mountain_temperature(
                self.climate.map(|climate| climate.temperature_centidegrees),
            );
            if self.climate.is_none() && self.mountain_percent.is_some() {
                self.mountain_unavailable = true;
            }
            self.next_check_due_ms = now_ms + wall_clock::policy::UNAVAILABLE_RETRY_INTERVAL_MS;
            // Drive the loader for the settled desired percent now so a
            // freshly established coverage never waits for the next parent
            // boundary; then compose the sky-only frame when that
            // transition is new.
            shared.service(now_ms, self.desired_mountain_percent());
            self.service_due_ms = shared.retry_due_ms();
            self.compose_if_due(token, shared);
            return changed;
        };

        let config = config();
        let seconds_of_day = model::local_seconds_of_day(snapshot.local_epoch_seconds);
        let fraction = model::journey_fraction(&config, seconds_of_day);
        let sun_changed = self.last_fraction != Some(fraction);
        if sun_changed {
            self.last_fraction = Some(fraction);
        }
        // Evaluate the selected corrected temperature on every parent
        // check; the policy suppresses changes that would not be visible.
        let temperature = self.climate.map(|climate| climate.temperature_centidegrees);
        let cursor_backup = (
            self.snow_policy,
            self.mountain_percent,
            self.mountain_unavailable,
        );
        let temperature_changed = self.note_mountain_temperature(temperature);
        let mountain_changed = match temperature {
            Some(temperature) => {
                if self.mountain_unavailable {
                    // Recovery recomposes from a clean slate so the
                    // restored mountain cannot inherit a stale removal.
                    self.snow_policy = mountain_snow::policy::State::default();
                }
                match mountain_snow::policy::poll(
                    &mountain_snow::policy::Config::DEFAULT,
                    &mut self.snow_policy,
                    temperature,
                ) {
                    Some(percent) => {
                        self.mountain_percent = Some(percent);
                        self.mountain_unavailable = false;
                        true
                    }
                    None => {
                        // Log only when the observed input moved.
                        if temperature_changed {
                            console::println!(
                                "MOUNTAIN_TEMP status=suppressed temperature_centidegrees={} rendered_temperature_centidegrees={}",
                                temperature,
                                self.snow_policy
                                    .rendered_temperature_centidegrees()
                                    .unwrap_or(i16::MIN),
                            );
                        }
                        false
                    }
                }
            }
            None => {
                // Flip to unavailable only while a mountain is on screen:
                // with no mountain rendered there is nothing visible to
                // remove, and flipping would schedule a pixels-identical
                // Clean refresh on every check. Missing climate is logged.
                // The result is the transition edge: one recompose without
                // the mountain, then the cursor rests.
                if self.climate.is_none() && self.mountain_percent.is_some() && !cursor_backup.2 {
                    self.mountain_unavailable = true;
                    true
                } else {
                    false
                }
            }
        };
        // One parent cadence: coverage is evaluated on the sun journey's own
        // schedule (five-minute boundaries 08:00-20:00, parked outside
        // it). Overnight the mountain holds its last value; the midnight
        // wrap and 08:00 checks still recompute without a separate refresh.
        // No changed composition means no panel refresh downstream.
        let delay_seconds = model::seconds_until_next_boundary(&config, seconds_of_day);
        self.next_check_due_ms = now_ms + u64::from(delay_seconds) * 1_000;
        // Drive the loader for the settled desired percent now — a just
        // established first coverage queues promptly instead of waiting
        // five minutes for the next parent boundary — and adopt a landed
        // matching overlay in the same pass so it merges below.
        shared.service(now_ms, self.desired_mountain_percent());
        self.service_due_ms = shared.retry_due_ms();
        let published = self.compose_if_due(token, shared);
        if mountain_changed && !published {
            // Commit only after successful canvas publication: restore the
            // cursor so a failed composition retries instead of losing the
            // repaint (or the unavailable transition).
            (
                self.snow_policy,
                self.mountain_percent,
                self.mountain_unavailable,
            ) = cursor_backup;
        }
        let changed = sun_changed || mountain_changed;
        changed
    }

    /// Log a changed temperature or source without touching the composition
    /// cursor. Repeated unchanged parent checks stay silent.
    fn note_mountain_temperature(&mut self, temperature: Option<i16>) -> bool {
        let changed = self.last_temperature_centidegrees != temperature
            || (temperature.is_some() && self.last_temperature_external != self.climate_external);
        if changed {
            match temperature {
                Some(temperature) => console::println!(
                    "MOUNTAIN_TEMP status=sample temperature_centidegrees={} external={} age_ms={}",
                    temperature,
                    self.climate_external as u8,
                    self.climate_age_ms.unwrap_or(u64::MAX),
                ),
                None => console::println!("MOUNTAIN_TEMP status=unavailable reason=no_climate"),
            }
            self.last_temperature_centidegrees = temperature;
            self.last_temperature_external = self.climate_external;
        }
        changed
    }

    /// Compose and publish when the pack is ready and the composition
    /// (sun position, mountain coverage, or unavailable state) is stale
    /// or never composed. Paints the parked failure placeholder once when
    /// the loader gives up. Returns whether a full frame landed, so the
    /// caller can commit (or roll back) the mountain cursor.
    fn compose_if_due(&mut self, token: &UiAccessToken, shared: &mut SharedCache) -> bool {
        if !shared.assets_ready() {
            if shared.parked() && !self.failure_painted {
                self.failure_painted = paint_failure_placeholder(token, shared);
                if self.failure_painted {
                    let _ = self.canvas_widget.invalidate(token);
                }
            }
            return false;
        }
        if self.composition_current(shared) {
            return false;
        }
        if self.publish_composition(token, shared) {
            if !self.mountain_unavailable
                && self
                    .mountain_percent
                    .is_some_and(|percent| shared.mountain_overlay_for(percent).is_some())
            {
                crate::firmware::ui::mark_ambient_mountain_composed();
            }
            shared.composed_fraction = self.last_fraction;
            shared.composed_mountain_percent = self.mountain_percent;
            shared.composed_mountain_unavailable = self.mountain_unavailable;
            shared.composed_mountain_loaded = shared.mountain_ready();
            shared.composition_valid = true;
            true
        } else {
            false
        }
    }

    /// Expand the pack planes into staging, publish to the canvas, and
    /// invalidate it. Returns `true` only when a full frame landed.
    fn publish_composition(&mut self, token: &UiAccessToken, shared: &mut SharedCache) -> bool {
        let Some(assets) = shared.decoded() else {
            return false;
        };
        let Some(staging) = self.staging.as_mut() else {
            return false;
        };
        let frame: &mut [u8] = staging.as_mut_slice();
        if !composer::paint_sky(&mut *frame, assets.sky()) {
            return false;
        }
        let config = config();
        let [p0, p1, p2, p3] = curve_points(&config);
        if !composer::paint_arc_guide(
            frame,
            [[p0.x, p0.y], [p1.x, p1.y], [p2.x, p2.y], [p3.x, p3.y]],
        ) {
            return false;
        }
        if let Some(fraction) = self.last_fraction {
            let point = model::point_on_curve(p0, p1, p2, p3, fraction);
            let sprite = SunSprite {
                ink: assets.sun_ink(),
                mask: assets.sun_mask(),
                width: assets.sun_w(),
                height: assets.sun_h(),
                stride: assets.sun_stride(),
                anchor: assets.anchor(),
            };
            if !composer::paint_sun(&mut *frame, &sprite, [point.x, point.y]) {
                return false;
            }
        }
        if !self.mountain_unavailable {
            if let Some(percent) = self.mountain_percent {
                if let Some(overlay) = shared.mountain_overlay_for(percent) {
                    if !composer::merge_mountain_overlay(
                        frame,
                        overlay,
                        composer::MOUNTAIN_OVERLAY_FIRST_ROW,
                        composer::MOUNTAIN_OVERLAY_ROWS,
                    ) {
                        console::println!(
                            "AMBIENT_MOUNTAIN status=overlay_error percent={}",
                            percent
                        );
                        return false;
                    }
                    console::println!("AMBIENT_MOUNTAIN status=composed percent={}", percent);
                }
            }
        }
        let Some(canvas) = shared.canvas else {
            return false;
        };
        let wrote = canvas
            .with_writer(token, |writer| {
                writer.write_ink_bits(&*frame, CANVAS_PIXELS)
            })
            .ok();
        if wrote != Some(CANVAS_PIXELS) {
            return false;
        }
        self.canvas_widget.invalidate(token).is_ok()
    }

    #[cfg(feature = "cpu-load")]
    pub(in crate::firmware::ui) fn update_cpu_footer(
        &mut self,
        token: &UiAccessToken,
        now_ms: u64,
        reading: Option<cpu_load::Snapshot>,
    ) {
        self.cpu.update(token, now_ms, reading);
    }

    /// Apply a sensor sample to the low-churn footer. Returns `true` only
    /// when its rounded, visible representation changed. Also caches the
    /// raw corrected climate (external preferred, corrected onboard
    /// fallback -- the same selection the footer uses) for snow evaluation
    /// on the next parent check: delivery updates cached state only, it
    /// neither composes nor starts a panel refresh.
    pub(in crate::firmware::ui) fn apply_environment_reading(
        &mut self,
        token: &UiAccessToken,
        reading: EnvironmentSnapshot,
    ) -> bool {
        self.environment.apply(token, reading)
    }

    /// Read current observation metadata on the parent check. The provider
    /// owns acquisition cadence and failure policy; the mountain accepts a
    /// paired sample only while its configured five-minute validity horizon
    /// has not elapsed. Footer delivery remains independent.
    pub(in crate::firmware::ui) fn observe_climate(
        &mut self,
        observation: Option<EnvironmentStateSnapshot>,
        now_ms: u64,
    ) {
        use observation::policy::Health;
        let Some(state) = observation else {
            self.climate = None;
            self.climate_age_ms = None;
            return;
        };
        let age = state.last_sample_at.map(|at| now_ms.saturating_sub(at.0));
        self.climate_age_ms = age;
        let current = matches!(state.health, Health::Ok | Health::Degraded)
            && state.sample_identity().is_some()
            && state.last_sample_at.is_some_and(|at| at.0 <= now_ms)
            && age.is_some_and(|age| {
                age <= u64::from(crate::firmware::environment::ENVIRONMENT_SAMPLE_INTERVAL_S)
                    * 1_000
            });
        if !current {
            self.climate = None;
            return;
        }
        let reading = state.snapshot;
        self.climate = Some(model::ambient_climate(
            reading.onboard.temperature_centidegrees,
            reading.onboard.humidity_millipercent,
            INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES,
            INKPLATE_HUMIDITY_SCALE_PERMILLE,
            reading.external.map(|external| {
                (
                    external.temperature_centidegrees,
                    external.humidity_millipercent,
                )
            }),
        ));
        self.climate_external = reading.external.is_some();
    }
}

/// Paint the parked failure field once: distinct from the loading fill so
/// a missing pack never reads as a slow load.
fn paint_failure_placeholder(token: &UiAccessToken, shared: &SharedCache) -> bool {
    let Some(canvas) = shared.canvas else {
        return false;
    };
    let painted = canvas
        .with_writer(token, |writer| {
            writer.fill(PLACEHOLDER_FAILED_FIELD);
            let w = SURFACE_WIDTH as usize;
            let h = SURFACE_HEIGHT as usize;
            for y in 0..PLACEHOLDER_BORDER_PX {
                for x in 0..w {
                    writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                    writer.write((h - 1 - y) * w + x, PLACEHOLDER_FAILED_BORDER);
                }
            }
            for y in PLACEHOLDER_BORDER_PX..h - PLACEHOLDER_BORDER_PX {
                for x in 0..PLACEHOLDER_BORDER_PX {
                    writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                    writer.write(y * w + (w - 1 - x), PLACEHOLDER_FAILED_BORDER);
                }
            }
            // Diagonal cross: the "parked" mark that tells this state
            // apart from the loading frame.
            let diagonal = w.min(h);
            for i in 0..diagonal {
                writer.write(i * w + i, PLACEHOLDER_FAILED_BORDER);
                writer.write(i * w + (w - 1 - i), PLACEHOLDER_FAILED_BORDER);
            }
        })
        .is_ok();
    painted
}

fn abandon(root: Widget, token: &UiAccessToken) -> Option<AmbientViewScreen> {
    if let Err(WidgetDeleteFailure::StillValid(widget)) = root.delete(token) {
        render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
    }
    None
}

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    shared: &mut SharedCache,
) -> Option<AmbientViewScreen> {
    let screen = Widget::screen(token).ok()?;
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    // The composed scene is a fixed canvas, not a scroll container. Keep
    // background taps.
    let built = screen.set_scrollable(token, false).is_ok()
        && screen
            .set_bg_color(token, white, StyleState::Default)
            .is_ok()
        && screen.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && screen
            .set_text_color(token, black, StyleState::Default)
            .is_ok();
    if !built {
        return abandon(screen, token);
    }

    let canvas_widget = match screen.child(token, WidgetKind::Canvas) {
        Ok(widget) => widget,
        Err(_) => return abandon(screen, token),
    };
    if canvas_widget.remove_style_all(token).is_err()
        || canvas_widget.set_pos(token, 0, 0).is_err()
        || canvas_widget
            .set_size(token, SURFACE_WIDTH as i32, SURFACE_HEIGHT as i32)
            .is_err()
    {
        return abandon(screen, token);
    }
    // Boot-lifetime canvas: adopt strict-PSRAM L8 storage once and reuse it
    // across navigations -- never per entry, never on the stack.
    let canvas = match shared.canvas_or_keep() {
        Ok(canvas) => canvas,
        Err(_) => return abandon(screen, token),
    };
    if canvas_widget
        .set_external_l8_canvas_buffer(token, canvas, SURFACE_WIDTH as i32, SURFACE_HEIGHT as i32)
        .is_err()
    {
        return abandon(screen, token);
    }
    // Loading state: the pack read takes whole display-loop turns, so paint
    // the canvas immediately instead of leaving adapter white up.
    // Loading state: the pack read takes whole display-loop turns, so paint
    // the canvas immediately instead of leaving adapter white up.
    // Palette discipline: the binary panel renders mid-grays as white
    // (proven on-panel: a 0x40 border read blank while 0x00 fills solid),
    // so placeholders use pure ink/paper only, exactly like composed
    // content. A white field plus an ink border reads as "waiting".
    let painted = canvas.with_writer(token, |writer| {
        writer.fill(PLACEHOLDER_LOADING);
        let w = SURFACE_WIDTH as usize;
        let h = SURFACE_HEIGHT as usize;
        for y in 0..PLACEHOLDER_BORDER_PX {
            for x in 0..w {
                writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                writer.write((h - 1 - y) * w + x, PLACEHOLDER_FAILED_BORDER);
            }
        }
        for y in PLACEHOLDER_BORDER_PX..h - PLACEHOLDER_BORDER_PX {
            for x in 0..PLACEHOLDER_BORDER_PX {
                writer.write(y * w + x, PLACEHOLDER_FAILED_BORDER);
                writer.write(y * w + (w - 1 - x), PLACEHOLDER_FAILED_BORDER);
            }
        }
    });
    let painted = painted.is_ok();
    let invalidated = canvas_widget.invalidate(token).is_ok();
    console::println!(
        "AMBIENT_CANVAS status=placeholder paint={} invalidate={}",
        painted,
        invalidated
    );

    // Per-activation packed staging in strict PSRAM; released on exit.
    let staging = match alloc_large_byte_buffer(STAGING_LEN) {
        Ok(buffer) if buffer.placement() == BufferPlacement::Psram => Some(buffer),
        _ => None,
    };
    if staging.is_none() {
        console::println!("AMBIENT_CANVAS status=staging_oom");
        return abandon(screen, token);
    }

    let Some(environment) = environment::Footer::create(&screen, token) else {
        return abandon(screen, token);
    };

    #[cfg(feature = "cpu-load")]
    let Some(cpu) = cpu_footer::Footer::create(&screen, token) else {
        return abandon(screen, token);
    };

    if intent_bridge::bind_ambient_tap(&screen, token).is_err() {
        return abandon(screen, token);
    }

    console::println!("AMBIENT_SCREEN status=entered");
    Some(AmbientViewScreen {
        root: screen,
        canvas_widget,
        staging,
        environment,
        #[cfg(feature = "cpu-load")]
        cpu,
        // Due immediately: entering Ambient Home starts the pack load and
        // shows the current position as soon as local time is available.
        next_check_due_ms: 0,
        service_due_ms: 0,
        last_fraction: None,
        failure_painted: false,
        // No climate yet and no coverage rendered: the scene is the exact
        // sky/sun baseline until the first parent check evaluates.
        climate: None,
        climate_external: false,
        climate_age_ms: None,
        snow_policy: mountain_snow::policy::State::default(),
        mountain_percent: None,
        mountain_unavailable: false,
        last_temperature_centidegrees: None,
        last_temperature_external: false,
    })
}
