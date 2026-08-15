use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

slotmap::new_key_type! {
    /// Generational identifier for a node in the live workspace graph.
    ///
    /// Runtime node identifiers are deliberately not persistent identities.
    pub struct NodeId;
}

/// Process-unique authority domain of one independently constructed engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EngineAuthorityDomainId(u64);

impl EngineAuthorityDomainId {
    pub(crate) fn mint() -> Option<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
        .map(Self)
    }

    /// Returns the process-local diagnostic representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn new_for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Opaque identity of one non-replayable host presentation attempt.
///
/// The serial is process-local and exists only to prevent tickets and adapter
/// sidecars from being spliced across independently prepared host frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostPresentationAttemptId {
    authority_domain: EngineAuthorityDomainId,
    serial: u64,
}

impl HostPresentationAttemptId {
    pub(crate) const fn mint(authority_domain: EngineAuthorityDomainId, serial: u64) -> Self {
        Self {
            authority_domain,
            serial,
        }
    }

    /// Returns the engine authority domain which minted this attempt.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the process-local diagnostic serial.
    #[must_use]
    pub const fn serial(self) -> u64 {
        self.serial
    }
}

macro_rules! stable_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates an identifier from its stable numeric representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the stable numeric representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.get()
            }
        }
    };
}

stable_id!(ItemId, "Stable application identity for dockable content.");
stable_id!(RootId, "Stable identity for a docking graph root.");
stable_id!(
    SurfaceId,
    "Stable identity for a logical presentation surface."
);
stable_id!(
    FloatingPresentationId,
    "Stable identity for a contained floating presentation."
);

/// Durable monotonic counters for core-minted presentation identities.
///
/// A complete dockspace document persists this state alongside its graph and
/// external item map. Bare workspace snapshots deliberately omit it and start
/// a new allocator lineage from their live identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PresentationIdentityFrontier {
    last_surface: u64,
    last_root: u64,
    last_floating: u64,
}

impl PresentationIdentityFrontier {
    pub(crate) const fn empty() -> Self {
        Self::from_counters(0, 0, 0)
    }

    pub(crate) const fn from_counters(
        last_surface: u64,
        last_root: u64,
        last_floating: u64,
    ) -> Self {
        Self {
            last_surface,
            last_root,
            last_floating,
        }
    }

    /// Returns the greatest surface identity observed or retired in this lineage.
    #[must_use]
    pub const fn last_surface(self) -> u64 {
        self.last_surface
    }

    /// Returns the greatest root identity observed or retired in this lineage.
    #[must_use]
    pub const fn last_root(self) -> u64 {
        self.last_root
    }

    /// Returns the greatest contained-floating identity observed or retired.
    #[must_use]
    pub const fn last_floating(self) -> u64 {
        self.last_floating
    }

    pub(crate) fn observe_surface(&mut self, surface: SurfaceId) {
        self.last_surface = self.last_surface.max(surface.get());
    }

    pub(crate) fn observe_root(&mut self, root: RootId) {
        self.last_root = self.last_root.max(root.get());
    }

    pub(crate) fn observe_floating(&mut self, floating: FloatingPresentationId) {
        self.last_floating = self.last_floating.max(floating.get());
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.last_surface = self.last_surface.max(other.last_surface);
        self.last_root = self.last_root.max(other.last_root);
        self.last_floating = self.last_floating.max(other.last_floating);
    }

    pub(crate) fn reserve_surface(&mut self) -> Option<SurfaceId> {
        let next = self.last_surface.checked_add(1)?;
        self.last_surface = next;
        Some(SurfaceId::new(next))
    }

    pub(crate) fn reserve_root(&mut self) -> Option<RootId> {
        let next = self.last_root.checked_add(1)?;
        self.last_root = next;
        Some(RootId::new(next))
    }

    pub(crate) fn reserve_floating(&mut self) -> Option<FloatingPresentationId> {
        let next = self.last_floating.checked_add(1)?;
        self.last_floating = next;
        Some(FloatingPresentationId::new(next))
    }
}

stable_id!(
    StableInputSourceId,
    "Stable diagnostic identity for one producer of reducer inputs."
);

macro_rules! transient_counter_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a typed counter from its runtime representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the runtime counter representation.
            #[must_use]
            #[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances the counter without wrapping.
            #[must_use]
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

transient_counter_id!(
    WorkspaceEpoch,
    "Runtime epoch invalidating state derived from a replaced workspace."
);
transient_counter_id!(
    WorkspaceRevision,
    "Runtime revision invalidating inputs derived from older workspace or policy state."
);
transient_counter_id!(
    InputSequence,
    "Monotonic sequence assigned by the engine's single input writer."
);
transient_counter_id!(
    ReducerTickId,
    "Monotonic causal boundary assigned by the engine reducer."
);
transient_counter_id!(
    NativeCreateSagaId,
    "Monotonic identity of one native tear-off creation saga."
);

/// Core-minted causal position within one [`ReducerTickId`].
///
/// The ordinal is only meaningful together with its reducer tick. It is
/// exposed for observation and trace comparison but is never accepted back as
/// authority by the reducer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ReducerCausalOrdinal(u64);

impl ReducerCausalOrdinal {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the frame-local protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
transient_counter_id!(
    SourceSequence,
    "Monotonic sequence assigned by one session-owned semantic input writer."
);
