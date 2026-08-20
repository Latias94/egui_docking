//! Renderer-neutral black-box conformance executor.
//!
//! The executor intentionally depends only on the public runtime and model
//! contracts. It never owns a core engine and never derives receiver facts by
//! asking the core hit resolver to choose a target.

#[doc(hidden)]
pub mod scenario;

#[cfg(test)]
mod tests;
