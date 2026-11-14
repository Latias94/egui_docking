//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.
//!
//! Raw reducer, scene, provider, and ingress protocols are implementation details,
//! even when every crate feature is enabled:
//!
//! ```compile_fail
//! use dockspace::engine::DockEngine;
//! ```
//!
//! ```compile_fail
//! use dockspace::graph::Workspace;
//! ```
//!
//! ```compile_fail
//! use dockspace::ids::NodeId;
//! ```
//!
//! ```compile_fail
//! use dockspace::presentation::scene::SurfaceSceneStamp;
//! ```
//!
//! ```compile_fail
//! use dockspace::platform::provider::PlatformObservationLease;
//! ```
//!
//! ```compile_fail
//! use dockspace::persistence::snapshot::WorkspaceSnapshot;
//! ```
//!
//! ```compile_fail
//! use dockspace::close::plan::ClosePlanPhase;
//! ```

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as dockspace;

pub mod close;
mod engine;
pub mod geometry;
mod graph;
mod ids;
mod input;
mod interaction;
pub mod model;
#[cfg(feature = "motion")]
pub mod motion;
#[cfg(any(feature = "serde", test))]
mod persistence;
mod platform;
pub mod policy;
mod presentation;
pub mod runtime;

#[cfg(test)]
mod behavior_tests;
