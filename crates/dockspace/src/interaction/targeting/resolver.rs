//! Deterministic resolution of authoritative drop locations.

mod preview;

use std::sync::Arc;

use crate::engine::transition::WorkspaceVersion;
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::graph::Workspace;
use crate::graph::command::{
    DockTarget, MovePayload, NodeSource, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use crate::graph::error::{CommandError, TransactionError};
use crate::graph::transaction::TransactionReport;
#[cfg(test)]
use crate::graph::transaction::WorkspaceTransaction;
use crate::graph::workspace::RootPresentationOwner;
use crate::graph::workspace::WorkspaceIndex;
use crate::ids::SurfaceId;
#[cfg(test)]
use crate::ids::WorkspaceRevision;
#[cfg(test)]
use crate::ids::{FloatingPresentationId, RootId};
use crate::input::intent::SurfaceBackgroundRootOffer;
use crate::interaction::DragSessionId;
use crate::interaction::targeting::guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideScope, DropGuideSlot,
};
use crate::interaction::targeting::targets::{
    DropSourceSuppression, DropTargetAvailability, DropTargetId, DropTargetKind,
    DropTargetLocation, DropTargetRecord, DropTargetUnavailable, DropVisual, SceneLayerKey,
    SurfaceBackground,
};
use crate::interaction::targeting::{
    DirectTargetCandidate, DirectTargetProvenance, TargetCandidate, TargetObservation,
};
use crate::policy::{DockPolicySnapshot, PolicyRejection};
use crate::presentation::hit::region::HitRegion;
use crate::presentation::manifest::PolicyRevision;
use crate::presentation::scene::{PresentationLayoutFacts, PresentationPlan, SurfaceSceneStamp};
#[cfg(test)]
use crate::presentation::scene::{SurfaceScene, SurfaceSceneSet};
use thiserror::Error;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropTargeting {
    Disabled,
    Explicit,
    Magnetic,
}

/// Queries both the exact drop outcome and the complete visible guide affordance.
///
/// A scene with explicit guide clusters uses exact, half-open guide-button and
/// tab-gap hits before its core-owned magnetic target field. Direct hits outrank
/// the active outer-root band, which outranks the deepest pane's center or
/// normalized nearest-edge basin. An ineligible winner never falls through to
/// another target. Guide clusters remain painting affordances for the same
/// structural targets selected by the larger field.
///
/// Every selected guide target is checked against scene availability and one
/// immutable payload-policy context. Only the unique geometric winner performs
/// a complete U3 transaction preflight before the owned affordance is returned.
/// If removing the frozen payload consumes its complete source root, that root's
/// targets and contained presentation are suppressed before occlusion and hit
/// resolution so the presentation being removed cannot hide its destination.
/// The result therefore remains valid for painting after the borrowed scene has
/// been released, while preview acknowledgement stays solely in
/// [`DropResolution::Resolved`].
///
/// # Errors
///
/// Returns [`DropResolutionError`] if read-only eligibility or the unique
/// winner's transaction preflight exposes an unexpected failure.
#[allow(
    clippy::too_many_arguments,
    reason = "the resolver binds every authoritative drag fact without an ambient context"
)]
#[cfg(test)]
pub fn resolve_drop(
    scene: &SurfaceSceneSet,
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    session: DragSessionId,
    source: MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    surface: SurfaceId,
    point: LogicalPoint,
) -> Result<DropQuery, DropResolutionError> {
    let Some(surface_scene) = scene.surface(surface) else {
        return Ok(DropQuery::new(
            DropResolution::Unavailable(UnavailableDrop::new(
                None,
                surface,
                DropSurfaceUnavailable::MissingSurface,
            )),
            None,
            TargetObservation::Unavailable,
        ));
    };
    let Some(presented) = scene.ready_surface(surface) else {
        let reason = match surface_scene {
            SurfaceScene::Stale(_) => DropSurfaceUnavailable::Stale,
            SurfaceScene::Bootstrap(_) => DropSurfaceUnavailable::Bootstrap,
            SurfaceScene::Ready(_) => DropSurfaceUnavailable::PendingPaint,
        };
        return Ok(DropQuery::new(
            DropResolution::Unavailable(UnavailableDrop::new(
                Some(surface_scene.stamp()),
                surface,
                reason,
            )),
            None,
            TargetObservation::Unavailable,
        ));
    };
    let workspace_version = WorkspaceVersion::new(
        presented.stamp().requirement().workspace_epoch(),
        WorkspaceRevision::default(),
    );
    let workspace_index =
        WorkspaceIndex::build(workspace, workspace_version).map_err(|source| {
            DropResolutionError::UnexpectedPrevalidation(TransactionError::Command {
                index: 0,
                source,
            })
        })?;
    let source_layout_facts = source_presentation_layout_facts(scene, workspace, &source);
    resolve_presented_drop(
        presented.stamp(),
        presented.plan(),
        source_layout_facts,
        workspace,
        workspace_version,
        &workspace_index,
        policy,
        session,
        source,
        surface_background_offer,
        point,
    )
}

/// Resolves a drop against one exact output already proven presented.
///
/// This is the canonical resolver entry used by the pointer journal. The
/// caller owns presentation provenance; this function owns deterministic
/// geometry selection, source suppression, policy, and transaction preflight.
#[allow(
    clippy::too_many_arguments,
    reason = "the resolver binds every authoritative drag fact without an ambient context"
)]
#[cfg(test)]
pub(crate) fn resolve_presented_drop(
    stamp: SurfaceSceneStamp,
    ready: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &WorkspaceIndex,
    policy: &DockPolicySnapshot,
    session: DragSessionId,
    source: MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    point: LogicalPoint,
) -> Result<DropQuery, DropResolutionError> {
    resolve_presented_drop_with_targeting(
        stamp,
        ready,
        source_layout_facts,
        workspace,
        workspace_version,
        workspace_index,
        policy,
        session,
        source,
        surface_background_offer,
        point,
        DropTargeting::Magnetic,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the resolver binds every authoritative drag fact without an ambient context"
)]
pub(crate) fn resolve_presented_drop_with_targeting(
    stamp: SurfaceSceneStamp,
    ready: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &WorkspaceIndex,
    policy: &DockPolicySnapshot,
    session: DragSessionId,
    source: MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    point: LogicalPoint,
    targeting: DropTargeting,
) -> Result<DropQuery, DropResolutionError> {
    let surface = stamp.surface();

    if !HitRegion::new(ready.bounds()).contains(point) {
        return Ok(DropQuery::new(
            DropResolution::KnownNone(KnownDropAbsence::new(stamp, surface, point)),
            None,
            TargetObservation::OutsideSurface,
        ));
    }
    if targeting == DropTargeting::Disabled {
        return Ok(DropQuery::new(
            DropResolution::KnownNone(KnownDropAbsence::new(stamp, surface, point)),
            None,
            TargetObservation::surface(stamp, SceneLayerKey::surface_base(), None),
        ));
    }

    let assessment = DropEligibilityContext::new(
        workspace,
        workspace_version,
        workspace_index,
        policy,
        ready,
        source_layout_facts,
        &source,
        surface_background_offer,
    );
    let suppression = assessment.source_suppression();
    let occluding_layer = occluding_layer(ready, point, suppression);
    let selected = select_guide_clusters(ready, point, occluding_layer, suppression);
    let (mut affordance, exact) = build_affordance(stamp, surface, point, selected, &assessment)?;
    let (resolution, candidate) = match exact {
        Some(ExactGuidePreparation::Eligible {
            target,
            candidate,
            visual,
            mutation,
        }) => (
            DropResolution::Resolved(ResolvedDrop {
                scene: stamp,
                session,
                source,
                target,
                visual,
                mutation,
            }),
            Some(TargetCandidate::Direct(candidate)),
        ),
        Some(ExactGuidePreparation::Rejected {
            target,
            candidate,
            reason,
        }) => (
            DropResolution::Rejected(RejectedDrop::new(
                stamp,
                surface,
                point,
                vec![DropCandidateRejection::new(target, reason)],
            )),
            Some(TargetCandidate::Direct(candidate)),
        ),
        None => {
            let location = DropLocation {
                scene: stamp,
                ready,
                workspace,
                workspace_version,
                workspace_index,
                policy,
                source_layout_facts,
                session,
                surface,
                point,
                occluding_layer,
                suppression,
                surface_background_offer,
            };
            let (direct, direct_candidate) =
                resolve_exact_unguided_target(&location, source.clone())?;
            if targeting == DropTargeting::Magnetic
                && matches!(direct, DropResolution::KnownNone(_))
            {
                resolve_magnetic_target(&location, &assessment, source.clone())?.map_or(
                    (direct, None),
                    |(resolution, candidate)| {
                        (resolution, Some(TargetCandidate::Magnetic(candidate)))
                    },
                )
            } else {
                (direct, direct_candidate.map(TargetCandidate::Direct))
            }
        }
    };
    let selected_target = resolution_target_id(&resolution);
    if let (Some(affordance), Some(target)) = (&mut affordance, selected_target) {
        affordance.activate_target(target);
    }

    Ok(DropQuery::new(
        resolution,
        affordance,
        TargetObservation::surface(
            stamp,
            occluding_layer.unwrap_or_else(SceneLayerKey::surface_base),
            candidate,
        ),
    ))
}

fn resolution_target_id(resolution: &DropResolution) -> Option<DropTargetId> {
    match resolution {
        DropResolution::Resolved(resolved) => Some(resolved.target_id()),
        DropResolution::Rejected(rejected) => rejected
            .candidates()
            .first()
            .map(|candidate| candidate.target_id()),
        DropResolution::KnownNone(_) | DropResolution::Unavailable(_) => None,
    }
}

fn resolve_magnetic_target(
    location: &DropLocation<'_>,
    assessment: &DropEligibilityContext<'_>,
    source: MovePayload,
) -> Result<
    Option<(
        DropResolution,
        crate::interaction::targeting::field::MagneticTargetCandidate,
    )>,
    DropResolutionError,
> {
    let layer = location
        .occluding_layer
        .unwrap_or_else(SceneLayerKey::surface_base);
    let Some(candidate) = location
        .ready
        .magnetic_target_at(location.point, layer, |target| {
            !location.suppression.excludes_target(target)
                && !source_excludes_magnetic_target(&source, target)
        })
    else {
        return Ok(None);
    };
    let target_id = candidate.target();
    let target = location
        .ready
        .drop_target(target_id)
        .ok_or(DropResolutionError::PresentedTargetIdentityMismatch { target: target_id })?;
    if target.layer() != layer {
        return Err(DropResolutionError::PresentedTargetIdentityMismatch { target: target_id });
    }
    match assessment.prepare_winner(target)? {
        PreparedDropWinner::Eligible { mutation, visual } => Ok(Some((
            DropResolution::Resolved(ResolvedDrop {
                scene: location.scene,
                session: location.session,
                source,
                target: target_id,
                visual,
                mutation,
            }),
            candidate,
        ))),
        PreparedDropWinner::Rejected(reason) => Ok(Some((
            DropResolution::Rejected(RejectedDrop::new(
                location.scene,
                location.surface,
                location.point,
                vec![DropCandidateRejection::new(target_id, reason)],
            )),
            candidate,
        ))),
    }
}

fn source_excludes_magnetic_target(source: &MovePayload, target: DropTargetId) -> bool {
    let DropTargetId::Center { root, tabs, .. } = target else {
        return false;
    };
    match source {
        MovePayload::Item(source) => source.root() == root && source.tabs() == tabs,
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            source.root() == root && source.node() == tabs
        }
    }
}

/// Resolves one authoritative surface-local point against an immutable scene.
///
/// Visible candidates are first restricted by the frontmost explicit occlusion
/// at the point: targets below that layer cannot participate or become fallback
/// candidates. Remaining resolution order is exactly
/// `TabGap > Center > InnerEdge > OuterEdge > SurfaceBackground`, then larger
/// explicit layer, then the lowest structural target identity. Only that unique
/// geometric winner is checked against availability, policy, root-offer facts,
/// and a complete U3 candidate transaction. Rejection never falls through to a
/// lower target. Geometry area, scene insertion order, focus, time, and pointer
/// delta never participate.
///
/// If removing the frozen payload consumes its complete source root, that
/// root's targets and contained presentation are excluded before occlusion and
/// candidate ordering.
///
/// Contained occlusion records preserve the core-owned, unclipped durable
/// rectangle. Because resolution first proves that `point` lies inside the
/// ready surface bounds, their effective hit domain is exactly the durable
/// rectangle intersected with those bounds.
///
/// # Errors
///
/// Returns [`DropResolutionError`] if U3 candidate preparation exposes an
/// invariant, canonicalization, validation, or reconciliation failure. Expected
/// checked-command rejections are represented by [`DropResolution::Rejected`].
#[allow(
    clippy::too_many_arguments,
    reason = "the resolver binds every authoritative drag fact without an ambient context"
)]
#[cfg(test)]
fn resolve_presented_unguided_drop(
    stamp: SurfaceSceneStamp,
    surface_scene: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &WorkspaceIndex,
    policy: &DockPolicySnapshot,
    session: DragSessionId,
    source: MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    point: LogicalPoint,
) -> Result<DropResolution, DropResolutionError> {
    let surface = stamp.surface();

    if !HitRegion::new(surface_scene.bounds()).contains(point) {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            stamp, surface, point,
        )));
    }

    let assessment = DropEligibilityContext::new(
        workspace,
        workspace_version,
        workspace_index,
        policy,
        surface_scene,
        source_layout_facts,
        &source,
        surface_background_offer,
    );
    let suppression = assessment.source_suppression();
    let occluding_layer = occluding_layer(surface_scene, point, suppression);
    let mut hits: Vec<&DropTargetRecord> = surface_scene
        .drop_targets()
        .iter()
        .map(AsRef::as_ref)
        .chain(surface_scene.surface_background())
        .filter(|target| {
            !suppression.excludes_target(target.id())
                && target.region().contains(point)
                && occluding_layer.is_none_or(|layer| target.layer() >= layer)
        })
        .collect();
    if hits.is_empty() {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            stamp, surface, point,
        )));
    }

    hits.sort_unstable_by(|left, right| {
        right
            .id()
            .kind()
            .resolution_priority()
            .cmp(&left.id().kind().resolution_priority())
            .then_with(|| right.layer().cmp(&left.layer()))
            .then_with(|| left.id().cmp(&right.id()))
    });

    let Some(target) = hits.first().copied() else {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            stamp, surface, point,
        )));
    };
    match assessment.prepare_winner(target)? {
        PreparedDropWinner::Eligible { mutation, visual } => {
            Ok(DropResolution::Resolved(ResolvedDrop {
                scene: stamp,
                session,
                source,
                target: target.id(),
                visual,
                mutation,
            }))
        }
        PreparedDropWinner::Rejected(reason) => Ok(DropResolution::Rejected(RejectedDrop::new(
            stamp,
            surface,
            point,
            vec![DropCandidateRejection::new(target.id(), reason)],
        ))),
    }
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "contract tests exercise unguided resolution independently"
)]
fn resolve_unguided_drop(
    scene: &SurfaceSceneSet,
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    session: DragSessionId,
    source: MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    surface: SurfaceId,
    point: LogicalPoint,
) -> Result<DropResolution, DropResolutionError> {
    let Some(surface_scene) = scene.surface(surface) else {
        return Ok(DropResolution::Unavailable(UnavailableDrop::new(
            None,
            surface,
            DropSurfaceUnavailable::MissingSurface,
        )));
    };
    let Some(ready) = scene.ready_surface(surface) else {
        let reason = match surface_scene {
            SurfaceScene::Stale(_) => DropSurfaceUnavailable::Stale,
            SurfaceScene::Bootstrap(_) => DropSurfaceUnavailable::Bootstrap,
            SurfaceScene::Ready(_) => DropSurfaceUnavailable::PendingPaint,
        };
        return Ok(DropResolution::Unavailable(UnavailableDrop::new(
            Some(surface_scene.stamp()),
            surface,
            reason,
        )));
    };
    let workspace_version = WorkspaceVersion::new(
        ready.stamp().requirement().workspace_epoch(),
        WorkspaceRevision::default(),
    );
    let workspace_index =
        WorkspaceIndex::build(workspace, workspace_version).map_err(|source| {
            DropResolutionError::UnexpectedPrevalidation(TransactionError::Command {
                index: 0,
                source,
            })
        })?;
    let source_layout_facts = source_presentation_layout_facts(scene, workspace, &source);
    resolve_presented_unguided_drop(
        ready.stamp(),
        ready.plan(),
        source_layout_facts,
        workspace,
        workspace_version,
        &workspace_index,
        policy,
        session,
        source,
        surface_background_offer,
        point,
    )
}

fn occluding_layer(
    ready: &PresentationPlan,
    point: LogicalPoint,
    suppression: DropSourceSuppression,
) -> Option<SceneLayerKey> {
    ready
        .drop_occlusions()
        .iter()
        .filter(|occlusion| {
            !suppression.excludes_occlusion(occlusion.floating())
                && occlusion.region().contains(point)
        })
        .map(|occlusion| occlusion.layer())
        .max()
}

fn select_guide_clusters(
    ready: &PresentationPlan,
    point: LogicalPoint,
    occluding_layer: Option<SceneLayerKey>,
    suppression: DropSourceSuppression,
) -> Vec<&DropGuideClusterRecord> {
    let visible = |cluster: &&DropGuideClusterRecord| {
        !suppression.excludes_cluster(cluster.id())
            && cluster.activation().contains(point)
            && occluding_layer.is_none_or(|layer| cluster.layer() >= layer)
    };
    let frontmost = |left: &&DropGuideClusterRecord, right: &&DropGuideClusterRecord| {
        right
            .layer()
            .cmp(&left.layer())
            .then_with(|| left.id().cmp(&right.id()))
    };

    let inner = ready
        .drop_guide_clusters()
        .iter()
        .filter(visible)
        .filter(|cluster| matches!(cluster.id().scope, DropGuideScope::Inner(_)))
        .min_by(frontmost);
    if let Some(inner) = inner {
        let inner_id = inner.id();
        let outer = ready.drop_guide_clusters().iter().find(|cluster| {
            cluster.id() == DropGuideClusterId::outer(inner_id.surface, inner_id.root)
                && cluster.layer() == inner.layer()
                && cluster.activation().contains(point)
                && occluding_layer.is_none_or(|layer| cluster.layer() >= layer)
        });
        return [outer, Some(inner)].into_iter().flatten().collect();
    }

    ready
        .drop_guide_clusters()
        .iter()
        .filter(visible)
        .filter(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .min_by(frontmost)
        .into_iter()
        .collect()
}

fn build_affordance(
    scene: SurfaceSceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
    clusters: Vec<&DropGuideClusterRecord>,
    assessment: &DropEligibilityContext<'_>,
) -> Result<(Option<DropAffordance>, Option<ExactGuidePreparation>), DropResolutionError> {
    if clusters.is_empty() {
        return Ok((None, None));
    }

    let active = clusters.iter().find_map(|cluster| {
        let cluster_id = cluster.id();
        cluster.targets().find_map(|(slot, guide_target)| {
            guide_target
                .hit()
                .contains(point)
                .then(|| DropGuideTargetKey::new(cluster_id, slot, guide_target.id()))
        })
    });
    let mut exact = None;
    let mut owned_clusters = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        let cluster_id = cluster.id();
        let mut targets = Vec::with_capacity(cluster.targets().count());
        for (slot, guide_target) in cluster.targets() {
            let target = guide_target.target();
            let key = DropGuideTargetKey::new(cluster_id, slot, target.id());
            let is_exact_hit = active == Some(key);
            let eligibility = if is_exact_hit {
                match assessment.prepare_winner(target)? {
                    PreparedDropWinner::Eligible { mutation, visual } => {
                        let candidate = DirectTargetCandidate::new(
                            target.id(),
                            DirectTargetProvenance::Guide {
                                cluster: cluster_id,
                                slot,
                            },
                            target.region(),
                        );
                        exact = Some(ExactGuidePreparation::Eligible {
                            target: target.id(),
                            candidate,
                            visual,
                            mutation,
                        });
                        DropGuideEligibility::Eligible
                    }
                    PreparedDropWinner::Rejected(reason) => {
                        let candidate = DirectTargetCandidate::new(
                            target.id(),
                            DirectTargetProvenance::Guide {
                                cluster: cluster_id,
                                slot,
                            },
                            target.region(),
                        );
                        exact = Some(ExactGuidePreparation::Rejected {
                            target: target.id(),
                            candidate,
                            reason: reason.clone(),
                        });
                        DropGuideEligibility::Rejected(reason)
                    }
                }
            } else {
                match assessment.check_target(target)? {
                    PreparedDropTarget::Eligible(_) => DropGuideEligibility::Eligible,
                    PreparedDropTarget::Rejected(reason) => DropGuideEligibility::Rejected(reason),
                }
            };
            targets.push(DropAffordanceTarget::new(
                key,
                guide_target.draw(),
                eligibility,
            ));
        }
        owned_clusters.push(DropAffordanceCluster::new(
            cluster_id,
            cluster.activation(),
            cluster.layer(),
            targets,
        ));
    }
    // Selection is outer-first; paint inner then outer so the winner stays visible.
    owned_clusters.reverse();

    Ok((
        Some(DropAffordance::new(
            scene,
            surface,
            point,
            owned_clusters,
            active,
            assessment
                .target_plan
                .contained_projection()
                .map(|projection| projection.drop_source_suppression())
                .unwrap_or_default(),
        )),
        exact,
    ))
}

fn resolve_exact_unguided_target(
    location: &DropLocation<'_>,
    source: MovePayload,
) -> Result<(DropResolution, Option<DirectTargetCandidate>), DropResolutionError> {
    let target = location
        .ready
        .drop_targets()
        .iter()
        .map(AsRef::as_ref)
        .filter(|target| target.id().kind() == DropTargetKind::TabGap)
        .chain(location.ready.surface_background())
        .filter(|target| !location.suppression.excludes_target(target.id()))
        .filter(|target| target.region().contains(location.point))
        .filter(|target| {
            location
                .occluding_layer
                .is_none_or(|layer| target.layer() >= layer)
        })
        .min_by(|left, right| {
            right
                .id()
                .kind()
                .resolution_priority()
                .cmp(&left.id().kind().resolution_priority())
                .then_with(|| right.layer().cmp(&left.layer()))
                .then_with(|| left.id().cmp(&right.id()))
        });
    let Some(target) = target else {
        return Ok((
            DropResolution::KnownNone(KnownDropAbsence::new(
                location.scene,
                location.surface,
                location.point,
            )),
            None,
        ));
    };

    let provenance = match target.id().kind() {
        DropTargetKind::TabGap => DirectTargetProvenance::TabGap,
        DropTargetKind::SurfaceBackground => DirectTargetProvenance::SurfaceBackground,
        DropTargetKind::Center | DropTargetKind::InnerEdge | DropTargetKind::OuterEdge => {
            return Err(DropResolutionError::PresentedTargetIdentityMismatch {
                target: target.id(),
            });
        }
    };
    let candidate = DirectTargetCandidate::new(target.id(), provenance, target.region());

    let assessment = DropEligibilityContext::new(
        location.workspace,
        location.workspace_version,
        location.workspace_index,
        location.policy,
        location.ready,
        location.source_layout_facts,
        &source,
        location.surface_background_offer,
    );
    match assessment.prepare_winner(target)? {
        PreparedDropWinner::Eligible { mutation, visual } => Ok((
            DropResolution::Resolved(ResolvedDrop {
                scene: location.scene,
                session: location.session,
                source,
                target: target.id(),
                visual,
                mutation,
            }),
            Some(candidate),
        )),
        PreparedDropWinner::Rejected(reason) => Ok((
            DropResolution::Rejected(RejectedDrop::new(
                location.scene,
                location.surface,
                location.point,
                vec![DropCandidateRejection::new(target.id(), reason)],
            )),
            Some(candidate),
        )),
    }
}

struct DropEligibilityContext<'a> {
    workspace: &'a Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &'a WorkspaceIndex,
    policy: &'a DockPolicySnapshot,
    target_plan: &'a PresentationPlan,
    source_layout_facts: Option<&'a PresentationLayoutFacts>,
    source: &'a MovePayload,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    topology: Result<crate::graph::operation::DropCommandEligibility<'a>, CommandError>,
    complete_root_source: Result<Option<NodeSource>, CommandError>,
}

impl<'a> DropEligibilityContext<'a> {
    fn new(
        workspace: &'a Workspace,
        workspace_version: WorkspaceVersion,
        workspace_index: &'a WorkspaceIndex,
        policy: &'a DockPolicySnapshot,
        target_plan: &'a PresentationPlan,
        source_layout_facts: Option<&'a PresentationLayoutFacts>,
        source: &'a MovePayload,
        surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    ) -> Self {
        let topology = crate::graph::operation::DropCommandEligibility::new_indexed(
            workspace,
            workspace_version,
            workspace_index,
            policy,
            source,
        );
        let complete_root_source = match &topology {
            Ok(_) => {
                workspace_index.capture_complete_root_source(workspace, workspace_version, source)
            }
            Err(error) => Err(error.clone()),
        };
        Self {
            workspace,
            workspace_version,
            workspace_index,
            policy,
            target_plan,
            source_layout_facts,
            source,
            surface_background_offer,
            topology,
            complete_root_source,
        }
    }

    fn source_suppression(&self) -> DropSourceSuppression {
        self.complete_root_source
            .as_ref()
            .ok()
            .map(|complete_root| {
                DropSourceSuppression::from_source(
                    self.workspace,
                    self.source,
                    complete_root.as_ref().map(NodeSource::root),
                )
            })
            .unwrap_or_default()
    }

    fn check_target(
        &self,
        target: &DropTargetRecord,
    ) -> Result<PreparedDropTarget, DropResolutionError> {
        record_drop_target_assessed();
        if let DropTargetAvailability::Unavailable(reason) = target.availability() {
            return Ok(PreparedDropTarget::Rejected(
                DropRejectionReason::SceneUnavailable(reason),
            ));
        }

        let command = match target.location() {
            DropTargetLocation::SurfaceBackground { surface } => {
                let complete_root_source = match &self.complete_root_source {
                    Ok(source) => source.as_ref(),
                    Err(error) => return classify_command_error(error.clone()),
                };
                let Some(command) = prepare_surface_background_command(
                    self.workspace,
                    self.source,
                    complete_root_source,
                    SurfaceBackground::new(surface),
                    self.surface_background_offer,
                ) else {
                    return Ok(PreparedDropTarget::Rejected(
                        DropRejectionReason::SurfaceBackgroundRootOfferMissing,
                    ));
                };
                command
            }
            location => {
                let destination = match rebind_topology_target(
                    self.workspace,
                    self.workspace_version,
                    self.workspace_index,
                    location,
                    location
                        .root()
                        .and_then(|root| self.target_plan.command_basis(root)),
                ) {
                    Ok(destination) => destination,
                    Err(TargetRebindError::Stale) => {
                        return Ok(PreparedDropTarget::Rejected(
                            DropRejectionReason::SceneUnavailable(DropTargetUnavailable::Stale),
                        ));
                    }
                    Err(TargetRebindError::Command(error)) => {
                        return classify_command_error(error);
                    }
                };
                match &self.topology {
                    Ok(context) => {
                        if let Err(error) = context.check_target_indexed(
                            self.workspace_version,
                            self.workspace_index,
                            &destination,
                        ) {
                            return classify_command_error(error);
                        }
                    }
                    Err(error) => return classify_command_error(error.clone()),
                }
                WorkspaceCommand::Move {
                    payload: self.source.clone(),
                    target: destination,
                }
            }
        };
        Ok(PreparedDropTarget::Eligible(command))
    }

    fn prepare_winner(
        &self,
        target: &DropTargetRecord,
    ) -> Result<PreparedDropWinner, DropResolutionError> {
        record_geometric_winner();
        match self.check_target(target)? {
            PreparedDropTarget::Eligible(command) => self.preflight(target, command),
            PreparedDropTarget::Rejected(reason) => Ok(PreparedDropWinner::Rejected(reason)),
        }
    }

    fn preflight(
        &self,
        target: &DropTargetRecord,
        command: WorkspaceCommand,
    ) -> Result<PreparedDropWinner, DropResolutionError> {
        match crate::graph::operation::prepare_transaction(
            self.workspace,
            self.policy,
            std::slice::from_ref(&command),
        ) {
            Ok(prepared) => {
                let preview = preview::resolved_visual(
                    self.workspace,
                    prepared.candidate(),
                    self.policy,
                    self.target_plan,
                    self.source_layout_facts,
                    target,
                    &command,
                )
                .map_err(|source| DropResolutionError::PreviewProjection {
                    detail: source.to_string(),
                })?;
                let (candidate, report) = prepared.into_parts();
                Ok(PreparedDropWinner::Eligible {
                    mutation: PreparedDropMutation::new(
                        self.workspace_version,
                        self.policy.revision(),
                        command,
                        candidate,
                        report,
                    )
                    .with_preview_tab_strip(preview.tab_strip),
                    visual: preview.visual,
                })
            }
            Err(TransactionError::Command {
                source: CommandError::Policy(reason),
                ..
            }) => Ok(PreparedDropWinner::Rejected(DropRejectionReason::Policy(
                reason,
            ))),
            Err(error) if error.is_expected_rejection() => Ok(PreparedDropWinner::Rejected(
                DropRejectionReason::Prevalidation(error),
            )),
            Err(error) => Err(DropResolutionError::UnexpectedPrevalidation(error)),
        }
    }
}

enum TargetRebindError {
    Stale,
    Command(CommandError),
}

impl From<CommandError> for TargetRebindError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

/// Refreshes command preconditions without changing the exact structural target
/// selected by the frozen presentation.
///
/// Selection and MRU changes legitimately invalidate a root fingerprint while
/// leaving the target identity and geometry untouched. A journal reducer must
/// therefore recapture the proof from live topology, but it must never retarget
/// to a different root, node, edge, index, or surface.
fn rebind_topology_target(
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &WorkspaceIndex,
    location: DropTargetLocation,
    frozen: Option<&crate::graph::command::NodeFingerprint>,
) -> Result<DockTarget, TargetRebindError> {
    let frozen = frozen.ok_or(TargetRebindError::Stale)?;
    if location.root() != Some(frozen.root()) {
        return Err(TargetRebindError::Stale);
    }
    let rebound = match location {
        DropTargetLocation::Center { root, tabs, .. } => DockTarget::Center(
            workspace_index.capture_tab_target(workspace, workspace_version, root, tabs)?,
        ),
        DropTargetLocation::TabGap {
            root, tabs, index, ..
        } => DockTarget::TabGap {
            target: workspace_index.capture_tab_target(workspace, workspace_version, root, tabs)?,
            index,
        },
        DropTargetLocation::InnerEdge {
            root,
            node,
            edge,
            fraction,
            ..
        } => DockTarget::InnerEdge(workspace_index.capture_inner_edge_target(
            workspace,
            workspace_version,
            root,
            node,
            edge,
            fraction,
        )?),
        DropTargetLocation::OuterEdge {
            root,
            node,
            edge,
            fraction,
            ..
        } => {
            let rebound = workspace_index.capture_outer_edge_target(
                workspace,
                workspace_version,
                root,
                edge,
                fraction,
            )?;
            if rebound.node() != node {
                return Err(TargetRebindError::Stale);
            }
            DockTarget::OuterEdge(rebound)
        }
        DropTargetLocation::SurfaceBackground { .. } => return Err(TargetRebindError::Stale),
    };
    if rebound.surface() != location.surface() {
        return Err(TargetRebindError::Stale);
    }
    let current = workspace_index
        .root_fingerprint(frozen.root())
        .ok_or(TargetRebindError::Stale)?;
    if !frozen.presentation_structure_eq(current) {
        return Err(TargetRebindError::Stale);
    }
    Ok(rebound)
}

fn classify_command_error(error: CommandError) -> Result<PreparedDropTarget, DropResolutionError> {
    match error {
        CommandError::Policy(reason) => Ok(PreparedDropTarget::Rejected(
            DropRejectionReason::Policy(reason),
        )),
        error if error.is_expected_rejection() => Ok(PreparedDropTarget::Rejected(
            DropRejectionReason::Prevalidation(TransactionError::Command {
                index: 0,
                source: error,
            }),
        )),
        error => Err(DropResolutionError::UnexpectedPrevalidation(
            TransactionError::Command {
                index: 0,
                source: error,
            },
        )),
    }
}

#[cfg(test)]
pub(crate) mod structural_work {
    use std::cell::Cell;

    use crate::graph::{Node, Workspace};

    /// Logical inventory carried through one explicit core-owned workspace fork.
    ///
    /// Nodes and vector elements count occurrences in the source Workspace; Arc
    /// sharing means these are not physical payload-copy or allocation counts.
    /// Legacy deep-clone field names retain these inventory semantics. Actual
    /// store handle forks and payload cloning are recorded in workspace_storage.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WorkspaceCloneVolume {
        pub(crate) nodes: usize,
        pub(crate) tab_items: usize,
        pub(crate) split_child_edges: usize,
        pub(crate) tab_mru_items: usize,
        pub(crate) roots: usize,
        pub(crate) surfaces: usize,
        pub(crate) contained_records: usize,
        pub(crate) contained_roster_entries: usize,
    }

    impl WorkspaceCloneVolume {
        pub(crate) fn capture(workspace: &Workspace) -> Self {
            Self::capture_data(workspace.storage())
        }

        pub(crate) fn capture_data(workspace: &crate::graph::WorkspaceData) -> Self {
            let mut volume = Self {
                nodes: workspace.nodes.len(),
                roots: workspace.roots.len(),
                surfaces: workspace.surfaces.len(),
                contained_records: workspace.contained_floatings.len(),
                contained_roster_entries: workspace
                    .surfaces
                    .values()
                    .map(|surface| surface.contained.len())
                    .sum(),
                ..Self::default()
            };
            for node in workspace.nodes.values() {
                match node {
                    Node::Tabs { items, mru, .. } => {
                        volume.tab_items += items.len();
                        volume.tab_mru_items += mru.len();
                    }
                    Node::Split { children, .. } => {
                        volume.split_child_edges += children.len();
                    }
                }
            }
            volume
        }

        fn accumulate(&mut self, other: Self) {
            self.nodes += other.nodes;
            self.tab_items += other.tab_items;
            self.split_child_edges += other.split_child_edges;
            self.tab_mru_items += other.tab_mru_items;
            self.roots += other.roots;
            self.surfaces += other.surfaces;
            self.contained_records += other.contained_records;
            self.contained_roster_entries += other.contained_roster_entries;
        }
    }

    /// Calls and cumulative source inventory for one named legacy clone category.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct DeepCloneWork {
        pub(crate) calls: usize,
        pub(crate) volume: WorkspaceCloneVolume,
    }

    impl DeepCloneWork {
        fn record(&mut self, workspace: &Workspace) {
            self.calls += 1;
            self.volume
                .accumulate(WorkspaceCloneVolume::capture(workspace));
        }
    }

    /// Explicit Workspace fork sites; payload sharing does not zero their inventory.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WorkspaceDeepCloneWork {
        pub(crate) engine_candidates: DeepCloneWork,
        pub(crate) transaction_candidates: DeepCloneWork,
    }

    /// Whole-engine candidate preparation. Workspace authority is shared;
    /// explicit mutation preparation owns the separate fork-inventory counters.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct EngineDeepCloneWork {
        pub(crate) atomic_candidates: DeepCloneWork,
        pub(crate) shared_workspace: WorkspaceCloneVolume,
        pub(crate) atomic_candidate_state: EngineCloneVolume,
    }

    /// Deterministic non-workspace state copied by whole-engine candidates.
    ///
    /// Plans and hit manifests are retained behind `Arc`, so their counts
    /// describe the cloned authority inventory rather than bytes copied. The
    /// presentation ledger maps are owned collections. Semantic input replay authority is one
    /// scalar guard; its count makes that bounded invariant visible to structural tests.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct EngineCloneVolume {
        pub(crate) scene_surfaces: usize,
        pub(crate) scene_retained_plans: usize,
        pub(crate) scene_paint_hit_regions: usize,
        pub(crate) presentation_hosts: usize,
        pub(crate) presentation_streams: usize,
        pub(crate) presentation_pending_outputs: usize,
        pub(crate) live_pointer_providers: usize,
        pub(crate) semantic_input_watermark_guards: usize,
    }

    impl EngineCloneVolume {
        fn accumulate(&mut self, other: Self) {
            self.scene_surfaces += other.scene_surfaces;
            self.scene_retained_plans += other.scene_retained_plans;
            self.scene_paint_hit_regions += other.scene_paint_hit_regions;
            self.presentation_hosts += other.presentation_hosts;
            self.presentation_streams += other.presentation_streams;
            self.presentation_pending_outputs += other.presentation_pending_outputs;
            self.live_pointer_providers += other.live_pointer_providers;
            self.semantic_input_watermark_guards += other.semantic_input_watermark_guards;
        }
    }

    /// Actual shared-store operations, separate from legacy logical inventory.
    ///
    /// Node Clone counts every concrete Node value copy after reset, including
    /// make_mut and owned Builder returns. Tabs copies include both their item
    /// and MRU elements. Unrelated domain Vec copies, allocator events, vacant
    /// slot extent and placement-directory copying are not measured here.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WorkspaceStorageWork {
        pub(crate) node_store_forks: usize,
        pub(crate) node_handle_clones: usize,
        pub(crate) node_payload_clones: usize,
        pub(crate) tab_item_copies: usize,
        pub(crate) split_child_copies: usize,
        pub(crate) split_weight_copies: usize,
        pub(crate) mru_item_copies: usize,
    }

    /// Test-only deterministic work performed by docking resolution and its
    /// transaction/authority dependencies.
    ///
    /// Legacy Workspace inventory uses explicit core call sites. The separate
    /// workspace_storage counters run at the concrete store/Node Clone
    /// implementations, so fixture copies after reset also appear there.
    /// Arbitrary Vec clones and allocator events are not intercepted.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct StructuralWorkMetrics {
        pub(crate) root_fingerprint_builds: usize,
        pub(crate) root_fingerprint_node_visits: usize,
        pub(crate) drop_targets_assessed: usize,
        pub(crate) geometric_winners: usize,
        pub(crate) presentation_hit_lookup_comparisons: usize,
        pub(crate) presentation_roster_full_captures: usize,
        pub(crate) presentation_roster_surface_freezes: usize,
        pub(crate) selection_pane_visits: usize,
        pub(crate) selection_lookups: usize,
        pub(crate) canonical_full_passes: usize,
        // Successful body construction only; failed partial builds and allocator
        // events are excluded. Rows and shared split handles are actual outputs.
        pub(crate) completed_root_structures: usize,
        pub(crate) root_structure_rows: usize,
        pub(crate) root_structure_split_handles: usize,
        pub(crate) workspace_full_validations: usize,
        pub(crate) item_multiset_scans: usize,
        pub(crate) item_multiset_item_visits: usize,
        pub(crate) transaction_prepares: usize,
        pub(crate) transaction_commands: usize,
        pub(crate) workspace_deep_clones: WorkspaceDeepCloneWork,
        pub(crate) workspace_storage: WorkspaceStorageWork,
        pub(crate) engine_deep_clones: EngineDeepCloneWork,
    }

    thread_local! {
        static STRUCTURAL_WORK: Cell<StructuralWorkMetrics> =
            Cell::new(StructuralWorkMetrics::default());
        static PRESENTATION_HIT_LOOKUP_COMPARISONS: Cell<usize> = Cell::new(0);
    }

    fn update(update: impl FnOnce(&mut StructuralWorkMetrics)) {
        // Opt-in production-profile probes exclude the test ledger's graph traversals.
        if option_env!("DOCKSPACE_BENCH_NO_WORK_COUNTERS").is_some() {
            return;
        }
        STRUCTURAL_WORK.with(|work| {
            let mut next = work.get();
            update(&mut next);
            work.set(next);
        });
    }

    pub(crate) fn reset() {
        STRUCTURAL_WORK.with(|work| work.set(StructuralWorkMetrics::default()));
        PRESENTATION_HIT_LOOKUP_COMPARISONS.with(|comparisons| comparisons.set(0));
    }

    pub(crate) fn snapshot() -> StructuralWorkMetrics {
        let mut snapshot = STRUCTURAL_WORK.with(Cell::get);
        snapshot.presentation_hit_lookup_comparisons =
            PRESENTATION_HIT_LOOKUP_COMPARISONS.with(Cell::get);
        snapshot
    }

    pub(crate) fn record_root_structure_completed(rows: usize, split_handles: usize) {
        update(|work| {
            work.completed_root_structures += 1;
            work.root_structure_rows += rows;
            work.root_structure_split_handles += split_handles;
        });
    }

    pub(crate) fn record_node_store_clone(handles: usize) {
        update(|work| {
            work.workspace_storage.node_store_forks += 1;
            work.workspace_storage.node_handle_clones += handles;
        });
    }

    pub(crate) fn record_node_payload_clone(node: &Node) {
        update(|work| {
            let copies = &mut work.workspace_storage;
            copies.node_payload_clones += 1;
            match node {
                Node::Tabs { items, mru, .. } => {
                    copies.tab_item_copies += items.len();
                    copies.mru_item_copies += mru.len();
                }
                Node::Split {
                    children, weights, ..
                } => {
                    copies.split_child_copies += children.len();
                    copies.split_weight_copies += weights.len();
                }
            }
        });
    }

    pub(crate) fn record_root_fingerprint_build() {
        update(|work| work.root_fingerprint_builds += 1);
    }

    pub(crate) fn record_root_fingerprint_node_visit() {
        update(|work| work.root_fingerprint_node_visits += 1);
    }

    pub(crate) fn record_drop_target_assessed() {
        update(|work| work.drop_targets_assessed += 1);
    }

    pub(crate) fn record_geometric_winner() {
        update(|work| work.geometric_winners += 1);
    }

    pub(crate) fn record_presentation_hit_lookup_comparison() {
        PRESENTATION_HIT_LOOKUP_COMPARISONS.with(|comparisons| {
            comparisons.set(comparisons.get() + 1);
        });
    }

    pub(crate) fn record_presentation_roster_full_capture() {
        update(|work| work.presentation_roster_full_captures += 1);
    }

    pub(crate) fn record_presentation_roster_surface_freeze() {
        update(|work| work.presentation_roster_surface_freezes += 1);
    }

    pub(crate) fn record_selection_lookup() {
        update(|work| work.selection_lookups += 1);
    }

    pub(crate) fn record_canonical_full_pass() {
        update(|work| work.canonical_full_passes += 1);
    }

    pub(crate) fn record_workspace_full_validation() {
        update(|work| work.workspace_full_validations += 1);
    }

    pub(crate) fn record_item_multiset_scan(item_visits: usize) {
        update(|work| {
            work.item_multiset_scans += 1;
            work.item_multiset_item_visits += item_visits;
        });
    }

    pub(crate) fn record_transaction_prepare(commands: usize) {
        update(|work| {
            work.transaction_prepares += 1;
            work.transaction_commands += commands;
        });
    }

    pub(crate) fn record_transaction_candidate_clone(workspace: &Workspace) {
        update(|work| {
            work.workspace_deep_clones
                .transaction_candidates
                .record(workspace);
        });
    }

    pub(crate) fn record_transaction_candidate_data_clone(workspace: &crate::graph::WorkspaceData) {
        update(|work| {
            let clones = &mut work.workspace_deep_clones.transaction_candidates;
            clones.calls += 1;
            clones
                .volume
                .accumulate(WorkspaceCloneVolume::capture_data(workspace));
        });
    }

    pub(crate) fn record_engine_workspace_candidate_clone(workspace: &Workspace) {
        update(|work| {
            work.workspace_deep_clones
                .engine_candidates
                .record(workspace);
        });
    }

    pub(crate) fn record_engine_atomic_candidate_clone(
        workspace: &Workspace,
        state: EngineCloneVolume,
    ) {
        update(|work| {
            work.engine_deep_clones.atomic_candidates.calls += 1;
            work.engine_deep_clones
                .shared_workspace
                .accumulate(WorkspaceCloneVolume::capture(workspace));
            work.engine_deep_clones
                .atomic_candidate_state
                .accumulate(state);
        });
    }
}

#[cfg(test)]
use structural_work::StructuralWorkMetrics as DropResolutionWork;

#[cfg(test)]
fn reset_resolution_work() {
    structural_work::reset();
}

#[cfg(test)]
fn resolution_work() -> DropResolutionWork {
    structural_work::snapshot()
}

#[cfg(test)]
fn record_drop_target_assessed() {
    structural_work::record_drop_target_assessed();
}

#[cfg(not(test))]
#[inline]
fn record_drop_target_assessed() {}

#[cfg(test)]
fn record_geometric_winner() {
    structural_work::record_geometric_winner();
}

#[cfg(not(test))]
#[inline]
fn record_geometric_winner() {}

fn prepare_surface_background_command(
    workspace: &Workspace,
    source: &MovePayload,
    complete_root_source: Option<&NodeSource>,
    destination: SurfaceBackground,
    offer: Option<SurfaceBackgroundRootOffer>,
) -> Option<WorkspaceCommand> {
    if let Some(source) = complete_root_source {
        if let Some(RootPresentationOwner::Contained { surface, floating }) =
            workspace.presentation_for_root(source.root())
            && surface == destination.surface()
        {
            return Some(WorkspaceCommand::PromoteContained {
                source: source.clone(),
                surface,
                floating,
            });
        }
        return Some(WorkspaceCommand::RehomeRoot {
            source: source.clone(),
            target: RootPresentationTarget::Main {
                surface: destination.surface(),
            },
        });
    }

    offer.map(|offer| WorkspaceCommand::InstallMainRoot {
        surface: destination.surface(),
        root: offer.root(),
        content: RootContent::Move(source.clone()),
    })
}

#[cfg(test)]
fn source_presentation_layout_facts<'a>(
    scene: &'a SurfaceSceneSet,
    workspace: &Workspace,
    source: &MovePayload,
) -> Option<&'a PresentationLayoutFacts> {
    let root = match source {
        MovePayload::Item(source) => source.root(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
    };
    let surface = match workspace.presentation_for_root(root)? {
        RootPresentationOwner::Main { surface }
        | RootPresentationOwner::Contained { surface, .. } => surface,
    };
    scene
        .ready_surface(surface)
        .and_then(|scene| scene.plan().layout_facts())
}

struct DropLocation<'a> {
    scene: SurfaceSceneStamp,
    ready: &'a PresentationPlan,
    source_layout_facts: Option<&'a PresentationLayoutFacts>,
    workspace: &'a Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: &'a WorkspaceIndex,
    policy: &'a DockPolicySnapshot,
    session: DragSessionId,
    surface: SurfaceId,
    point: LogicalPoint,
    occluding_layer: Option<SceneLayerKey>,
    suppression: DropSourceSuppression,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
}

enum PreparedDropTarget {
    Eligible(WorkspaceCommand),
    Rejected(DropRejectionReason),
}

enum PreparedDropWinner {
    Eligible {
        mutation: PreparedDropMutation,
        visual: DropVisual,
    },
    Rejected(DropRejectionReason),
}

enum ExactGuidePreparation {
    Eligible {
        target: DropTargetId,
        candidate: DirectTargetCandidate,
        visual: DropVisual,
        mutation: PreparedDropMutation,
    },
    Rejected {
        target: DropTargetId,
        candidate: DirectTargetCandidate,
        reason: DropRejectionReason,
    },
}

/// Complete semantic classification of one authoritative drop location.
// Resolution runs on pointer movement; keep the successful path allocation-free.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum DropResolution {
    /// One exact command survived all deterministic checks.
    Resolved(ResolvedDrop),
    /// The ready surface contained no target geometry at this point.
    KnownNone(KnownDropAbsence),
    /// The unique geometric winner was ineligible.
    Rejected(RejectedDrop),
    /// The authoritative surface was absent or had no ready scene facts.
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
    Unavailable(UnavailableDrop),
}

/// Owned result of one authoritative pointer query.
///
/// Resolution controls preview and delivery. Affordance contains only guide
/// painting facts and may be retained after the queried scene borrow ends.
#[derive(Debug, Clone, PartialEq)]
pub struct DropQuery {
    resolution: DropResolution,
    affordance: Option<DropAffordance>,
    observation: TargetObservation,
}

impl DropQuery {
    const fn new(
        resolution: DropResolution,
        affordance: Option<DropAffordance>,
        observation: TargetObservation,
    ) -> Self {
        Self {
            resolution,
            affordance,
            observation,
        }
    }

    /// Returns the semantic drop outcome used for preview and delivery.
    #[must_use]
    #[cfg(test)]
    pub const fn resolution(&self) -> &DropResolution {
        &self.resolution
    }

    /// Returns complete guide painting facts when a cluster is active.
    #[must_use]
    #[cfg(test)]
    pub const fn affordance(&self) -> Option<&DropAffordance> {
        self.affordance.as_ref()
    }

    pub(crate) fn into_evaluation_parts(
        self,
    ) -> (DropResolution, Option<DropAffordance>, TargetObservation) {
        (self.resolution, self.affordance, self.observation)
    }
}

/// Complete owned guide state selected at one authoritative point.
#[derive(Debug, Clone, PartialEq)]
pub struct DropAffordance {
    scene: SurfaceSceneStamp,
    target_layout_basis: SurfaceSceneStamp,
    guide_suppression: DropSourceSuppression,
    surface: SurfaceId,
    point: LogicalPoint,
    clusters: Vec<DropAffordanceCluster>,
    active: Option<DropGuideTargetKey>,
}

impl DropAffordance {
    const fn new(
        scene: SurfaceSceneStamp,
        surface: SurfaceId,
        point: LogicalPoint,
        clusters: Vec<DropAffordanceCluster>,
        active: Option<DropGuideTargetKey>,
        guide_suppression: DropSourceSuppression,
    ) -> Self {
        Self {
            scene,
            target_layout_basis: scene,
            guide_suppression,
            surface,
            point,
            clusters,
            active,
        }
    }

    /// Returns the sealed scene from which these painting facts were copied.
    #[must_use]
    pub const fn scene(&self) -> SurfaceSceneStamp {
        self.scene
    }

    pub(crate) const fn target_layout_basis(&self) -> SurfaceSceneStamp {
        self.target_layout_basis
    }

    pub(crate) fn bind_target_layout_basis(&mut self, basis: SurfaceSceneStamp) {
        self.target_layout_basis = basis;
    }

    pub(crate) const fn guide_suppression(&self) -> DropSourceSuppression {
        self.guide_suppression
    }

    /// Returns the authoritative logical surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns selected clusters in deterministic back-to-front paint order.
    ///
    /// A matching inner cluster precedes its outer cluster, so painting this
    /// slice in order leaves explicit outer controls on top. Targets within a
    /// cluster retain canonical `Center, Left, Right, Top, Bottom` order; outer
    /// clusters omit center. Exact hit resolution gives the matching outer
    /// button precedence if an adapter publishes overlapping inner and outer
    /// geometry. No area or proximity heuristic participates.
    #[must_use]
    pub fn clusters(&self) -> &[DropAffordanceCluster] {
        &self.clusters
    }

    /// Returns the exact active button, including its disabled eligibility.
    #[must_use]
    pub fn active_target(&self) -> Option<&DropAffordanceTarget> {
        let active = self.active?;
        self.clusters
            .iter()
            .flat_map(|cluster| cluster.targets.iter())
            .find(|target| target.key == active)
    }

    pub(crate) fn activate_target(&mut self, target: DropTargetId) -> bool {
        self.active = self
            .clusters
            .iter()
            .flat_map(|cluster| cluster.targets.iter())
            .find(|candidate| candidate.target_id() == target)
            .map(DropAffordanceTarget::key);
        self.active.is_some()
    }
}

/// One selected complete cluster copied from a sealed scene.
#[derive(Debug, Clone, PartialEq)]
pub struct DropAffordanceCluster {
    id: DropGuideClusterId,
    activation: HitRegion,
    layer: SceneLayerKey,
    targets: Vec<DropAffordanceTarget>,
}

impl DropAffordanceCluster {
    const fn new(
        id: DropGuideClusterId,
        activation: HitRegion,
        layer: SceneLayerKey,
        targets: Vec<DropAffordanceTarget>,
    ) -> Self {
        Self {
            id,
            activation,
            layer,
            targets,
        }
    }

    /// Returns the stable structural cluster identity.
    #[must_use]
    pub const fn id(&self) -> DropGuideClusterId {
        self.id
    }

    /// Returns the exact region which made this cluster visible.
    #[must_use]
    pub const fn activation(&self) -> HitRegion {
        self.activation
    }

    /// Returns the explicit paint and hit-test layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    /// Returns every complete target in canonical slot order.
    #[must_use]
    pub fn targets(&self) -> &[DropAffordanceTarget] {
        &self.targets
    }
}

/// Stable identity of one guide button within an owned affordance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DropGuideTargetKey {
    cluster: DropGuideClusterId,
    slot: DropGuideSlot,
    target: DropTargetId,
}

impl DropGuideTargetKey {
    const fn new(cluster: DropGuideClusterId, slot: DropGuideSlot, target: DropTargetId) -> Self {
        Self {
            cluster,
            slot,
            target,
        }
    }

    /// Returns the stable slot within the cluster.
    #[must_use]
    pub const fn slot(self) -> DropGuideSlot {
        self.slot
    }

    /// Returns the structural drop target identity.
    #[must_use]
    pub const fn target_id(self) -> DropTargetId {
        self.target
    }
}

/// One owned guide button with payload-specific eligibility.
#[derive(Debug, Clone, PartialEq)]
pub struct DropAffordanceTarget {
    key: DropGuideTargetKey,
    draw: LogicalRect,
    eligibility: DropGuideEligibility,
}

impl DropAffordanceTarget {
    const fn new(
        key: DropGuideTargetKey,
        draw: LogicalRect,
        eligibility: DropGuideEligibility,
    ) -> Self {
        Self {
            key,
            draw,
            eligibility,
        }
    }

    /// Returns the complete stable guide-button identity.
    #[must_use]
    pub const fn key(&self) -> DropGuideTargetKey {
        self.key
    }

    /// Returns this button's canonical cluster slot.
    #[must_use]
    pub const fn slot(&self) -> DropGuideSlot {
        self.key.slot()
    }

    /// Returns the structural drop target identity.
    #[must_use]
    pub const fn target_id(&self) -> DropTargetId {
        self.key.target_id()
    }

    /// Returns the exact rectangle used to paint the guide button.
    #[must_use]
    pub const fn draw(&self) -> LogicalRect {
        self.draw
    }

    /// Returns payload-specific eligibility computed by full prevalidation.
    #[must_use]
    pub const fn eligibility(&self) -> &DropGuideEligibility {
        &self.eligibility
    }
}

/// Payload-specific result of prevalidating one visible guide target.
#[derive(Debug, Clone, PartialEq)]
pub enum DropGuideEligibility {
    /// The exact target can produce a checked move command.
    Eligible,
    /// The exact target remains paintable but cannot accept this payload.
    Rejected(DropRejectionReason),
}

impl DropGuideEligibility {
    /// Returns whether this exact target can accept the queried payload.
    #[must_use]
    #[cfg(test)]
    pub const fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }
}

/// Fatal resolver failure which must roll back the containing engine boundary.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DropResolutionError {
    /// Candidate preparation exposed a core invariant or publication failure.
    #[error("drop command prevalidation failed unexpectedly: {0}")]
    UnexpectedPrevalidation(TransactionError),
    /// A compiled target record disagreed with its own stable structural id.
    #[error("presented drop target {target:?} has inconsistent command identity")]
    PresentedTargetIdentityMismatch {
        /// Structural id that disagreed with the compiled command proof.
        target: DropTargetId,
    },
    /// A successful transaction candidate could not reproduce its final edge preview.
    #[error("post-transaction drop preview projection failed: {detail}")]
    PreviewProjection {
        /// Internal projection failure retained for host diagnostics.
        detail: String,
    },
}

/// Private commit proof paired with renderer-visible target and preview geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDrop {
    scene: SurfaceSceneStamp,
    session: DragSessionId,
    source: MovePayload,
    target: DropTargetId,
    visual: DropVisual,
    mutation: PreparedDropMutation,
}

/// One immutable preflight result. Cloning a query shares its graph; only the
/// current release may take the uniquely owned draft into publication.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedDropMutation {
    base: WorkspaceVersion,
    policy: PolicyRevision,
    command: WorkspaceCommand,
    candidate: Arc<Workspace>,
    report: TransactionReport,
    tab_strip: Option<Arc<crate::presentation::compiler::FutureTabStrip>>,
}

impl PreparedDropMutation {
    pub(crate) fn new(
        base: WorkspaceVersion,
        policy: PolicyRevision,
        command: WorkspaceCommand,
        candidate: Workspace,
        report: TransactionReport,
    ) -> Self {
        Self {
            base,
            policy,
            command,
            candidate: Arc::new(candidate),
            report,
            tab_strip: None,
        }
    }

    fn with_preview_tab_strip(
        mut self,
        tab_strip: Option<Arc<crate::presentation::compiler::FutureTabStrip>>,
    ) -> Self {
        self.tab_strip = tab_strip;
        self
    }

    pub(crate) fn matches(
        &self,
        base: WorkspaceVersion,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
    ) -> bool {
        self.base == base && self.policy == policy.revision() && self.command == *command
    }

    pub(crate) fn into_owned_draft(self) -> Option<(Workspace, TransactionReport)> {
        Arc::try_unwrap(self.candidate)
            .ok()
            .map(|candidate| (candidate, self.report))
    }
}

impl ResolvedDrop {
    pub(crate) fn preview_tab_strip(
        &self,
    ) -> Option<Arc<crate::presentation::compiler::FutureTabStrip>> {
        self.mutation.tab_strip.clone()
    }

    /// Returns the structural target identity used to acknowledge this preview.
    #[must_use]
    pub const fn target_id(&self) -> DropTargetId {
        self.target
    }

    /// Returns exact preview geometry for renderer painting.
    #[must_use]
    pub const fn visual(&self) -> DropVisual {
        self.visual
    }

    /// Returns the exact prevalidated command carried through preview and release.
    #[must_use]
    pub const fn command(&self) -> &WorkspaceCommand {
        &self.mutation.command
    }

    pub(crate) const fn scene_stamp(&self) -> SurfaceSceneStamp {
        self.scene
    }

    pub(crate) const fn session(&self) -> DragSessionId {
        self.session
    }

    pub(crate) const fn source(&self) -> &MovePayload {
        &self.source
    }

    pub(crate) fn into_prepared_mutation(self) -> PreparedDropMutation {
        self.mutation
    }
}

/// Proof that a ready surface had no geometric target at an authoritative point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnownDropAbsence {
    scene: SurfaceSceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
}

impl KnownDropAbsence {
    const fn new(scene: SurfaceSceneStamp, surface: SurfaceId, point: LogicalPoint) -> Self {
        Self {
            scene,
            surface,
            point,
        }
    }
}

/// Why a sealed scene cannot answer a query for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropSurfaceUnavailable {
    /// The surface was absent from the scene's frozen active roster.
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
    MissingSurface,
    /// The surface was rostered but did not publish ready facts.
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
    Bootstrap,
    /// A compiled candidate exists but was not confirmed painted before reduction.
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
    PendingPaint,
    /// The surface retains a paint fallback but has no current hit authority.
    #[allow(
        dead_code,
        reason = "Retained internal protocol case is matched by the shared reducer and exercised by protocol tests"
    )]
    Stale,
}

/// Explicit unavailable result for an authoritative surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnavailableDrop {
    scene: Option<SurfaceSceneStamp>,
    surface: SurfaceId,
    reason: DropSurfaceUnavailable,
}

impl UnavailableDrop {
    #[cfg(test)]
    const fn new(
        scene: Option<SurfaceSceneStamp>,
        surface: SurfaceId,
        reason: DropSurfaceUnavailable,
    ) -> Self {
        Self {
            scene,
            surface,
            reason,
        }
    }

    /// Returns the exact unavailable classification.
    #[must_use]
    #[cfg(test)]
    pub const fn reason(&self) -> DropSurfaceUnavailable {
        self.reason
    }
}

/// Why one geometrically hit target was ineligible.
#[derive(Debug, Clone, PartialEq)]
pub enum DropRejectionReason {
    /// Scene construction explicitly retained an unavailable target.
    SceneUnavailable(DropTargetUnavailable),
    /// Current application policy rejects this operation class.
    Policy(PolicyRejection),
    /// The exact U3 command could not stage on a cloned candidate workspace.
    Prevalidation(TransactionError),
    /// Partial content targeted a rootless background without a frozen fresh root offer.
    SurfaceBackgroundRootOfferMissing,
}

/// Deterministic rejection for one structural hit candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct DropCandidateRejection {
    target: DropTargetId,
    reason: DropRejectionReason,
}

impl DropCandidateRejection {
    const fn new(target: DropTargetId, reason: DropRejectionReason) -> Self {
        Self { target, reason }
    }

    /// Returns the rejected structural target.
    #[must_use]
    pub const fn target_id(&self) -> DropTargetId {
        self.target
    }

    /// Returns the exact eligibility failure.
    #[must_use]
    #[cfg(test)]
    pub const fn reason(&self) -> &DropRejectionReason {
        &self.reason
    }
}

/// Rejection of the unique winner at one geometrically known point.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedDrop {
    scene: SurfaceSceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
    candidates: Vec<DropCandidateRejection>,
}

impl RejectedDrop {
    const fn new(
        scene: SurfaceSceneStamp,
        surface: SurfaceId,
        point: LogicalPoint,
        candidates: Vec<DropCandidateRejection>,
    ) -> Self {
        Self {
            scene,
            surface,
            point,
            candidates,
        }
    }

    /// Returns the sole winner rejection.
    #[must_use]
    pub fn candidates(&self) -> &[DropCandidateRejection] {
        &self.candidates
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod contract_tests;
