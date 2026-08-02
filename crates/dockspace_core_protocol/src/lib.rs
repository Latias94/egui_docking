//! Typed executable traces for the `dockspace` core protocol.
//!
//! This workspace-private crate owns fixture decoding, canonicalization, and a
//! [`CoreProtocolHarness`] that directly owns a [`dockspace::engine::DockEngine`].
//! Pointer interaction enters only through ordered [`HostFrameEvent`] values
//! and receives core-minted candidate receipts. These traces prove core
//! transport and causality, not an independent hit compiler or UI-adapter
//! conformance; they never accept renderer intents or final pointer snapshots.

#![forbid(unsafe_code)]

mod core_protocol_harness;
mod core_protocol_trace;

pub use core_protocol_harness::{
    CoreProtocolHarness, CoreProtocolReplayReport, replay_core_protocol_trace_suite,
};
pub use core_protocol_trace::*;
