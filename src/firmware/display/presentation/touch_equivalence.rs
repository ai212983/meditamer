use crate::firmware::{
    touch::types::{TouchEvent, TouchEventKind},
    ui::lvgl::DirtyArea,
};
use crate::platform::inkplate::BinaryFramebufferDebugSnapshot;

const EQUIVALENCE_PROBE_ENABLED: bool = option_env!("MEDITAMER_TOUCH_EQUIVALENCE_PROBE").is_some();
const PRIME_THEN_SYNTHETIC_PROBE_ENABLED: bool =
    option_env!("MEDITAMER_TOUCH_PRIME_THEN_SYNTHETIC_PROBE").is_some();
const PIPELINE_REPLAY_PROBE_ENABLED: bool =
    option_env!("MEDITAMER_TOUCH_PIPELINE_REPLAY_PROBE").is_some();
const PROBE_ENABLED: bool = EQUIVALENCE_PROBE_ENABLED
    || PRIME_THEN_SYNTHETIC_PROBE_ENABLED
    || PIPELINE_REPLAY_PROBE_ENABLED;
const SYNTHETIC_START_DELAY_MS: u64 = 1_500;
const SYNTHETIC_RELEASE_DELAY_MS: u64 = 8;
const CONTROL_PAIR_DELAY_MS: u64 = 500;
const POST_PRIME_DELAY_MS: u64 = 500;
const TOP_TEST_X: u16 = 300;
const TOP_TEST_Y: u16 = 74;
const TOP_TEST_X_MIN: u16 = 210;
const TOP_TEST_X_MAX: u16 = 389;
const TOP_TEST_Y_MIN: u16 = 42;
const TOP_TEST_Y_MAX: u16 = 105;
const PRIME_X_MIN: u16 = 0;
const PRIME_X_MAX: u16 = 190;
const PRIME_Y_MIN: u16 = 150;
const PRIME_Y_MAX: u16 = 350;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SyntheticTouchPhase {
    Down,
    Up,
}

impl SyntheticTouchPhase {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeStage {
    Disabled,
    WaitingSyntheticDown,
    WaitingSyntheticUp,
    WaitingPhysicalDown,
    WaitingPhysicalUp,
    WaitingControlSyntheticDown,
    WaitingControlSyntheticUp,
    WaitingPipelineReplayRequest,
    WaitingPipelineReplayDown,
    WaitingPipelineReplayUp,
    WaitingPrimeDown,
    WaitingPrimeUp,
    WaitingPostPrimeSyntheticDown,
    WaitingPostPrimeSyntheticUp,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RenderSignature {
    framebuffer: BinaryFramebufferDebugSnapshot,
    dirty: DirtyArea,
}

impl RenderSignature {
    const fn new(framebuffer: BinaryFramebufferDebugSnapshot, dirty: DirtyArea) -> Self {
        Self { framebuffer, dirty }
    }
}

pub(in crate::firmware::display) struct TouchEquivalenceProbe {
    stage: ProbeStage,
    next_synthetic_ms: u64,
    synthetic_down: Option<RenderSignature>,
    synthetic_up: Option<RenderSignature>,
    physical_down_match: bool,
    pipeline_replay_active: bool,
}

impl TouchEquivalenceProbe {
    pub(super) const fn new() -> Self {
        Self {
            stage: ProbeStage::Disabled,
            next_synthetic_ms: 0,
            synthetic_down: None,
            synthetic_up: None,
            physical_down_match: false,
            pipeline_replay_active: false,
        }
    }

    pub(super) fn arm(&mut self, now_ms: u64) {
        if !PROBE_ENABLED {
            return;
        }
        self.stage = ProbeStage::WaitingSyntheticDown;
        self.next_synthetic_ms = now_ms.saturating_add(SYNTHETIC_START_DELAY_MS);
        esp_println::println!(
            "PANEL_EQUIV state=armed mode={} target=top_test x={} y={} synthetic_delay_ms={}",
            if PIPELINE_REPLAY_PROBE_ENABLED {
                "pipeline_replay"
            } else if PRIME_THEN_SYNTHETIC_PROBE_ENABLED {
                "prime_then_synthetic"
            } else {
                "physical_equivalence"
            },
            TOP_TEST_X,
            TOP_TEST_Y,
            SYNTHETIC_START_DELAY_MS
        );
    }

    pub(super) fn take_synthetic_event(
        &self,
        now_ms: u64,
    ) -> Option<(SyntheticTouchPhase, TouchEvent)> {
        if now_ms < self.next_synthetic_ms {
            return None;
        }
        let phase = match self.stage {
            ProbeStage::WaitingSyntheticDown
            | ProbeStage::WaitingControlSyntheticDown
            | ProbeStage::WaitingPostPrimeSyntheticDown => SyntheticTouchPhase::Down,
            ProbeStage::WaitingSyntheticUp
            | ProbeStage::WaitingControlSyntheticUp
            | ProbeStage::WaitingPostPrimeSyntheticUp => SyntheticTouchPhase::Up,
            _ => return None,
        };
        Some((phase, synthetic_event(phase, now_ms)))
    }

    pub(super) fn record_synthetic(
        &mut self,
        phase: SyntheticTouchPhase,
        framebuffer: BinaryFramebufferDebugSnapshot,
        dirty: DirtyArea,
        now_ms: u64,
    ) {
        let signature = RenderSignature::new(framebuffer, dirty);
        match (self.stage, phase) {
            (ProbeStage::WaitingSyntheticDown, SyntheticTouchPhase::Down) => {
                log_signature("synthetic", phase.label(), "recorded", signature);
                self.synthetic_down = Some(signature);
                self.stage = ProbeStage::WaitingSyntheticUp;
                self.next_synthetic_ms = now_ms.saturating_add(SYNTHETIC_RELEASE_DELAY_MS);
            }
            (ProbeStage::WaitingSyntheticUp, SyntheticTouchPhase::Up) => {
                log_signature("synthetic", phase.label(), "recorded", signature);
                self.synthetic_up = Some(signature);
                if PRIME_THEN_SYNTHETIC_PROBE_ENABLED || PIPELINE_REPLAY_PROBE_ENABLED {
                    self.stage = ProbeStage::WaitingControlSyntheticDown;
                    self.next_synthetic_ms = now_ms.saturating_add(CONTROL_PAIR_DELAY_MS);
                    esp_println::println!(
                        "PANEL_EQUIV state=scheduled_control_pair delay_ms={}",
                        CONTROL_PAIR_DELAY_MS
                    );
                } else {
                    self.stage = ProbeStage::WaitingPhysicalDown;
                    self.next_synthetic_ms = u64::MAX;
                }
            }
            (ProbeStage::WaitingControlSyntheticDown, SyntheticTouchPhase::Down) => {
                let matches = self.synthetic_down == Some(signature);
                log_signature(
                    "control_synthetic",
                    phase.label(),
                    if matches { "match" } else { "mismatch" },
                    signature,
                );
                self.physical_down_match = matches;
                self.stage = ProbeStage::WaitingControlSyntheticUp;
                self.next_synthetic_ms = now_ms.saturating_add(SYNTHETIC_RELEASE_DELAY_MS);
            }
            (ProbeStage::WaitingControlSyntheticUp, SyntheticTouchPhase::Up) => {
                let matches = self.synthetic_up == Some(signature);
                log_signature(
                    "control_synthetic",
                    phase.label(),
                    if matches { "match" } else { "mismatch" },
                    signature,
                );
                esp_println::println!(
                    "PANEL_EQUIV state=control_complete down_match={} up_match={} updates=4",
                    self.physical_down_match,
                    matches
                );
                if PIPELINE_REPLAY_PROBE_ENABLED {
                    self.stage = ProbeStage::WaitingPipelineReplayRequest;
                    self.next_synthetic_ms = u64::MAX;
                    esp_println::println!(
                        "PANEL_EQUIV state=pipeline_replay_pending target=blank x={}..{} y={}..{}",
                        PRIME_X_MIN,
                        PRIME_X_MAX,
                        PRIME_Y_MIN,
                        PRIME_Y_MAX
                    );
                } else {
                    self.stage = ProbeStage::WaitingPrimeDown;
                    self.next_synthetic_ms = u64::MAX;
                    esp_println::println!(
                        "PANEL_EQUIV state=awaiting_prime target=blank x={}..{} y={}..{} instruction=tap_once",
                        PRIME_X_MIN,
                        PRIME_X_MAX,
                        PRIME_Y_MIN,
                        PRIME_Y_MAX
                    );
                }
            }
            (ProbeStage::WaitingPostPrimeSyntheticDown, SyntheticTouchPhase::Down) => {
                self.pipeline_replay_active = false;
                let matches = self.synthetic_down == Some(signature);
                log_signature(
                    self.post_prime_source_label(),
                    phase.label(),
                    if matches { "match" } else { "mismatch" },
                    signature,
                );
                self.physical_down_match = matches;
                self.stage = ProbeStage::WaitingPostPrimeSyntheticUp;
                self.next_synthetic_ms = now_ms.saturating_add(SYNTHETIC_RELEASE_DELAY_MS);
            }
            (ProbeStage::WaitingPostPrimeSyntheticUp, SyntheticTouchPhase::Up) => {
                let matches = self.synthetic_up == Some(signature);
                log_signature(
                    self.post_prime_source_label(),
                    phase.label(),
                    if matches { "match" } else { "mismatch" },
                    signature,
                );
                self.stage = ProbeStage::Complete;
                self.next_synthetic_ms = u64::MAX;
                esp_println::println!(
                    "PANEL_EQUIV state=complete mode={} down_match={} up_match={}",
                    if PIPELINE_REPLAY_PROBE_ENABLED {
                        "pipeline_replay"
                    } else {
                        "prime_then_synthetic"
                    },
                    self.physical_down_match,
                    matches
                );
            }
            _ => {}
        }
    }

    pub(super) fn synthetic_render_missing(&mut self, phase: SyntheticTouchPhase) {
        esp_println::println!(
            "PANEL_EQUIV source=synthetic phase={} verdict=invalid reason=no_dirty_render",
            phase.label()
        );
        self.stage = ProbeStage::Disabled;
    }

    pub(super) const fn awaiting_physical_equivalence(&self) -> bool {
        matches!(self.stage, ProbeStage::WaitingPhysicalDown)
    }

    pub(super) fn begin_pipeline_replay(&mut self, control_refresh_succeeded: bool) -> bool {
        if !matches!(self.stage, ProbeStage::WaitingPipelineReplayRequest) {
            return false;
        }
        if !control_refresh_succeeded {
            esp_println::println!(
                "PANEL_EQUIV source=pipeline_replay verdict=invalid reason=control_refresh_failed"
            );
            self.stage = ProbeStage::Disabled;
            return false;
        }

        self.stage = ProbeStage::WaitingPipelineReplayDown;
        self.pipeline_replay_active = true;
        esp_println::println!(
            "PANEL_EQUIV state=pipeline_replay_requested target=blank x={}..{} y={}..{}",
            PRIME_X_MIN,
            PRIME_X_MAX,
            PRIME_Y_MIN,
            PRIME_Y_MAX
        );
        true
    }

    pub(super) fn observe_prime_event(&mut self, event: TouchEvent, rendered: bool, now_ms: u64) {
        if self.pipeline_replay_active && rendered {
            esp_println::println!(
                "PANEL_EQUIV source=pipeline_replay phase={:?} verdict=invalid reason=unexpected_dirty_render",
                event.kind
            );
            self.stage = ProbeStage::Disabled;
            self.pipeline_replay_active = false;
            return;
        }

        match (self.stage, event.kind) {
            (ProbeStage::WaitingPipelineReplayDown, TouchEventKind::Down)
                if prime_target_contains(event.x, event.y) =>
            {
                self.stage = ProbeStage::WaitingPipelineReplayUp;
                esp_println::println!(
                    "PANEL_EQUIV source=pipeline_replay phase=down verdict=accepted x={} y={} rendered=false",
                    event.x,
                    event.y
                );
            }
            (ProbeStage::WaitingPipelineReplayUp, TouchEventKind::Up | TouchEventKind::Cancel) => {
                self.stage = ProbeStage::WaitingPostPrimeSyntheticDown;
                self.next_synthetic_ms = now_ms.saturating_add(POST_PRIME_DELAY_MS);
                esp_println::println!(
                    "PANEL_EQUIV source=pipeline_replay phase=up verdict=accepted x={} y={} rendered=false post_synthetic_delay_ms={}",
                    event.x,
                    event.y,
                    POST_PRIME_DELAY_MS
                );
            }
            (ProbeStage::WaitingPrimeDown, TouchEventKind::Down)
                if prime_target_contains(event.x, event.y) =>
            {
                if rendered {
                    esp_println::println!(
                        "PANEL_EQUIV source=physical_prime phase=down verdict=invalid reason=unexpected_dirty_render"
                    );
                    self.stage = ProbeStage::Disabled;
                    return;
                }
                self.stage = ProbeStage::WaitingPrimeUp;
                esp_println::println!(
                    "PANEL_EQUIV source=physical_prime phase=down verdict=accepted x={} y={} rendered=false",
                    event.x,
                    event.y
                );
            }
            (ProbeStage::WaitingPrimeUp, TouchEventKind::Up | TouchEventKind::Cancel) => {
                if rendered {
                    esp_println::println!(
                        "PANEL_EQUIV source=physical_prime phase=up verdict=invalid reason=unexpected_dirty_render"
                    );
                    self.stage = ProbeStage::Disabled;
                    return;
                }
                self.stage = ProbeStage::WaitingPostPrimeSyntheticDown;
                self.next_synthetic_ms = now_ms.saturating_add(POST_PRIME_DELAY_MS);
                esp_println::println!(
                    "PANEL_EQUIV source=physical_prime phase=up verdict=accepted x={} y={} rendered=false post_synthetic_delay_ms={}",
                    event.x,
                    event.y,
                    POST_PRIME_DELAY_MS
                );
            }
            _ => {}
        }
    }

    pub(super) fn observe_pipeline_replay_render(&mut self, rendered: bool, source: &str) {
        if !self.pipeline_replay_active || !rendered {
            return;
        }
        esp_println::println!(
            "PANEL_EQUIV source=pipeline_replay verdict=invalid reason=unexpected_dirty_render render_source={}",
            source
        );
        self.stage = ProbeStage::Disabled;
        self.pipeline_replay_active = false;
    }

    fn post_prime_source_label(&self) -> &'static str {
        if PIPELINE_REPLAY_PROBE_ENABLED {
            "post_replay_synthetic"
        } else {
            "post_touch_synthetic"
        }
    }

    pub(super) fn observe_physical(
        &mut self,
        event: TouchEvent,
        framebuffer: BinaryFramebufferDebugSnapshot,
        dirty: DirtyArea,
    ) {
        let phase = match (self.stage, event.kind) {
            (ProbeStage::WaitingPhysicalDown, TouchEventKind::Down)
                if top_test_contains(event.x, event.y) =>
            {
                SyntheticTouchPhase::Down
            }
            (ProbeStage::WaitingPhysicalUp, TouchEventKind::Up | TouchEventKind::Cancel) => {
                SyntheticTouchPhase::Up
            }
            _ => return,
        };
        let expected = match phase {
            SyntheticTouchPhase::Down => self.synthetic_down,
            SyntheticTouchPhase::Up => self.synthetic_up,
        };
        let actual = RenderSignature::new(framebuffer, dirty);
        let verdict = if expected == Some(actual) {
            "match"
        } else {
            "mismatch"
        };
        log_signature("physical", phase.label(), verdict, actual);

        match phase {
            SyntheticTouchPhase::Down => {
                self.physical_down_match = expected == Some(actual);
                self.stage = ProbeStage::WaitingPhysicalUp;
            }
            SyntheticTouchPhase::Up => {
                self.stage = ProbeStage::Complete;
                esp_println::println!(
                    "PANEL_EQUIV state=complete down_match={} up_match={}",
                    self.physical_down_match,
                    expected == Some(actual)
                );
            }
        }
    }
}

fn synthetic_event(phase: SyntheticTouchPhase, now_ms: u64) -> TouchEvent {
    TouchEvent {
        kind: match phase {
            SyntheticTouchPhase::Down => TouchEventKind::Down,
            SyntheticTouchPhase::Up => TouchEventKind::Up,
        },
        t_ms: now_ms,
        x: TOP_TEST_X,
        y: TOP_TEST_Y,
        contact_x: TOP_TEST_X,
        contact_y: TOP_TEST_Y,
        start_x: TOP_TEST_X,
        start_y: TOP_TEST_Y,
        duration_ms: match phase {
            SyntheticTouchPhase::Down => 0,
            SyntheticTouchPhase::Up => SYNTHETIC_RELEASE_DELAY_MS as u16,
        },
        touch_count: match phase {
            SyntheticTouchPhase::Down => 1,
            SyntheticTouchPhase::Up => 0,
        },
        move_count: 0,
        max_travel_px: 0,
        release_debounce_ms: 0,
        dropout_count: 0,
    }
}

fn top_test_contains(x: u16, y: u16) -> bool {
    (TOP_TEST_X_MIN..=TOP_TEST_X_MAX).contains(&x) && (TOP_TEST_Y_MIN..=TOP_TEST_Y_MAX).contains(&y)
}

fn prime_target_contains(x: u16, y: u16) -> bool {
    (PRIME_X_MIN..=PRIME_X_MAX).contains(&x) && (PRIME_Y_MIN..=PRIME_Y_MAX).contains(&y)
}

fn log_signature(source: &str, phase: &str, verdict: &str, signature: RenderSignature) {
    let framebuffer = signature.framebuffer;
    esp_println::println!(
        "PANEL_EQUIV source={} phase={} verdict={} current_hash={:#010x} previous_hash={:?} changed_bytes={} changed_pixels={} rows={:?}..{:?} byte_columns={:?}..{:?} dirty={},{},{},{}",
        source,
        phase,
        verdict,
        framebuffer.current_hash,
        framebuffer.previous_hash,
        framebuffer.changed_bytes,
        framebuffer.changed_pixels,
        framebuffer.min_row,
        framebuffer.max_row,
        framebuffer.min_byte_column,
        framebuffer.max_byte_column,
        signature.dirty.x1,
        signature.dirty.y1,
        signature.dirty.x2,
        signature.dirty.y2,
    );
}
