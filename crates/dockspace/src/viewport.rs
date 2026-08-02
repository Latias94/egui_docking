//! Stable logical viewport identities and ephemeral native-window bindings.

use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};

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
    CapabilityObservationGeneration,
    "Provider-captured generation of the complete platform-capability roster."
);
monotonic_id!(
    PlatformSnapshotGeneration,
    "Provider-captured generation of one complete atomic platform snapshot."
);
monotonic_id!(
    CapabilityGeneration,
    "Generation of the complete accepted platform-capability snapshot."
);
monotonic_id!(
    InventoryObservationGeneration,
    "Provider-captured generation of the complete native-window inventory."
);
monotonic_id!(
    InventoryGeneration,
    "Generation of the complete accepted native-window inventory."
);
monotonic_id!(
    InputObservationGeneration,
    "Provider-captured generation of one window's pointer-input observation."
);
monotonic_id!(
    CloseObservationGeneration,
    "Provider-captured generation of one exact binding's native-close observation."
);
monotonic_id!(
    PresentationObservationGeneration,
    "Provider-captured generation of one window's presentation observation."
);
monotonic_id!(
    CoordinateObservationGeneration,
    "Provider-captured generation of one exact binding's coordinate observation."
);
monotonic_id!(
    CoordinateGeneration,
    "Generation of acknowledged placement facts for one native-window binding."
);
monotonic_id!(
    WorkAreaToken,
    "Opaque adapter identity of one explicitly selectable desktop work area."
);
monotonic_id!(
    WorkAreaObservationGeneration,
    "Provider-captured generation of the complete desktop work-area roster."
);
monotonic_id!(
    WorkAreaGeneration,
    "Generation of the complete canonical desktop work-area roster."
);
/// Complete identity of one native-window binding.
///
/// `SurfaceId` remains stable across window recreation. The engine domain, epoch,
/// and incarnation make callbacks from another engine, a previous workspace, or
/// a recycled platform token harmless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewportBinding {
    authority_domain: EngineAuthorityDomainId,
    epoch: WorkspaceEpoch,
    surface: SurfaceId,
    token: WindowToken,
    incarnation: WindowIncarnation,
}

impl ViewportBinding {
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        incarnation: WindowIncarnation,
    ) -> Self {
        Self {
            authority_domain,
            epoch,
            surface,
            token,
            incarnation,
        }
    }

    /// Returns the engine authority domain which minted this binding.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
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
        assert_eq!(InventoryGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(
            InputObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(
            CloseObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(
            CoordinateObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(CoordinateGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(WorkAreaToken::new(u64::MAX).checked_next(), None);
        assert_eq!(
            WorkAreaObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(WorkAreaGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(
            CapabilityObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(CapabilityGeneration::new(u64::MAX).checked_next(), None);
        assert_eq!(
            PlatformSnapshotGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(
            InventoryObservationGeneration::new(u64::MAX).checked_next(),
            None
        );
        assert_eq!(WindowIncarnation::new(u64::MAX).checked_next(), None);
        assert_eq!(WindowToken::new(u64::MAX).checked_next(), None);
    }

    #[test]
    fn binding_keeps_every_identity_domain_distinct() {
        let binding = ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(2),
            WorkspaceEpoch::new(3),
            SurfaceId::new(5),
            WindowToken::new(7),
            WindowIncarnation::new(11),
        );

        assert_eq!(
            binding.authority_domain(),
            EngineAuthorityDomainId::new_for_test(2)
        );
        assert_eq!(binding.epoch(), WorkspaceEpoch::new(3));
        assert_eq!(binding.surface(), SurfaceId::new(5));
        assert_eq!(binding.token(), WindowToken::new(7));
        assert_eq!(binding.incarnation(), WindowIncarnation::new(11));
    }
}
