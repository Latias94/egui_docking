//! Linear ownership ledger for retained native staging resources.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use thiserror::Error;

use crate::ids::NativeCreateSagaId;
use crate::presentation_observation::{NativeStagingResourceDescriptor, NativeStagingResourceId};
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::ViewportBinding;

/// The sole lifecycle owner currently responsible for a retained staging resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeStagingResourceOwner {
    NativeCreateSaga(NativeCreateSagaId),
    NativeCreateFirstLive(ViewportBinding),
    SurfaceRecovery {
        obligation: SurfaceRecoveryObligationId,
        binding: ViewportBinding,
    },
    RecoveryReplacementFirstLive {
        obligation: SurfaceRecoveryObligationId,
        binding: ViewportBinding,
    },
    BindingCleanup(ViewportBinding),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeStagingResourceEntry {
    descriptor: NativeStagingResourceDescriptor,
    owner: NativeStagingResourceOwner,
}

/// Owns retained descriptors and enforces one exact lifecycle owner per resource.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NativeStagingResourceLedger {
    entries: BTreeMap<NativeStagingResourceId, NativeStagingResourceEntry>,
}

/// Rejection from a checked retained-resource lifecycle operation.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub(super) enum NativeStagingResourceLedgerError {
    #[error("native staging resource {resource:?} already exists")]
    DuplicateResource { resource: NativeStagingResourceId },
    #[error("native staging resource {resource:?} does not exist")]
    MissingResource { resource: NativeStagingResourceId },
    #[error(
        "native staging resource {resource:?} is owned by {actual:?}, not expected owner {expected:?}"
    )]
    OwnerMismatch {
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
        actual: NativeStagingResourceOwner,
    },
}

/// A mismatch between retained resources and their durable lifecycle references.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub(super) enum NativeStagingResourceConservationError {
    #[error(
        "native staging resource {resource:?} is referenced twice by {first:?} and {duplicate:?}"
    )]
    DuplicateReference {
        resource: NativeStagingResourceId,
        first: NativeStagingResourceOwner,
        duplicate: NativeStagingResourceOwner,
    },
    #[error("referenced native staging resource {resource:?} does not exist")]
    ReferencedResourceMissing {
        resource: NativeStagingResourceId,
        owner: NativeStagingResourceOwner,
    },
    #[error(
        "native staging resource {resource:?} is owned by {actual:?}, not durable reference {expected:?}"
    )]
    ReferenceOwnerMismatch {
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
        actual: NativeStagingResourceOwner,
    },
    #[error("native staging resource {resource:?} owned by {owner:?} has no durable reference")]
    UnreferencedResource {
        resource: NativeStagingResourceId,
        owner: NativeStagingResourceOwner,
    },
}

impl NativeStagingResourceLedger {
    /// Inserts a newly retained descriptor under its first and only owner.
    pub(super) fn insert(
        &mut self,
        descriptor: NativeStagingResourceDescriptor,
        owner: NativeStagingResourceOwner,
    ) -> Result<(), NativeStagingResourceLedgerError> {
        let resource = descriptor.id();
        match self.entries.entry(resource) {
            Entry::Vacant(entry) => {
                entry.insert(NativeStagingResourceEntry { descriptor, owner });
                Ok(())
            }
            Entry::Occupied(_) => {
                Err(NativeStagingResourceLedgerError::DuplicateResource { resource })
            }
        }
    }

    /// Transfers responsibility from the exact current owner to one successor.
    pub(super) fn transition(
        &mut self,
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
        next: NativeStagingResourceOwner,
    ) -> Result<(), NativeStagingResourceLedgerError> {
        let entry = self
            .entries
            .get_mut(&resource)
            .ok_or(NativeStagingResourceLedgerError::MissingResource { resource })?;
        if entry.owner != expected {
            return Err(NativeStagingResourceLedgerError::OwnerMismatch {
                resource,
                expected,
                actual: entry.owner,
            });
        }
        entry.owner = next;
        Ok(())
    }

    /// Releases a resource only when the caller proves its exact current owner.
    pub(super) fn release(
        &mut self,
        resource: NativeStagingResourceId,
        expected: NativeStagingResourceOwner,
    ) -> Result<NativeStagingResourceDescriptor, NativeStagingResourceLedgerError> {
        let actual = self
            .owner(resource)
            .ok_or(NativeStagingResourceLedgerError::MissingResource { resource })?;
        if actual != expected {
            return Err(NativeStagingResourceLedgerError::OwnerMismatch {
                resource,
                expected,
                actual,
            });
        }
        self.entries
            .remove(&resource)
            .map(|entry| entry.descriptor)
            .ok_or(NativeStagingResourceLedgerError::MissingResource { resource })
    }

    /// Returns the retained descriptor for one exact resource identity.
    #[must_use]
    pub(super) fn get(
        &self,
        resource: NativeStagingResourceId,
    ) -> Option<&NativeStagingResourceDescriptor> {
        self.entries.get(&resource).map(|entry| &entry.descriptor)
    }

    /// Iterates every retained descriptor in stable resource-identity order.
    #[must_use]
    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> {
        self.entries.values().map(|entry| &entry.descriptor)
    }

    /// Returns the sole lifecycle owner of one retained resource.
    #[must_use]
    pub(super) fn owner(
        &self,
        resource: NativeStagingResourceId,
    ) -> Option<NativeStagingResourceOwner> {
        self.entries.get(&resource).map(|entry| entry.owner)
    }

    /// Proves that every retained resource has exactly one durable owner reference.
    pub(super) fn validate_conservation(
        &self,
        references: impl IntoIterator<Item = (NativeStagingResourceId, NativeStagingResourceOwner)>,
    ) -> Result<(), NativeStagingResourceConservationError> {
        let mut expected = BTreeMap::new();
        for (resource, owner) in references {
            if let Some(first) = expected.insert(resource, owner) {
                return Err(NativeStagingResourceConservationError::DuplicateReference {
                    resource,
                    first,
                    duplicate: owner,
                });
            }
        }

        for (resource, owner) in &expected {
            let entry = self.entries.get(resource).ok_or(
                NativeStagingResourceConservationError::ReferencedResourceMissing {
                    resource: *resource,
                    owner: *owner,
                },
            )?;
            if entry.owner != *owner {
                return Err(
                    NativeStagingResourceConservationError::ReferenceOwnerMismatch {
                        resource: *resource,
                        expected: *owner,
                        actual: entry.owner,
                    },
                );
            }
        }

        for (resource, entry) in &self.entries {
            if !expected.contains_key(resource) {
                return Err(
                    NativeStagingResourceConservationError::UnreferencedResource {
                        resource: *resource,
                        owner: entry.owner,
                    },
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::command::MovePayload;
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{EngineAuthorityDomainId, ItemId, RootId, SurfaceId, WorkspaceEpoch};
    use crate::policy::PolicyRevision;
    use crate::presentation_config::PresentationConfigRevision;
    use crate::presentation_observation::{
        PresentationOutputSerial, PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
    };
    use crate::scene::SurfaceSceneStamp;
    use crate::scene_manifest::{
        SurfaceMeasurementTicket, SurfaceRequirementRevision, SurfaceSceneRevision,
    };
    use crate::viewport::{CoordinateGeneration, WindowIncarnation, WindowToken};

    const AUTHORITY_DOMAIN: EngineAuthorityDomainId = EngineAuthorityDomainId::new_for_test(31);

    fn binding(surface: u64, token: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            AUTHORITY_DOMAIN,
            WorkspaceEpoch::new(1),
            SurfaceId::new(surface),
            WindowToken::new(token),
            WindowIncarnation::new(incarnation),
        )
    }

    fn descriptor(saga: NativeCreateSagaId) -> NativeStagingResourceDescriptor {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let item = ItemId::new(3);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("test workspace must be valid");
        let payload = MovePayload::Item(
            workspace
                .capture_item_source(root, tabs, item)
                .expect("test item source must be current"),
        );

        let measurement = SurfaceMeasurementTicket::new(
            AUTHORITY_DOMAIN,
            WorkspaceEpoch::new(1),
            PresentationConfigRevision::new(1),
            PolicyRevision::new(1),
            SurfaceRequirementRevision::new(1),
            surface,
        );
        let output = SurfacePresentationOutputTicket::mint(
            AUTHORITY_DOMAIN,
            PresentationOutputSerial::new_for_test(1),
            SurfaceSceneStamp::new(measurement, SurfaceSceneRevision::new(1)),
        );
        let presentation =
            PresentedSurfaceAuthority::mint_observed_for_test(output, CoordinateGeneration::new(1));
        let resource = NativeStagingResourceId::mint(AUTHORITY_DOMAIN, saga);
        NativeStagingResourceDescriptor::mint(resource, presentation, payload)
            .expect("test descriptor must share the engine authority domain")
    }

    #[test]
    fn legal_lifecycle_has_one_owner_until_release() {
        let saga = NativeCreateSagaId::new(1);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let original = descriptor.clone();
        let first_binding = binding(10, 20, 1);
        let replacement_binding = binding(10, 21, 2);
        let obligation = SurfaceRecoveryObligationId::new_for_test(30);
        let mut ledger = NativeStagingResourceLedger::default();

        let saga_owner = NativeStagingResourceOwner::NativeCreateSaga(saga);
        ledger
            .insert(descriptor, saga_owner)
            .expect("new resource must insert");
        assert_eq!(ledger.get(resource), Some(&original));
        assert_eq!(ledger.iter().count(), 1);
        assert_eq!(ledger.owner(resource), Some(saga_owner));

        let first_live = NativeStagingResourceOwner::NativeCreateFirstLive(first_binding);
        ledger
            .transition(resource, saga_owner, first_live)
            .expect("native create must enter first-live ownership");
        let recovery = NativeStagingResourceOwner::SurfaceRecovery {
            obligation,
            binding: first_binding,
        };
        ledger
            .transition(resource, first_live, recovery)
            .expect("first-live destruction must transfer to recovery");
        let replacement = NativeStagingResourceOwner::RecoveryReplacementFirstLive {
            obligation,
            binding: replacement_binding,
        };
        ledger
            .transition(resource, recovery, replacement)
            .expect("recovery replacement must acquire first-live ownership");
        let retirement = NativeStagingResourceOwner::BindingCleanup(replacement_binding);
        ledger
            .transition(resource, replacement, retirement)
            .expect("replacement cleanup must transfer to binding retirement");

        assert_eq!(ledger.owner(resource), Some(retirement));
        assert_eq!(
            ledger
                .release(resource, retirement)
                .expect("exact terminal owner must release"),
            original
        );
        assert_eq!(ledger.get(resource), None);
        assert_eq!(ledger.iter().count(), 0);
        assert_eq!(ledger.owner(resource), None);
    }

    #[test]
    fn wrong_owner_cannot_transition_or_release() {
        let saga = NativeCreateSagaId::new(2);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let actual = NativeStagingResourceOwner::NativeCreateSaga(saga);
        let wrong = NativeStagingResourceOwner::NativeCreateFirstLive(binding(10, 20, 1));
        let next = NativeStagingResourceOwner::BindingCleanup(binding(10, 20, 1));
        let mut ledger = NativeStagingResourceLedger::default();
        ledger
            .insert(descriptor, actual)
            .expect("new resource must insert");

        assert_eq!(
            ledger.transition(resource, wrong, next),
            Err(NativeStagingResourceLedgerError::OwnerMismatch {
                resource,
                expected: wrong,
                actual,
            })
        );
        assert_eq!(
            ledger.release(resource, wrong),
            Err(NativeStagingResourceLedgerError::OwnerMismatch {
                resource,
                expected: wrong,
                actual,
            })
        );
        assert_eq!(ledger.owner(resource), Some(actual));
    }

    #[test]
    fn duplicate_insert_preserves_the_original_entry() {
        let saga = NativeCreateSagaId::new(3);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let original = descriptor.clone();
        let original_owner = NativeStagingResourceOwner::NativeCreateSaga(saga);
        let replacement_owner =
            NativeStagingResourceOwner::NativeCreateFirstLive(binding(10, 20, 1));
        let mut ledger = NativeStagingResourceLedger::default();
        ledger
            .insert(descriptor, original_owner)
            .expect("new resource must insert");

        assert_eq!(
            ledger.insert(original.clone(), replacement_owner),
            Err(NativeStagingResourceLedgerError::DuplicateResource { resource })
        );
        assert_eq!(ledger.get(resource), Some(&original));
        assert_eq!(ledger.owner(resource), Some(original_owner));
    }

    #[test]
    fn released_resource_cannot_be_released_twice() {
        let saga = NativeCreateSagaId::new(4);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let owner = NativeStagingResourceOwner::NativeCreateSaga(saga);
        let mut ledger = NativeStagingResourceLedger::default();
        ledger
            .insert(descriptor, owner)
            .expect("new resource must insert");
        ledger
            .release(resource, owner)
            .expect("exact owner must release once");

        assert_eq!(
            ledger.release(resource, owner),
            Err(NativeStagingResourceLedgerError::MissingResource { resource })
        );
    }

    #[test]
    fn conservation_requires_one_exact_reference_per_resource() {
        let saga = NativeCreateSagaId::new(5);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let owner = NativeStagingResourceOwner::NativeCreateSaga(saga);
        let mut ledger = NativeStagingResourceLedger::default();
        ledger
            .insert(descriptor, owner)
            .expect("new resource must insert");

        assert_eq!(ledger.validate_conservation([(resource, owner)]), Ok(()));
        assert_eq!(
            ledger.validate_conservation([(resource, owner), (resource, owner)]),
            Err(NativeStagingResourceConservationError::DuplicateReference {
                resource,
                first: owner,
                duplicate: owner,
            })
        );
        assert_eq!(
            ledger.validate_conservation([]),
            Err(NativeStagingResourceConservationError::UnreferencedResource { resource, owner })
        );
    }

    #[test]
    fn conservation_rejects_missing_and_mismatched_resources() {
        let saga = NativeCreateSagaId::new(6);
        let descriptor = descriptor(saga);
        let resource = descriptor.id();
        let owner = NativeStagingResourceOwner::NativeCreateSaga(saga);
        let replacement = NativeStagingResourceOwner::NativeCreateFirstLive(binding(10, 20, 1));
        let mut ledger = NativeStagingResourceLedger::default();

        assert_eq!(
            ledger.validate_conservation([(resource, owner)]),
            Err(
                NativeStagingResourceConservationError::ReferencedResourceMissing {
                    resource,
                    owner,
                }
            )
        );
        ledger
            .insert(descriptor, owner)
            .expect("new resource must insert");
        assert_eq!(
            ledger.validate_conservation([(resource, replacement)]),
            Err(
                NativeStagingResourceConservationError::ReferenceOwnerMismatch {
                    resource,
                    expected: replacement,
                    actual: owner,
                }
            )
        );
    }
}
