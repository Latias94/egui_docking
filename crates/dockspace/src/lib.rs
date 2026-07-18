//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.

#![forbid(unsafe_code)]

pub mod canonical;
pub mod geometry;
pub mod graph;
pub mod ids;
pub mod layout;
pub mod policy;
pub mod validation;

/// Snapshot and conformance fixture schema version implemented by this crate.
pub const CONTRACT_VERSION: u32 = 1;
