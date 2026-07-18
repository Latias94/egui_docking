//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.

#![forbid(unsafe_code)]

pub mod canonical;
pub mod command;
pub mod drop_resolver;
pub mod drop_target;
pub mod engine;
pub mod error;
pub mod event;
pub mod geometry;
pub mod graph;
pub mod hit_region;
pub mod ids;
pub mod intent;
pub mod interaction;
pub mod layout;
mod operation;
#[cfg(feature = "serde")]
pub mod persistence;
pub mod policy;
pub mod scene;
pub mod transaction;
pub mod transition;
pub mod validation;
mod workspace;

/// Snapshot and conformance fixture schema version implemented by this crate.
pub const CONTRACT_VERSION: u32 = 1;
