//! Bounded chime/A-B playback plus the source-based manufacturer melody;
//! no product, radio or panel tasks.
#![no_std]
#![no_main]

use buzzer::{
    runner::{self, Actuator, Cancel, Clock, RunOutcome},
    score::{ActionKind, Element, Note, RequestedAction},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Instant, Timer};
use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::{clock::CpuClock, i2c::master::I2c};
use inkplate_tempera::{
    buzzer::{code_for_frequency, BuzzerRail},
    buzzer_device::BuzzerDevice,
    expander::PcalExpander,
};
use meditamer_product::firmware::types::{
    i2c_config, shared_i2c_bus, shared_i2c_device, SharedI2cBus, SharedI2cDevice,
};
use static_cell::StaticCell;

// Generated notification scores (see assets/sounds/notifications.rtttl):
// borrowed read-only statics, no task buffers, no runtime RTTTL parsing.
#[path = "../buzzer_notifications.rs"]
mod buzzer_notifications;

// Generated melody scores (see assets/sounds/melodies.rtttl): same
// generator contract as the notification bank, no runtime RTTTL parsing.
#[path = "../buzzer_melodies.rs"]
mod buzzer_melodies;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));
    let _ = meditamer_product::firmware::psram::init_allocator(p.PSRAM);
    let timers = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    esp_rtos::start(timers.timer0, p.FROM_CPU_INTR0);
    let i2c = I2c::new(p.I2C0, i2c_config(100))
        .unwrap()
        .with_sda(p.GPIO21)
        .with_scl(p.GPIO22);
    static BUS: StaticCell<SharedI2cBus> = StaticCell::new();
    let bus = BUS.init(shared_i2c_bus(i2c));
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR
        .init(esp_rtos::embassy::Executor::new())
        .run(|spawner| {
            spawner.spawn(play(bus).unwrap());
        })
}

struct ScoreClock(Instant);
impl Clock for ScoreClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis()
    }
    async fn wait_until_ms(&mut self, deadline: u64) {
        Timer::at(self.0 + embassy_time::Duration::from_millis(deadline)).await;
    }
}
struct NeverCancel;
impl Cancel for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

struct Trace {
    device: BuzzerDevice<SharedI2cDevice>,
    verbose: bool,
    default_gate: bool,
    max_apply_us: u64,
}
impl Actuator for Trace {
    type Error = <BuzzerDevice<SharedI2cDevice> as Actuator>::Error;
    async fn apply(&mut self, action: RequestedAction) -> Result<(), Self::Error> {
        let started = Instant::now();
        // This probe-only path is enabled after priming code 63. A reset
        // restores 63; residual power retains the same already-programmed code.
        let result = if self.default_gate
            && matches!(action.kind, ActionKind::PowerOnSetCode { code: 63 })
        {
            self.device.set_rail_enabled(true).await
        } else {
            self.device.apply(action).await
        };
        let elapsed_us = started.elapsed().as_micros();
        self.max_apply_us = self.max_apply_us.max(elapsed_us);
        if self.verbose {
            console::println!(
                "BUZZER_PROBE action={:?} at_ms={} elapsed_us={} ok={}",
                action.kind,
                action.at_ms,
                elapsed_us,
                result.is_ok()
            );
        }
        result
    }
    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        let result = self.device.shutdown().await;
        console::println!("BUZZER_PROBE shutdown ok={}", result.is_ok());
        result
    }
}

// Read-only flash scores: fixed pitch, 5 ms pulses in 10 ms slots. Only
// pulse density differs. Accumulator scheduling spreads the fading pulses
// rather than bunching them at the start of each block.
const fn density_score(fade: bool) -> [Element; 120] {
    let mut score = [Element::Rest { duration_ms: 5 }; 120];
    let mut accumulator = 0;
    let mut slot = 0;
    while slot < 60 {
        accumulator += if fade { 60 - slot } else { 60 };
        if accumulator >= 60 {
            accumulator -= 60;
            score[slot * 2] = Element::Note(Note {
                code: 63,
                duration_ms: 5,
            });
        }
        slot += 1;
    }
    score
}
static FLAT_DENSITY: [Element; 120] = density_score(false);
static FADING_DENSITY: [Element; 120] = density_score(true);

// Every slot remains present. The only difference is the pulse-width
// envelope; 3 ms minimum retains margin over the observed 2.4 ms power-on
// action. This is a timing-qualified starting value, not an acoustic model.
const fn envelope_score(fade: bool) -> [Element; 120] {
    let mut score = [Element::Rest { duration_ms: 0 }; 120];
    let mut slot = 0;
    while slot < 60 {
        let on_ms = if fade { 8 - (5 * slot / 59) as u32 } else { 8 };
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: 10 - on_ms,
        };
        slot += 1;
    }
    score
}
static FLAT_ENVELOPE: [Element; 120] = envelope_score(false);
static FADING_ENVELOPE: [Element; 120] = envelope_score(true);

// Same six 100 ms duty levels (80%, 70%, ... 30%) at either cadence.
// Unused entries are zero-duration rests and never reach the actuator.
const fn cadence_score(slot_ms: u32) -> [Element; 120] {
    assert!(slot_ms == 10 || slot_ms == 20);
    let mut score = [Element::Rest { duration_ms: 0 }; 120];
    let mut slot = 0;
    while slot < (600 / slot_ms) as usize {
        let on_ms = (8 - slot as u32 * slot_ms / 100) * slot_ms / 10;
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: slot_ms - on_ms,
        };
        slot += 1;
    }
    score
}
static FAST_CADENCE: [Element; 120] = cadence_score(10);
static SLOW_CADENCE: [Element; 120] = cadence_score(20);

// Three identical 200 ms duty blocks (75%, 50%, 25%). Only the gating
// period changes. Read-only scores avoid growing task-local RAM buffers.
const fn fast_cadence_score(slot_ms: u32) -> [Element; 300] {
    assert!(slot_ms == 4 || slot_ms == 8);
    let mut score = [Element::Rest { duration_ms: 0 }; 300];
    let mut slot = 0;
    while slot < (600 / slot_ms) as usize {
        let on_ms = (3 - slot as u32 * slot_ms / 200) * slot_ms / 4;
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: slot_ms - on_ms,
        };
        slot += 1;
    }
    score
}
static GATE_8MS: [Element; 300] = fast_cadence_score(8);
static GATE_4MS: [Element; 300] = fast_cadence_score(4);

// Fixed 4 ms cadence: A holds 75% duty, B uses the retained stepped fade.
const fn fast_envelope_score(fade: bool) -> [Element; 300] {
    let mut score = [Element::Rest { duration_ms: 0 }; 300];
    let mut slot = 0;
    while slot < 150 {
        let on_ms = if fade { 3 - (slot / 50) as u32 } else { 3 };
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: 4 - on_ms,
        };
        slot += 1;
    }
    score
}
static FAST_FLAT_ENVELOPE: [Element; 300] = fast_envelope_score(false);
static FAST_FADING_ENVELOPE: [Element; 300] = fast_envelope_score(true);

const fn constant_duty_score(on_ms: u32) -> [Element; 300] {
    assert!(on_ms > 0 && on_ms < 4);
    let mut score = [Element::Rest { duration_ms: 0 }; 300];
    let mut slot = 0;
    while slot < 150 {
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: 4 - on_ms,
        };
        slot += 1;
    }
    score
}
static CONSTANT_LOW_DUTY: [Element; 300] = constant_duty_score(1);

// Optional midpoint drop between the two steady listening levels.
const fn single_drop_score<const N: usize>(drop: bool) -> [Element; N] {
    assert!(N == 300 || N == 1000);
    let mut score = [Element::Rest { duration_ms: 0 }; N];
    let mut slot = 0;
    while slot < N / 2 {
        let on_ms = if drop && slot >= N / 4 { 1 } else { 3 };
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: 4 - on_ms,
        };
        slot += 1;
    }
    score
}
static SINGLE_DROP: [Element; 300] = single_drop_score(true);
static LONG_FLAT: [Element; 1000] = single_drop_score(false);
static LONG_DROP: [Element; 1000] = single_drop_score(true);

// Keep every 4 ms pulse present. Spread progressively more 1 ms pulses
// among 3 ms pulses to interpolate the two tested steady duty levels.
const fn gradual_drop_score() -> [Element; 1000] {
    let mut score = [Element::Rest { duration_ms: 0 }; 1000];
    let mut accumulator = 0;
    let mut slot = 0;
    while slot < 500 {
        accumulator += slot;
        let on_ms = if accumulator >= 499 {
            accumulator -= 499;
            1
        } else {
            3
        };
        score[slot * 2] = Element::Note(Note {
            code: 63,
            duration_ms: on_ms,
        });
        score[slot * 2 + 1] = Element::Rest {
            duration_ms: 4 - on_ms,
        };
        slot += 1;
    }
    score
}
static GRADUAL_DROP: [Element; 1000] = gradual_drop_score();

// Nominal 900/1450 Hz map to raw codes 73/34. Adjacent notes have no
// rests, so the score compiler keeps power on and emits pitch writes only.
const fn pitch_alternation_score<const N: usize>(step_ms: u32) -> [Element; N] {
    assert!((N == 20 && step_ms == 100) || (N == 200 && step_ms == 10));
    let mut score = [Element::Note(Note {
        code: 73,
        duration_ms: step_ms,
    }); N];
    let mut slot = 1;
    while slot < N {
        score[slot] = Element::Note(Note {
            code: if slot % 2 == 0 { 73 } else { 34 },
            duration_ms: step_ms,
        });
        slot += 1;
    }
    score
}
static SLOW_PITCH_ALTERNATION: [Element; 20] = pitch_alternation_score(100);
static FAST_PITCH_ALTERNATION: [Element; 200] = pitch_alternation_score(10);

// Source-based manufacturer melody: Soldered's Inkplate4TEMPERA_Buzzer loop,
// octave-transposed into the nominal model range. The source loops four
// four-note cycles over C/E/G/B [523, 659, 783, 987]: the first two cycles
// play each pitch 100 ms + 600 ms rest; the last two play 100 ms + 250 ms
// rest + same-pitch 50 ms + 300 ms rest. (Its "second four times" comment
// mislabels repeatCounter 2..=3: two cycles, not four.) The 523 Hz root
// lies below the nominal 586 Hz endpoint, so ALL requests transpose x2 to
// [1046, 1318, 1566, 1974] -> codes [58, 40, 29, 17]. One bounded pass:
// 48 elements, 24 notes, 11200 ms. No envelopes, duty gating, or bell claim.
const fn manufacturer_code(pitch: usize) -> u8 {
    match pitch {
        0 => 58, // nominal 1046 Hz request
        1 => 40, // nominal 1318 Hz request
        2 => 29, // nominal 1566 Hz request
        _ => 17, // nominal 1974 Hz request
    }
}
const fn manufacturer_melody_score() -> [Element; 48] {
    let mut score = [Element::Rest { duration_ms: 0 }; 48];
    let mut pos = 0;
    let mut cycle = 0;
    while cycle < 4 {
        let mut pitch = 0;
        while pitch < 4 {
            let code = manufacturer_code(pitch);
            score[pos] = Element::Note(Note {
                code,
                duration_ms: 100,
            });
            pos += 1;
            if cycle < 2 {
                score[pos] = Element::Rest { duration_ms: 600 };
                pos += 1;
            } else {
                score[pos] = Element::Rest { duration_ms: 250 };
                pos += 1;
                score[pos] = Element::Note(Note {
                    code,
                    duration_ms: 50,
                });
                pos += 1;
                score[pos] = Element::Rest { duration_ms: 300 };
                pos += 1;
            }
            pitch += 1;
        }
        cycle += 1;
    }
    score
}
static MANUFACTURER_MELODY: [Element; 48] = manufacturer_melody_score();

// Descending-chime A/B: two-note chime adapted from the vendor
// melody/echo rhythm. A repeats 1974 Hz; B drops the second note to
// 1566 Hz. Rhythm and durations match; only the second pitch changes.
static DESCENDING_CHIME_A: [Element; 4] = [
    Element::Note(Note {
        code: 17,
        duration_ms: 100,
    }),
    Element::Rest { duration_ms: 250 },
    Element::Note(Note {
        code: 17,
        duration_ms: 50,
    }),
    Element::Rest { duration_ms: 1000 },
];
static DESCENDING_CHIME_B: [Element; 4] = [
    Element::Note(Note {
        code: 17,
        duration_ms: 100,
    }),
    Element::Rest { duration_ms: 250 },
    Element::Note(Note {
        code: 29,
        duration_ms: 50,
    }),
    Element::Rest { duration_ms: 1000 },
];
// Ring-down A/B: one short burst at the source-specified resonance, then a
// full second of rail-off rest so the tail stays quiet. No envelope,
// modulation, lower body, echoes, or priming sound. Fixed code 5 is the
// current nominal mapping of requested 2700 Hz; the mode guards the mapping
// at runtime. Score durations: A 1100 ms, B 1020 ms.
static RING_DOWN_A: [Element; 2] = [
    Element::Note(Note {
        code: 5,
        duration_ms: 100,
    }),
    Element::Rest { duration_ms: 1000 },
];
static RING_DOWN_B: [Element; 2] = [
    Element::Note(Note {
        code: 5,
        duration_ms: 20,
    }),
    Element::Rest { duration_ms: 1000 },
];
fn note(hz: i32, duration_ms: u32) -> Element {
    Element::Note(Note {
        code: code_for_frequency(hz).unwrap(),
        duration_ms,
    })
}

async fn play_score(device: &mut Trace, round: u8, name: &str, score: &[Element]) -> bool {
    play_score_with_pause(device, round, name, score, 700).await
}

async fn play_score_with_pause(
    device: &mut Trace,
    round: u8,
    name: &str,
    score: &[Element],
    post_pause_ms: u64,
) -> bool {
    console::println!("BUZZER_PROBE begin round={} candidate={}", round, name);
    device.max_apply_us = 0;
    let outcome = runner::run(score, device, &mut ScoreClock(Instant::now()), &NeverCancel).await;
    console::println!(
        "BUZZER_PROBE end round={} candidate={} max_apply_us={} outcome={:?}",
        round,
        name,
        device.max_apply_us,
        outcome
    );
    if !matches!(outcome, RunOutcome::Completed(_)) {
        return false;
    }
    Timer::after_millis(post_pause_ms).await;
    true
}

async fn play_descending_chime(device: &mut Trace) -> bool {
    if code_for_frequency(1974) != Ok(17) || code_for_frequency(1566) != Ok(29) {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!("BUZZER_PROBE ready mode=descending-chime requested_hz=1974,1566 codes=17,29 first_ms=100 pause_ms=250 second_ms=50 trailing_ms=1000 duration_ms=1400 rounds=2 A=same-pitch B=descending");
    Timer::after_millis(1500).await;
    let candidates: [(&str, &[Element]); 2] = [
        ("A-same-pitch", &DESCENDING_CHIME_A),
        ("B-descending", &DESCENDING_CHIME_B),
    ];
    for round in 1..=2 {
        for (name, score) in candidates {
            // Each score already ends with its 1000 ms rest, so no
            // post-score pause beyond that trailing rest.
            if !play_score_with_pause(device, round, name, score, 0).await {
                return false;
            }
        }
    }
    true
}

async fn play_ring_down(device: &mut Trace) -> bool {
    if code_for_frequency(2700) != Ok(5) {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!("BUZZER_PROBE ready mode=ring-down requested_hz=2700 code=5 A=100ms B=20ms rest_ms=1000 rounds=2");
    Timer::after_millis(1500).await;
    let candidates: [(&str, &[Element]); 2] = [("A-100ms", &RING_DOWN_A), ("B-20ms", &RING_DOWN_B)];
    for round in 1..=2 {
        for (name, score) in candidates {
            // Each score already ends with its 1000 ms rest, so no
            // post-score pause: commanded silence between bursts is
            // exactly 1000 ms, not 1700 ms.
            if !play_score_with_pause(device, round, name, score, 0).await {
                return false;
            }
        }
    }
    true
}

async fn play_manufacturer_melody(device: &mut Trace) -> bool {
    if code_for_frequency(1046) != Ok(58)
        || code_for_frequency(1318) != Ok(40)
        || code_for_frequency(1566) != Ok(29)
        || code_for_frequency(1974) != Ok(17)
    {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!("BUZZER_PROBE ready mode=manufacturer-melody nominal_hz=1046,1318,1566,1974 codes=58,40,29,17 duration_ms=11200 cycles=4 pass=1 candidate=manufacturer-pattern");
    Timer::after_millis(1500).await;
    if !play_score(device, 1, "manufacturer-pattern", &MANUFACTURER_MELODY).await {
        return false;
    }
    true
}

async fn play_pitch_alternation(device: &mut Trace) -> bool {
    if code_for_frequency(900) != Ok(73) || code_for_frequency(1450) != Ok(34) {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!("BUZZER_PROBE ready mode=pitch-alternation nominal_hz=900,1450 codes=73,34 duration_ms=2000 rail=continuous A=100ms-steps B=10ms-steps rounds=2");
    Timer::after_millis(1500).await;
    let candidates: [(&str, &[Element]); 2] = [
        ("A-100ms", &SLOW_PITCH_ALTERNATION),
        ("B-10ms", &FAST_PITCH_ALTERNATION),
    ];
    for round in 1..=2 {
        for (name, score) in candidates {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
    }
    true
}

async fn play_fast_modes(device: &mut Trace, mode: &str) -> bool {
    let primed = inkplate_tempera::buzzer::start_note(&mut device.device, 993).await;
    let off = device.shutdown().await;
    if !matches!(primed, Ok(63)) || off.is_err() {
        console::println!("BUZZER_PROBE failed prime={:?} off={:?}", primed, off);
        let _ = device.shutdown().await;
        return false;
    }
    device.default_gate = true;
    if mode == "gradual-drop" {
        console::println!("BUZZER_PROBE ready mode=gradual-drop primed_code=63 duration_ms=2000 slot_ms=4 A=single-drop-at-1000ms B=gradual-width-mix-75-to-25-percent rounds=2");
    } else if mode == "single-drop-long" {
        console::println!("BUZZER_PROBE ready mode=single-drop-long primed_code=63 duration_ms=2000 slot_ms=4 A=constant-75-percent B=75-to-25-percent drop_at_ms=1000 rounds=2");
    } else if mode == "single-drop" {
        console::println!("BUZZER_PROBE ready mode=single-drop primed_code=63 duration_ms=600 slot_ms=4 A=constant-75-percent B=75-to-25-percent drop_at_ms=300 rounds=2");
    } else if mode == "constant-duty" {
        console::println!("BUZZER_PROBE ready mode=constant-duty primed_code=63 duration_ms=600 slot_ms=4 A=constant-75-percent B=constant-25-percent neither=fade rounds=2");
    } else if mode == "fast-envelope" {
        console::println!("BUZZER_PROBE ready mode=fast-envelope primed_code=63 duration_ms=600 slot_ms=4 A=constant-75-percent B=75,50,25-percent levels_ms=200 rounds=2");
    } else {
        console::println!("BUZZER_PROBE ready mode=fast-cadence primed_code=63 duration_ms=600 duty=75,50,25-percent levels_ms=200 A=8ms-slots B=4ms-slots both=fade rounds=2");
    }
    Timer::after_millis(1500).await;
    let candidates: [(&str, &[Element]); 2] = if mode == "gradual-drop" {
        [("A-single-drop", &LONG_DROP), ("B-gradual", &GRADUAL_DROP)]
    } else if mode == "single-drop-long" {
        [("A-flat-long", &LONG_FLAT), ("B-drop-long", &LONG_DROP)]
    } else if mode == "single-drop" {
        [("A-flat", &FAST_FLAT_ENVELOPE), ("B-drop", &SINGLE_DROP)]
    } else if mode == "constant-duty" {
        [
            ("A-75percent", &FAST_FLAT_ENVELOPE),
            ("B-25percent", &CONSTANT_LOW_DUTY),
        ]
    } else if mode == "fast-envelope" {
        [
            ("A-flat", &FAST_FLAT_ENVELOPE),
            ("B-fade", &FAST_FADING_ENVELOPE),
        ]
    } else {
        [("A-8ms", &GATE_8MS), ("B-4ms", &GATE_4MS)]
    };
    for round in 1..=2 {
        for (name, score) in candidates {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
    }
    true
}

async fn play_cadence(device: &mut Trace) -> bool {
    console::println!("BUZZER_PROBE ready mode=cadence code=63 duration_ms=600 duty=80-to-30-percent levels_ms=100 A=10ms-slots B=20ms-slots both=fade rounds=2");
    Timer::after_millis(1500).await;
    for round in 1..=2 {
        for (name, score) in [("A-10ms", &FAST_CADENCE), ("B-20ms", &SLOW_CADENCE)] {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
    }
    true
}

async fn play_envelope(device: &mut Trace) -> bool {
    console::println!("BUZZER_PROBE ready mode=envelope code=63 duration_ms=600 slot_ms=10 A=pulse-width-8ms B=pulse-width-8-to-3ms rounds=2");
    Timer::after_millis(1500).await;
    for round in 1..=2 {
        for (name, score) in [("A-flat", &FLAT_ENVELOPE), ("B-fade", &FADING_ENVELOPE)] {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
    }
    true
}

async fn play_density(device: &mut Trace) -> bool {
    console::println!("BUZZER_PROBE ready mode=density code=63 duration_ms=600 slot_ms=10 pulse_ms=5 A=constant-density B=falling-density rounds=2");
    Timer::after_millis(1500).await;
    for round in 1..=2 {
        for (name, score) in [("A-flat", &FLAT_DENSITY), ("B-fade", &FADING_DENSITY)] {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
    }
    true
}

async fn play_notifications(device: &mut Trace) -> bool {
    // Generated RTTTL notification audition (listening candidates, not
    // selected presets): every generated catalogue entry once in
    // catalogue order, with the existing 700 ms post-score gap. Normal
    // actuator programming, per-action logging suppressed, no runtime
    // RTTTL and no hand-maintained score copies. The ready lines below
    // are the only pre-audition output: one count line plus one bounded
    // name/duration line per candidate derived from the entry score.
    if buzzer_notifications::COMPILER_OSCILLATOR_R_BASE_OHM
        != inkplate_tempera::buzzer::OSCILLATOR.r_base_ohm
        || buzzer_notifications::COMPILER_OSCILLATOR_R_POT_OHM
            != inkplate_tempera::buzzer::OSCILLATOR.r_pot_ohm
        || buzzer_notifications::COMPILER_OSCILLATOR_CODE_MAX
            != inkplate_tempera::buzzer::OSCILLATOR.code_max
    {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!(
        "BUZZER_PROBE ready mode=notifications candidates={} transpose={:+} rounds=1",
        buzzer_notifications::CATALOGUE.len(),
        buzzer_notifications::COMPILER_TRANSPOSE
    );
    for entry in buzzer_notifications::CATALOGUE.iter() {
        console::println!(
            "BUZZER_PROBE candidate name={} duration_ms={}",
            entry.name,
            buzzer::score::total_duration_ms(entry.score)
        );
    }
    Timer::after_millis(1500).await;
    for entry in buzzer_notifications::CATALOGUE.iter() {
        if !play_score(device, 1, entry.name, entry.score).await {
            return false;
        }
    }
    true
}

async fn play_melody(device: &mut Trace) -> bool {
    // Generated Twinkle opening (prepared demo): the single generated
    // catalogue entry once in catalogue order, with the existing
    // 700 ms post-score gap. Normal actuator programming, per-action
    // logging suppressed, no runtime RTTTL and no hand-maintained
    // score copies. The ready lines below are the only pre-audition
    // output: one count line plus one bounded name/duration line per
    // candidate derived from the entry score.
    if buzzer_melodies::COMPILER_OSCILLATOR_R_BASE_OHM
        != inkplate_tempera::buzzer::OSCILLATOR.r_base_ohm
        || buzzer_melodies::COMPILER_OSCILLATOR_R_POT_OHM
            != inkplate_tempera::buzzer::OSCILLATOR.r_pot_ohm
        || buzzer_melodies::COMPILER_OSCILLATOR_CODE_MAX
            != inkplate_tempera::buzzer::OSCILLATOR.code_max
    {
        console::println!("BUZZER_PROBE failed pitch_mapping_changed");
        return false;
    }
    console::println!(
        "BUZZER_PROBE ready mode=melody candidates={} transpose={:+} rounds=1",
        buzzer_melodies::CATALOGUE.len(),
        buzzer_melodies::COMPILER_TRANSPOSE
    );
    for entry in buzzer_melodies::CATALOGUE.iter() {
        console::println!(
            "BUZZER_PROBE candidate name={} duration_ms={}",
            entry.name,
            buzzer::score::total_duration_ms(entry.score)
        );
    }
    Timer::after_millis(1500).await;
    for entry in buzzer_melodies::CATALOGUE.iter() {
        if !play_score(device, 1, entry.name, entry.score).await {
            return false;
        }
    }
    true
}

async fn play_steady(device: &mut Trace) -> bool {
    console::println!("BUZZER_PROBE ready mode=steady codes=0,63,127 hold_ms=650 rounds=1");
    Timer::after_millis(1500).await;
    for (name, code) in [("raw-0", 0), ("raw-63", 63), ("raw-127", 127)] {
        let score = [
            Element::Note(Note {
                code,
                duration_ms: 650,
            }),
            Element::Rest { duration_ms: 50 },
        ];
        if !play_score(device, 1, name, &score).await {
            return false;
        }
    }
    true
}

async fn play_chime(device: &mut Trace) -> bool {
    console::println!("BUZZER_PROBE ready mode=chime A=2700Hz/100ms B=2700Hz/12ms+900Hz/120ms C=irregular-attack+900Hz/120ms rounds=2");
    Timer::after_millis(1500).await;
    let a = [note(2700, 100), Element::Rest { duration_ms: 50 }];
    let b = [note(2700, 12), note(900, 120)];
    let c = [note(2700, 5), note(1800, 5), note(2350, 5), note(900, 120)];
    for round in 1..=2 {
        for (name, score) in [
            ("A", a.as_slice()),
            ("B", b.as_slice()),
            ("C", c.as_slice()),
        ] {
            if !play_score(device, round, name, score).await {
                return false;
            }
        }
        Timer::after_millis(800).await;
    }
    true
}

#[embassy_executor::task]
async fn play(bus: &'static SharedI2cBus) {
    bus.lock().await.finish_startup();
    static EXPANDER: StaticCell<Mutex<CriticalSectionRawMutex, PcalExpander<SharedI2cDevice>>> =
        StaticCell::new();
    let expander = EXPANDER.init(Mutex::new(PcalExpander::new(shared_i2c_device(bus))));
    let mode = option_env!("BUZZER_PROBE_MODE").unwrap_or("chime");
    let mut device = Trace {
        device: BuzzerDevice::new(shared_i2c_device(bus), expander),
        verbose: !matches!(
            mode,
            "envelope"
                | "density"
                | "cadence"
                | "fast-cadence"
                | "fast-envelope"
                | "constant-duty"
                | "single-drop"
                | "single-drop-long"
                | "gradual-drop"
                | "pitch-alternation"
                | "manufacturer-melody"
                | "ring-down"
                | "descending-chime"
                | "notifications"
                | "melody"
        ),
        default_gate: false,
        max_apply_us: 0,
    };
    if let Err(error) = device.shutdown().await {
        console::println!("BUZZER_PROBE failed init={:?}", error);
        return;
    }
    let completed = match mode {
        "descending-chime" => play_descending_chime(&mut device).await,
        "ring-down" => play_ring_down(&mut device).await,
        "manufacturer-melody" => play_manufacturer_melody(&mut device).await,
        "pitch-alternation" => play_pitch_alternation(&mut device).await,
        "fast-cadence" | "fast-envelope" | "constant-duty" | "single-drop" | "single-drop-long"
        | "gradual-drop" => play_fast_modes(&mut device, mode).await,
        "cadence" => play_cadence(&mut device).await,
        "envelope" => play_envelope(&mut device).await,
        "density" => play_density(&mut device).await,
        "notifications" => play_notifications(&mut device).await,
        "melody" => play_melody(&mut device).await,
        "steady" => play_steady(&mut device).await,
        _ => play_chime(&mut device).await,
    };
    if !completed {
        return;
    }
    let mut i2c = shared_i2c_device(bus);
    let mut latch = [0u8];
    let mut config = [0u8];
    use embedded_hal_async::i2c::I2c as _;
    let readback = i2c.write_read(0x20, &[0x03], &mut latch).await;
    let direction = i2c.write_read(0x20, &[0x07], &mut config).await;
    if readback.is_ok() && direction.is_ok() && latch[0] & 0x10 != 0 && config[0] & 0x10 == 0 {
        console::println!("BUZZER_PROBE complete rail_latch=off output=enabled readback=ok");
    } else {
        console::println!(
            "BUZZER_PROBE failed shutdown_readback latch={:?} config={:?} read={:?} direction={:?}",
            latch,
            config,
            readback,
            direction
        );
    }
}
