//! Layout projection, scene compilation and exact output observation.
//!
//! Compiled scenes describe what can be painted. Hit manifests and the observation
//! ledger retain which exact output was presented and can authorize later input.
//! These records do not mutate durable topology or own the engine's publication.

pub(crate) mod compiler;
pub(crate) mod config;
pub(crate) mod hit;
pub(crate) mod layout;
pub(crate) mod manifest;
pub(crate) mod observation;
pub(crate) mod scene;
pub(crate) mod tab_strip;
