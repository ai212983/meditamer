//! Product-neutral typed observation subscriptions (ADR-0018;
//! `docs/architecture/0018-typed-observation-subscriptions.md`).
//!
//! This crate owns pure subscription, freshness, and delivery logic over
//! provider-defined field and snapshot types. It does not know about SHTC3, BME688, BQ27441, or any board;
//! a provider implements [`field::FieldMask`] for its own field bitmask and
//! plugs its snapshot type into [`state::ObservationState`].
//!
//! Module map:
//!
//! - Phase 1: [`ids`], [`policy`], [`time`], [`field`], [`subscription`],
//!   [`demand`], [`schedule`], [`state`], [`delivery`], [`observe_now`] --
//!   pure logic, deterministic on host.
//! - Phase 2: [`runtime`] -- the generic Embassy provider loop (demand
//!   `Watch`, observe-now `Channel`, state `Watch`, and the reactive step
//!   that drives a caller-supplied acquisition driver).
#![no_std]

pub mod delivery;
pub mod demand;
pub mod field;
pub mod fixture;
pub mod ids;
pub mod ingress;
pub mod observe_now;
pub mod periodic;
pub mod policy;
pub mod runtime;
pub mod schedule;
pub mod state;
pub mod subscription;
pub mod time;
