//! Stable logical viewport identities and ephemeral native-window bindings.

use crate::ids::{SurfaceId, WorkspaceEpoch};

macro_rules! monotonic_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates an identity from its protocol representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the protocol representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances without ever permitting ABA through integer wrapping.
            #[must_use]
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

monotonic_id!(
    WindowToken,
    "Opaque adapter token used instead of storing an operating-system handle."
);
monotonic_id!(
    WindowIncarnation,
    "Core-owned incarnation which changes whenever a token is rebound."
);
monotonic_id!(
    CapabilityGeneration,
    "Generation of the complete accepted platform-capability snapshot."
);
monotonic_id!(
    InventoryGeneration,
    "Generation of the complete accepted native-window inventory."
);
monotonic_id!(
    CoordinateGeneration,
    "Generation of acknowledged placement facts for one native-window binding."
);
monotonic_id!(
    RouteGeneration,
    "Generation of a core-resolved authoritative pointer route."
);

/// Complete identity of one native-window binding.
///
/// `SurfaceId` remains stable across window recreation. The epoch and incarnation
/// make callbacks from a previous workspace or recycled platform token harmless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewportBinding {
    epoch: WorkspaceEpoch,
    surface: SurfaceId,
    token: WindowToken,
    incarnation: WindowIncarnation,
}

impl ViewportBinding {
    pub(crate) const fn new(
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        incarnation: WindowIncarnation,
    ) -> Self {
        Self {
            epoch,
            surface,
            token,
            incarnation,
        }
    }

    /// Returns the workspace epoch which owns this binding.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the stable logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the adapter-facing opaque window token.
    #[must_use]
    pub const fn token(self) -> WindowToken {
        self.token
    }

    /// Returns the core-owned incarnation.
    #[must_use]
    pub const fn incarnation(self) -> WindowIncarnation {
        self.incarnation
    }
}

/// Platform ownership semantics of one registered native viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewportRole {
    /// Application root window whose close request must be explicitly cancelled on veto.
    Root,
    /// Docking-owned child window whose roster controls close acceptance and veto.
    Child,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_generations_never_wrap() {
        assert_eq!(
            RouteGeneration::new(0).checked_next(),
            Some(RouteGeneration::new(1))
        );
        assert_eq!(RouteGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(InventoryGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(CoordinateGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(CapabilityGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(WindowIncarnation::new(u64::MAX).checked_next(), None);
        assert_eq!(WindowToken::new(u64::MAX).checked_next(), None);
    }

    #[test]
    fn binding_keeps_every_identity_domain_distinct() {
        let binding = ViewportBinding::new(
            WorkspaceEpoch::new(3),
            SurfaceId::new(5),
            WindowToken::new(7),
            WindowIncarnation::new(11),
        );

        assert_eq!(binding.epoch(), WorkspaceEpoch::new(3));
        assert_eq!(binding.surface(), SurfaceId::new(5));
        assert_eq!(binding.token(), WindowToken::new(7));
        assert_eq!(binding.incarnation(), WindowIncarnation::new(11));
    }
}
