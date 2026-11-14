//! Ordered input facts and the exact presented evidence used to reduce them.
//!
//! Ingress preserves provider order, the pointer journal tracks stream lifetimes,
//! and receiver receipts prove framework delivery against a frozen output.
//! Gesture state and publication remain owned by the interaction and engine modules.

pub(crate) mod ingress;
pub(crate) mod intent;
pub(crate) mod journal;
pub(crate) mod presentation;
pub(crate) mod receiver;
