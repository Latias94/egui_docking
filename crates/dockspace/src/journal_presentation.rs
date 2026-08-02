//! Immutable presentation projections used while reducing one pointer journal.
//!
//! A journal can contain several causally ordered edges. The first may mutate
//! workspace selection or contained z-order, which invalidates the current
//! candidate scene. Later edges must nevertheless be interpreted against the
//! output that was actually presented when their receipts were collected.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::ids::SurfaceId;
use crate::pointer_journal::PointerInputLease;
use crate::pointer_receiver::{
    PointerReceiverObservation, PointerReceiverProbeReceipt, ValidatedPointerReceiverReceiptBatch,
};
use crate::presentation_hit::PresentationHitManifest;
use crate::presentation_observation::{PresentedSurfaceAuthority, SurfacePresentationOutputTicket};
use crate::scene::{PopupInteractionGateRevision, PresentationPlan, SurfacePlanScene};

/// One exact output projection frozen for a pointer-journal reduction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JournalSurfacePresentation {
    output: SurfacePlanScene,
    authority: PresentedSurfaceAuthority,
    popup_gate_revision: PopupInteractionGateRevision,
}

impl JournalSurfacePresentation {
    pub(crate) fn from_interaction(
        projection: crate::scene::SurfaceInteractionProjection<'_>,
    ) -> Self {
        Self {
            output: projection.output().clone(),
            authority: projection.authority(),
            popup_gate_revision: projection.popup_gate_revision(),
        }
    }

    pub(crate) const fn output_ticket(&self) -> SurfacePresentationOutputTicket {
        self.output.output_ticket()
    }

    pub(crate) const fn authority(&self) -> PresentedSurfaceAuthority {
        self.authority
    }

    pub(crate) const fn popup_gate_revision(&self) -> PopupInteractionGateRevision {
        self.popup_gate_revision
    }

    pub(crate) const fn surface(&self) -> SurfaceId {
        self.output.output_ticket().surface()
    }

    pub(crate) fn plan(&self) -> &PresentationPlan {
        self.output.plan()
    }

    pub(crate) fn hit_manifest(&self) -> &PresentationHitManifest {
        self.output.hit_manifest()
    }

    pub(crate) const fn coordinate_capture(&self) -> crate::scene::SurfaceCoordinateCapture {
        self.output.coordinate_capture()
    }

    pub(crate) fn scene(&self) -> crate::scene::SurfaceSceneStamp {
        self.output.stamp()
    }

    pub(crate) fn projection(&self) -> crate::scene::SurfaceInteractionProjection<'_> {
        crate::scene::SurfaceInteractionProjection::from_frozen_parts(
            &self.output,
            self.authority,
            self.popup_gate_revision,
        )
    }
}

/// Exact set of presented outputs referenced by validated journal receipts.
///
/// The snapshot deliberately holds only outputs actually named by this journal,
/// rather than cloning the full scene roster. `SurfacePlanScene` itself shares
/// its plan and manifest through `Arc`, so this remains bounded by the journal
/// fan-out instead of surface count or pane count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JournalPresentationSnapshot {
    provider: PointerInputLease,
    surfaces: BTreeMap<SurfaceId, JournalSurfacePresentation>,
}

impl JournalPresentationSnapshot {
    pub(crate) fn from_validated_receipts(
        provider: PointerInputLease,
        presented: &BTreeMap<SurfaceId, JournalSurfacePresentation>,
        receipts: &ValidatedPointerReceiverReceiptBatch,
    ) -> Result<Self, JournalPresentationSnapshotError> {
        let mut surfaces = BTreeMap::new();
        for receipt in receipts.receipts() {
            let PointerReceiverObservation::Presented(observation) = receipt.observation() else {
                continue;
            };
            for probe in observation.probes() {
                let binding = match probe {
                    PointerReceiverProbeReceipt::Delivery(delivery) => {
                        match (delivery.output(), delivery.authority()) {
                            (Some(output), Some(authority)) => Some((output, authority)),
                            (None, None) => None,
                            _ => {
                                return Err(
                                    JournalPresentationSnapshotError::ReceiptBindingInvariant {
                                        sequence: receipt.candidate().sequence(),
                                    },
                                );
                            }
                        }
                    }
                    PointerReceiverProbeReceipt::HoverHit(hover) => {
                        match (hover.output(), hover.authority()) {
                            (Some(output), Some(authority)) => Some((output, authority)),
                            (None, None) => None,
                            _ => {
                                return Err(
                                    JournalPresentationSnapshotError::ReceiptBindingInvariant {
                                        sequence: receipt.candidate().sequence(),
                                    },
                                );
                            }
                        }
                    }
                };
                let Some((output, authority)) = binding else {
                    continue;
                };
                if let Some(binding) = provider
                    .scope()
                    .surface_local()
                    .and_then(|scope| scope.endpoint().native_binding())
                    && (output.surface() != binding.surface()
                        || authority.surface() != binding.surface()
                        || authority.binding() != Some(binding))
                {
                    return Err(
                        JournalPresentationSnapshotError::SurfaceLocalBindingMismatch {
                            expected: binding,
                            output,
                            authority,
                            sequence: receipt.candidate().sequence(),
                        },
                    );
                }
                let surface = output.surface();
                let presentation = presented.get(&surface).ok_or(
                    JournalPresentationSnapshotError::InteractionProjectionUnavailable {
                        surface,
                        sequence: receipt.candidate().sequence(),
                    },
                )?;
                if presentation.output_ticket() != output
                    || !presentation
                        .authority()
                        .same_interaction_semantics(authority)
                {
                    return Err(JournalPresentationSnapshotError::ReceiptOutputNotCurrent {
                        surface,
                        sequence: receipt.candidate().sequence(),
                    });
                }
                let presentation = presentation.clone();
                if let Some(existing) = surfaces.insert(surface, presentation.clone())
                    && existing != presentation
                {
                    return Err(JournalPresentationSnapshotError::ConflictingReceiptOutput {
                        surface,
                        sequence: receipt.candidate().sequence(),
                    });
                }
            }
        }
        Ok(Self { provider, surfaces })
    }

    pub(crate) const fn provider(&self) -> PointerInputLease {
        self.provider
    }

    pub(crate) fn presentation(
        &self,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
    ) -> Result<&JournalSurfacePresentation, JournalPresentationSnapshotError> {
        let surface = output.surface();
        let Some(presentation) = self.surfaces.get(&surface) else {
            return Err(JournalPresentationSnapshotError::ReceiptOutputAbsent { surface });
        };
        if presentation.output_ticket() != output || presentation.authority() != authority {
            return Err(JournalPresentationSnapshotError::ReceiptOutputMismatch { surface });
        }
        Ok(presentation)
    }
}

/// Failure while freezing the exact output geometry referenced by a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum JournalPresentationSnapshotError {
    #[error("pointer receipt for edge {sequence} contains a non-atomic output binding")]
    ReceiptBindingInvariant {
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    #[error(
        "pointer receipt for edge {sequence} names surface {surface} without a current interaction projection"
    )]
    InteractionProjectionUnavailable {
        surface: SurfaceId,
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    #[error(
        "pointer receipt for edge {sequence} no longer matches the current interaction output on surface {surface}"
    )]
    ReceiptOutputNotCurrent {
        surface: SurfaceId,
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    #[error(
        "pointer receipt for edge {sequence} does not belong to the surface-local native binding {expected:?}"
    )]
    SurfaceLocalBindingMismatch {
        expected: crate::viewport::ViewportBinding,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    #[error(
        "pointer journal receipts conflict about the presented output on surface {surface} at edge {sequence}"
    )]
    ConflictingReceiptOutput {
        surface: SurfaceId,
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    #[error(
        "pointer receipt names an output absent from the frozen journal snapshot on surface {surface}"
    )]
    ReceiptOutputAbsent { surface: SurfaceId },
    #[error("pointer receipt output differs from the frozen journal snapshot on surface {surface}")]
    ReceiptOutputMismatch { surface: SurfaceId },
}
