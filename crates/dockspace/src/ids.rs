use std::fmt;

slotmap::new_key_type! {
    /// Generational identifier for a node in the live workspace graph.
    ///
    /// Runtime node identifiers are deliberately not persistent identities.
    pub struct NodeId;
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
