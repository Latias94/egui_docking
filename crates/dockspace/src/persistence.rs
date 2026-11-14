//! Durable documents, item identity mapping and platform placement preferences.
//!
//! Restores construct a validated candidate before the engine publishes it;
//! snapshot-local node identities never become runtime graph identities.

#[cfg(feature = "serde")]
pub(crate) mod document;
pub(crate) mod item_key;
#[cfg(feature = "serde")]
pub(crate) mod placement;
#[cfg(feature = "serde")]
pub(crate) mod snapshot;
