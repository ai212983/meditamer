//! Shared single-oscillator buzzer sequencer.
//!
//! One host-testable sequencer feeds the firmware runner and the WAV
//! previewer with the same timestamped requested actions. Scores are borrowed
//! slices of notes and rests; adjacent notes change pitch without
//! interrupting power, and rests power the rail off so a following note
//! re-applies its code after power-up.
//!
//! Module map:
//!
//! - [`model`]: integer-only nominal astable-oscillator pitch math shared by
//!   the board mapping, host tests, and the previewer. Board-specific
//!   resistor/capacitor values stay in the board crate.
//! - [`score`]: borrowed scores plus the event-deadline planner. No fixed
//!   tick: the planner emits one action per note/rest boundary.
//! - [`runner`]: asynchronous, fallible execution contract. Absolute
//!   deadlines, expired events are skipped rather than burst through,
//!   failure halts playback, and end/stop/cancellation always run an awaited
//!   shutdown path.
//! - [`diag`]: bounded diagnostic operation model (raw codes, powered pitch
//!   changes, bursts, power cycles, sweeps, short scores, stop) with
//!   validation and compact trace lines.
#![no_std]

pub mod diag;
pub mod model;
pub mod runner;
pub mod score;
