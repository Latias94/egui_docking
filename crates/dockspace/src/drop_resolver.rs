//! Deterministic resolution of authoritative drop locations.

mod preview;

use std::sync::Arc;

use crate::RootPresentationOwner;
use crate::command::{
    DockTarget, MovePayload, NodeSource, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideScope, DropGuideSlot,
};
use crate::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetKind, DropTargetLocation, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey, SurfaceBackground,
};
use crate::error::{CommandError, TransactionError};
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::graph::Workspace;
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId, WorkspaceRevision};
use crate::intent::SurfaceBackgroundRootOffer;
use crate::interaction::DragSessionId;
use crate::policy::{DockPolicySnapshot, PolicyRejection};
use crate::runtime::direct_manipulation::{
    DirectTargetCandidate, DirectTargetProvenance, TargetCandidate, TargetObservation,
};
use crate::scene::{
    PresentationLayoutFacts, PresentationPlan, SurfaceScene, SurfaceSceneSet, SurfaceSceneStamp,
};
use crate::scene_manifest::PolicyRevision;
use crate::transaction::TransactionReport;
#[cfg(test)]
use crate::transaction::WorkspaceTransaction;
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;
use thiserror::Error;

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
    resolve_presented_drop_with_magnetic(
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
        true,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the resolver binds every authoritative drag fact without an ambient context"
)]
pub(crate) fn resolve_presented_drop_with_magnetic(
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
    allow_magnetic: bool,
) -> Result<DropQuery, DropResolutionError> {
    let surface = stamp.surface();

    if !HitRegion::new(ready.bounds()).contains(point) {
        return Ok(DropQuery::new(
            DropResolution::KnownNone(KnownDropAbsence::new(stamp, surface, point)),
            None,
            TargetObservation::OutsideSurface,
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
            if allow_magnetic && matches!(direct, DropResolution::KnownNone(_)) {
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
    Option<(DropResolution, crate::target_field::MagneticTargetCandidate)>,
    DropResolutionError,
> {
    let layer = location
        .occluding_layer
        .unwrap_or_else(SceneLayerKey::surface_base);
    let Some(candidate) = location
        .ready
        .magnetic_target_at(location.point, layer, |target| {
            location
                .suppression
                .is_none_or(|entry| !entry.excludes_target(target))
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
            suppression.is_none_or(|entry| !entry.excludes_target(target.id()))
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
    suppression: Option<SourceSuppression>,
) -> Option<SceneLayerKey> {
    ready
        .drop_occlusions()
        .iter()
        .filter(|occlusion| {
            suppression.is_none_or(|entry| !entry.excludes_occlusion(occlusion.floating()))
                && occlusion.region().contains(point)
        })
        .map(|occlusion| occlusion.layer())
        .max()
}

fn select_guide_clusters(
    ready: &PresentationPlan,
    point: LogicalPoint,
    occluding_layer: Option<SceneLayerKey>,
    suppression: Option<SourceSuppression>,
) -> Vec<&DropGuideClusterRecord> {
    let visible = |cluster: &&DropGuideClusterRecord| {
        suppression.is_none_or(|entry| entry.root != cluster.id().root)
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
        .filter(|target| {
            location
                .suppression
                .is_none_or(|entry| !entry.excludes_target(target.id()))
        })
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
    topology: Result<crate::operation::DropCommandEligibility<'a>, CommandError>,
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
        let topology = crate::operation::DropCommandEligibility::new_indexed(
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

    fn source_suppression(&self) -> Option<SourceSuppression> {
        let source = self.complete_root_source.as_ref().ok()?.as_ref()?;
        SourceSuppression::from_source(self.workspace, source)
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
        match crate::operation::prepare_transaction(
            self.workspace,
            self.policy,
            std::slice::from_ref(&command),
        ) {
            Ok(prepared) => {
                let visual = preview::resolved_visual(
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
                    ),
                    visual,
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
    frozen: Option<&crate::command::NodeFingerprint>,
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

    /// Structural volume copied by one explicit core-owned workspace clone.
    ///
    /// This deliberately counts durable graph records and vector elements, not
    /// allocator calls or bytes. It is therefore deterministic across allocators
    /// and platforms while still exposing the size multiplied by each clone site.
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
                tab_mru_items: workspace.tab_mru.values().map(Vec::len).sum(),
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
                    Node::Tabs { items, .. } => volume.tab_items += items.len(),
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

    /// Calls and cumulative structural volume for one named deep-clone category.
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

    /// Explicit workspace clone sites owned by transaction preparation.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WorkspaceDeepCloneWork {
        pub(crate) engine_candidates: DeepCloneWork,
        pub(crate) transaction_candidates: DeepCloneWork,
    }

    /// Whole-engine candidate preparation. Workspace authority is shared;
    /// explicit mutation preparation owns the separate deep-copy counters.
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

    /// Test-only deterministic work performed by docking resolution and its
    /// transaction/authority dependencies.
    ///
    /// This is an explicit-call-site ledger, not a global `Clone` interceptor:
    /// caller-owned fixture clones and small field-local clones are intentionally
    /// outside its authority.
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
        pub(crate) workspace_full_validations: usize,
        pub(crate) item_multiset_scans: usize,
        pub(crate) item_multiset_item_visits: usize,
        pub(crate) transaction_prepares: usize,
        pub(crate) transaction_commands: usize,
        pub(crate) workspace_deep_clones: WorkspaceDeepCloneWork,
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
    suppression: Option<SourceSuppression>,
    surface_background_offer: Option<SurfaceBackgroundRootOffer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceSuppression {
    root: RootId,
    floating: Option<FloatingPresentationId>,
}

impl SourceSuppression {
    fn from_source(workspace: &Workspace, source: &NodeSource) -> Option<Self> {
        let root = source.root();
        let floating = match workspace.presentation_for_root(root)? {
            RootPresentationOwner::Main { .. } => None,
            RootPresentationOwner::Contained { floating, .. } => Some(floating),
        };
        Some(Self { root, floating })
    }

    fn excludes_target(self, target: DropTargetId) -> bool {
        drop_target_root(target).is_some_and(|root| self.root == root)
    }

    fn excludes_occlusion(self, floating: FloatingPresentationId) -> bool {
        matches!(self.floating, Some(source) if source == floating)
    }
}

const fn drop_target_root(target: DropTargetId) -> Option<RootId> {
    match target {
        DropTargetId::TabGap { root, .. }
        | DropTargetId::Center { root, .. }
        | DropTargetId::InnerEdge { root, .. }
        | DropTargetId::OuterEdge { root, .. } => Some(root),
        DropTargetId::SurfaceBackground { .. } => None,
    }
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
    pub const fn resolution(&self) -> &DropResolution {
        &self.resolution
    }

    /// Returns complete guide painting facts when a cluster is active.
    #[must_use]
    pub const fn affordance(&self) -> Option<&DropAffordance> {
        self.affordance.as_ref()
    }

    /// Consumes the query into its independent resolution and affordance values.
    #[must_use]
    pub fn into_parts(self) -> (DropResolution, Option<DropAffordance>) {
        (self.resolution, self.affordance)
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
    ) -> Self {
        Self {
            scene,
            target_layout_basis: scene,
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

    /// Returns the authoritative logical surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact authoritative point used for the query.
    #[must_use]
    pub const fn point(&self) -> LogicalPoint {
        self.point
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

    /// Returns the owning cluster identity.
    #[must_use]
    pub const fn cluster(self) -> DropGuideClusterId {
        self.cluster
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

    /// Returns the owning cluster identity.
    #[must_use]
    pub const fn cluster(&self) -> DropGuideClusterId {
        self.key.cluster()
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
    pub const fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }

    /// Returns the exact shared resolver rejection, if any.
    #[must_use]
    pub const fn rejection(&self) -> Option<&DropRejectionReason> {
        match self {
            Self::Eligible => None,
            Self::Rejected(reason) => Some(reason),
        }
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
        }
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

    /// Returns the sealed scene queried by the resolver.
    #[must_use]
    pub const fn scene(&self) -> SurfaceSceneStamp {
        self.scene
    }

    /// Returns the authoritative surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the authoritative logical point.
    #[must_use]
    pub const fn point(&self) -> LogicalPoint {
        self.point
    }
}

/// Why a sealed scene cannot answer a query for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropSurfaceUnavailable {
    /// The surface was absent from the scene's frozen active roster.
    MissingSurface,
    /// The surface was rostered but did not publish ready facts.
    Bootstrap,
    /// A compiled candidate exists but was not confirmed painted before reduction.
    PendingPaint,
    /// The surface retains a paint fallback but has no current hit authority.
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

    /// Returns the sealed scene queried by the resolver.
    #[must_use]
    pub const fn scene(&self) -> Option<SurfaceSceneStamp> {
        self.scene
    }

    /// Returns the unavailable authoritative surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact unavailable classification.
    #[must_use]
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

    /// Returns the sealed scene queried by the resolver.
    #[must_use]
    pub const fn scene(&self) -> SurfaceSceneStamp {
        self.scene
    }

    /// Returns the authoritative surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the authoritative logical point.
    #[must_use]
    pub const fn point(&self) -> LogicalPoint {
        self.point
    }

    /// Returns the sole winner rejection.
    #[must_use]
    pub fn candidates(&self) -> &[DropCandidateRejection] {
        &self.candidates
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::command::{CommandOutcome, DockFraction, Edge};
    use crate::drop_guide::{
        DropGuideEdgeSet, DropGuideScope, DropGuideSlot, DropGuideTargetRecord,
    };
    use crate::drop_target::{
        DropOcclusionRecord, DropTargetAvailability, DropTargetUnavailable, SurfaceBackground,
    };
    use crate::engine::{DockEngine, PreparedSurfacePaintCandidate};
    use crate::geometry::{LogicalRect, LogicalSize};
    use crate::graph::{
        Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, WorkspaceBuilder,
    };
    use crate::ids::{ItemId, NodeId, WorkspaceEpoch};
    use crate::policy::{DockPolicy, DockPresentationMode, PolicyRevision};
    use crate::presentation_observation::{PresentationOutputSerial, PresentedSurfaceAuthority};
    use crate::scene::{
        ContainedMinimumMeasurement, PresentationPlanValidator, SurfaceCoordinateCapture,
    };
    use crate::scene_manifest::{
        Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, TabIntrinsic, TabStripMetrics,
    };
    use crate::viewport::CoordinateGeneration;

    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
    const COMPLETE_SURFACE: SurfaceId = SurfaceId::new(2);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(3);
    const CROSS_TARGET_SURFACE: SurfaceId = SurfaceId::new(4);
    const SOURCE_ROOT: RootId = RootId::new(11);
    const COMPLETE_ROOT: RootId = RootId::new(12);
    const CONTAINED_ROOT: RootId = RootId::new(13);
    const OFFERED_ROOT: RootId = RootId::new(14);
    const CROSS_CONTAINED_ROOT: RootId = RootId::new(15);
    const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(21);
    const CROSS_CONTAINED: FloatingPresentationId = FloatingPresentationId::new(22);
    const PROMOTION_BACK_ROOT: RootId = RootId::new(31);
    const PROMOTION_ROOT: RootId = RootId::new(32);
    const PROMOTION_FRONT_ROOT: RootId = RootId::new(33);
    const PROMOTION_BACK: FloatingPresentationId = FloatingPresentationId::new(41);
    const PROMOTION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(42);
    const PROMOTION_FRONT: FloatingPresentationId = FloatingPresentationId::new(43);

    struct Fixture {
        workspace: Workspace,
        source_tabs: NodeId,
        complete_tabs: NodeId,
        contained_tabs: NodeId,
    }

    fn rect() -> LogicalRect {
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test rectangle must be valid")
    }

    fn contained_rect() -> LogicalRect {
        LogicalRect::new(500.0, 350.0, 200.0, 150.0).expect("contained rectangle must be valid")
    }

    fn cross_contained_rect() -> LogicalRect {
        LogicalRect::new(600.0, 50.0, 140.0, 120.0).expect("cross contained rect must be valid")
    }

    fn contained_minimum(floating: FloatingPresentationId) -> ContainedMinimumMeasurement {
        ContainedMinimumMeasurement::new(
            floating,
            LogicalSize::new(120.0, 90.0).expect("contained minimum must be valid"),
        )
    }

    fn fixture() -> Fixture {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        let source_peer = builder.insert_node(Node::tabs([ItemId::new(5)]));
        let source_root = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [source_tabs, source_peer])
                .expect("source split must be valid"),
        );
        let complete_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
        let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
        let cross_contained_tabs = builder.insert_node(Node::tabs([ItemId::new(6)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_root));
        builder.set_root(COMPLETE_ROOT, RootRecord::new(complete_tabs));
        builder.set_root(CONTAINED_ROOT, RootRecord::new(contained_tabs));
        builder.set_root(CROSS_CONTAINED_ROOT, RootRecord::new(cross_contained_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
        builder.set_surface(
            COMPLETE_SURFACE,
            SurfacePresentation::with_main(COMPLETE_ROOT),
        );
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
        builder.set_surface(CROSS_TARGET_SURFACE, SurfacePresentation::rootless());
        builder.set_contained_floating(
            CONTAINED,
            ContainedFloating::new(CONTAINED_ROOT, contained_rect()),
        );
        builder.set_contained_floating(
            CROSS_CONTAINED,
            ContainedFloating::new(CROSS_CONTAINED_ROOT, cross_contained_rect()),
        );
        builder
            .attach_contained(TARGET_SURFACE, CONTAINED)
            .expect("target surface exists");
        builder
            .attach_contained(CROSS_TARGET_SURFACE, CROSS_CONTAINED)
            .expect("cross target surface exists");
        Fixture {
            workspace: builder.build().expect("fixture workspace must be valid"),
            source_tabs,
            complete_tabs,
            contained_tabs,
        }
    }

    fn session() -> DragSessionId {
        DragSessionId::new(
            WorkspaceEpoch::new(1),
            crate::interaction::DragGeneration::new(1),
        )
    }

    fn bind_measurement_ticket(
        plan: &PresentationPlan,
        ticket: SurfaceMeasurementTicket,
    ) -> PresentationPlan {
        plan.rebind_measurement_ticket_for_test(ticket)
    }

    fn fixture_measurements(
        engine: &DockEngine,
        surface: SurfaceId,
        bounds: LogicalRect,
    ) -> SurfaceMeasurements {
        let requirements = engine
            .presentation_requirements()
            .surface(surface)
            .expect("fixture surface requirements must exist");
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
            .expect("fixture surface bounds answer must be unique");
        if let Some(key) = requirements.popup_plane_bounds() {
            measurements
                .set_popup_plane_bounds(key, Measurement::Measured(bounds))
                .expect("fixture popup-plane bounds answer must be unique");
        }
        let minimum = LogicalSize::new(0.0, 0.0).expect("fixture minimum must be valid");
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(minimum))
                .expect("fixture pane minimum answer must be unique");
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(
                    key,
                    Measurement::Measured(
                        TabIntrinsic::new(56.0).expect("fixture tab intrinsic must be valid"),
                    ),
                )
                .expect("fixture tab intrinsic answer must be unique");
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        TabStripMetrics::new(0.0, 0.0)
                            .expect("fixture tab strip metrics must be valid"),
                    ),
                )
                .expect("fixture tab strip answer must be unique");
        }
        measurements
    }

    fn compile_fixture_surface_plan(
        engine: &DockEngine,
        surface: SurfaceId,
        bounds: LogicalRect,
    ) -> PresentationPlan {
        let token = engine
            .begin_surface_contribution(surface)
            .expect("fixture surface must be in the presentation roster");
        let contribution = engine
            .prepare_surface_contribution(token, fixture_measurements(engine, surface, bounds))
            .expect("fixture measurements must compile through the production path");
        match contribution.paint_candidate() {
            PreparedSurfacePaintCandidate::Ready(candidate) => candidate.plan().clone(),
            PreparedSurfacePaintCandidate::Retained { .. }
            | PreparedSurfacePaintCandidate::Unavailable { .. } => {
                panic!("fixture measurements must produce a ready paint candidate")
            }
        }
    }

    fn compile_fixture_surface_plan_without_guides(
        engine: &DockEngine,
        surface: SurfaceId,
        bounds: LogicalRect,
    ) -> PresentationPlan {
        let plan = crate::scene_compiler::compile_surface_measurements_without_guides(
            engine.workspace(),
            engine.version(),
            engine.policy_snapshot(),
            engine.presentation_config(),
            engine.presentation_requirements(),
            &fixture_measurements(engine, surface, bounds),
            &crate::tab_strip::TabStripStateStore::default(),
            &[],
            None,
        )
        .expect("guide-disabled fixture measurements must compile");
        PresentationPlanValidator::new(engine.workspace(), engine.policy_snapshot())
            .expect("guide-disabled fixture validator must initialize")
            .validate_and_canonicalize(plan)
            .expect("guide-disabled fixture plan must validate")
    }

    fn ready_background(extra: Option<DropTargetRecord>) -> PresentationPlan {
        ready_background_in(rect(), extra)
    }

    fn ready_background_in(
        surface_bounds: LogicalRect,
        extra: Option<DropTargetRecord>,
    ) -> PresentationPlan {
        let mut ready = PresentationPlan::new(TARGET_SURFACE, surface_bounds);
        ready
            .set_surface_background(DropTargetRecord::surface_background(
                SurfaceBackground::new(TARGET_SURFACE),
                HitRegion::new(surface_bounds),
                DropVisual::new(surface_bounds),
            ))
            .expect("background record is valid");
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            CONTAINED,
            HitRegion::new(contained_rect()),
            SceneLayerKey::new(2),
        ));
        ready.push_contained_minimum(contained_minimum(CONTAINED));
        if let Some(extra) = extra {
            ready.push_drop_target(extra.into());
        }
        ready
    }

    fn cross_surface_background(extra: Option<DropTargetRecord>) -> PresentationPlan {
        let mut ready = PresentationPlan::new(CROSS_TARGET_SURFACE, rect());
        ready
            .set_surface_background(DropTargetRecord::surface_background(
                SurfaceBackground::new(CROSS_TARGET_SURFACE),
                HitRegion::new(rect()),
                DropVisual::new(rect()),
            ))
            .expect("background record is valid");
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            CROSS_CONTAINED,
            HitRegion::new(cross_contained_rect()),
            SceneLayerKey::new(2),
        ));
        ready.push_contained_minimum(contained_minimum(CROSS_CONTAINED));
        if let Some(extra) = extra {
            ready.push_drop_target(extra.into());
        }
        ready
    }

    fn seal(fixture: &Fixture, ready: PresentationPlan) -> SurfaceSceneSet {
        seal_workspace(&fixture.workspace, ready)
    }

    fn seal_workspace(workspace: &Workspace, ready: PresentationPlan) -> SurfaceSceneSet {
        let surface = ready.surface();
        let policy = DockPolicySnapshot::default();
        let mut ready = ready;
        if ready.surface_program().blocks().is_empty() {
            ready
                .finish_surface_program()
                .expect("synthetic fixture program should finish exactly once");
        }
        let ready = PresentationPlanValidator::new(workspace, &policy)
            .expect("fixture validator must initialize")
            .validate_and_canonicalize(ready)
            .expect("scene must validate");
        let engine = DockEngine::new(workspace.clone(), policy.to_policy())
            .expect("fixture engine is valid");
        let mut scenes = engine.scene().clone();
        let roster: Vec<_> = engine
            .presentation_requirements()
            .surfaces()
            .map(|(surface, requirements)| (surface, requirements.ticket()))
            .collect();
        let mut outputs = Vec::with_capacity(roster.len());
        for (index, (rostered_surface, ticket)) in roster.iter().copied().enumerate() {
            let plan = if rostered_surface == surface {
                bind_measurement_ticket(&ready, ticket)
            } else {
                compile_fixture_surface_plan(&engine, rostered_surface, ready.bounds())
            };
            let serial = u64::try_from(index + 1).expect("fixture presentation serial must fit");
            let (_stamp, output_ticket) = scenes
                .install_ready(
                    plan,
                    SurfaceCoordinateCapture::Headless {
                        authority_generation: CoordinateGeneration::default(),
                    },
                    ticket.authority_domain(),
                    PresentationOutputSerial::new_for_test(serial),
                )
                .expect("validated candidate must install");
            outputs.push((output_ticket, serial));
        }
        for (output_ticket, serial) in outputs {
            let authority = PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
                output_ticket,
                CoordinateGeneration::default(),
                serial,
            );
            assert!(scenes.accept_observed_authority(authority).is_ok());
        }
        scenes
    }

    fn partial_payload(fixture: &Fixture) -> MovePayload {
        MovePayload::Item(
            fixture
                .workspace
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
                .expect("partial source must be current"),
        )
    }

    fn guide_target_record(
        targets: &mut Vec<Arc<DropTargetRecord>>,
        location: DropTargetLocation,
    ) -> DropGuideTargetRecord {
        let target = Arc::new(DropTargetRecord::new(
            location,
            DropTargetAvailability::Available,
            HitRegion::new(rect()),
            SceneLayerKey::new(2),
            DropVisual::new(rect()),
        ));
        targets.push(Arc::clone(&target));
        DropGuideTargetRecord::new(target, rect())
    }

    fn complete_guide_clusters(
        fixture: &Fixture,
    ) -> (Vec<Arc<DropTargetRecord>>, [DropGuideClusterRecord; 2]) {
        let fraction = DockFraction::new(0.35).expect("guide fraction must be valid");
        let mut targets = Vec::new();
        let center = guide_target_record(
            &mut targets,
            DropTargetLocation::Center {
                surface: TARGET_SURFACE,
                root: CONTAINED_ROOT,
                tabs: fixture.contained_tabs,
            },
        );
        let inner_edges = {
            let mut inner_edge = |edge| {
                guide_target_record(
                    &mut targets,
                    DropTargetLocation::InnerEdge {
                        surface: TARGET_SURFACE,
                        root: CONTAINED_ROOT,
                        node: fixture.contained_tabs,
                        edge,
                        fraction,
                    },
                )
            };
            DropGuideEdgeSet::new(
                inner_edge(Edge::Left),
                inner_edge(Edge::Right),
                inner_edge(Edge::Top),
                inner_edge(Edge::Bottom),
            )
        };
        let outer_edges = {
            let mut outer_edge = |edge| {
                guide_target_record(
                    &mut targets,
                    DropTargetLocation::OuterEdge {
                        surface: TARGET_SURFACE,
                        root: CONTAINED_ROOT,
                        node: fixture.contained_tabs,
                        edge,
                        fraction,
                    },
                )
            };
            DropGuideEdgeSet::new(
                outer_edge(Edge::Left),
                outer_edge(Edge::Right),
                outer_edge(Edge::Top),
                outer_edge(Edge::Bottom),
            )
        };
        let inner = DropGuideClusterRecord::inner(
            TARGET_SURFACE,
            CONTAINED_ROOT,
            fixture.contained_tabs,
            HitRegion::new(rect()),
            SceneLayerKey::new(2),
            center,
            inner_edges,
        );
        let outer = DropGuideClusterRecord::outer(
            TARGET_SURFACE,
            CONTAINED_ROOT,
            HitRegion::new(rect()),
            SceneLayerKey::new(2),
            outer_edges,
        );
        (targets, [outer, inner])
    }

    fn install_complete_guide_targets(
        fixture: &Fixture,
        mut ready: PresentationPlan,
    ) -> (PresentationPlan, [DropGuideClusterRecord; 2]) {
        let (targets, clusters) = complete_guide_clusters(fixture);
        for target in targets {
            ready.push_drop_target(target);
        }
        (ready, clusters)
    }

    #[test]
    fn complete_guide_affordance_preflights_only_the_unique_winner() {
        let fixture = fixture();
        let (ready, clusters) = install_complete_guide_targets(&fixture, ready_background(None));
        let scene = seal(&fixture, ready);
        let stamp = scene
            .ready_surface(TARGET_SURFACE)
            .expect("target scene must be ready")
            .stamp();
        let payload = partial_payload(&fixture);
        let policy = DockPolicySnapshot::default();
        let workspace_version = WorkspaceVersion::initial();
        let workspace_index = WorkspaceIndex::build(&fixture.workspace, workspace_version)
            .expect("fixture index must build");
        let assessment = DropEligibilityContext::new(
            &fixture.workspace,
            workspace_version,
            &workspace_index,
            &policy,
            scene
                .ready_surface(TARGET_SURFACE)
                .expect("target scene must be ready")
                .plan(),
            None,
            &payload,
            None,
        );

        reset_resolution_work();
        let (_, exact) = build_affordance(
            stamp,
            TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
            clusters.iter().collect(),
            &assessment,
        )
        .expect("guide resolution cannot expose an invariant");

        assert!(matches!(
            exact,
            Some(ExactGuidePreparation::Eligible { .. })
        ));
        let work = resolution_work();
        assert_eq!(work.drop_targets_assessed, 9);
        assert_eq!(work.geometric_winners, 1);
        assert_eq!(work.transaction_prepares, 1);
        assert_eq!(work.transaction_commands, 1);
        assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
        // The winning command uses actual root issuance and builds no value proof.
        assert_eq!(work.root_fingerprint_builds, 0);
        assert_eq!(work.root_fingerprint_node_visits, 0);
    }

    #[test]
    fn rejected_geometric_winner_never_falls_through_or_preflights() {
        let fixture = fixture();
        let (ready, clusters) = install_complete_guide_targets(&fixture, ready_background(None));
        let scene = seal(&fixture, ready);
        let stamp = scene
            .ready_surface(TARGET_SURFACE)
            .expect("target scene must be ready")
            .stamp();
        let payload = partial_payload(&fixture);
        let mut policy = DockPolicy::default();
        policy.set_allow_edge_split(false);
        let policy = policy.snapshot(PolicyRevision::default());
        let workspace_version = WorkspaceVersion::initial();
        let workspace_index = WorkspaceIndex::build(&fixture.workspace, workspace_version)
            .expect("fixture index must build");
        let assessment = DropEligibilityContext::new(
            &fixture.workspace,
            workspace_version,
            &workspace_index,
            &policy,
            scene
                .ready_surface(TARGET_SURFACE)
                .expect("target scene must be ready")
                .plan(),
            None,
            &payload,
            None,
        );

        reset_resolution_work();
        let (_, exact) = build_affordance(
            stamp,
            TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
            clusters.iter().collect(),
            &assessment,
        )
        .expect("guide resolution cannot expose an invariant");

        assert!(matches!(
            exact,
            Some(ExactGuidePreparation::Rejected {
                target: DropTargetId::OuterEdge {
                    edge: Edge::Left,
                    ..
                },
                reason: DropRejectionReason::Policy(PolicyRejection::EdgeSplitDisabled),
                ..
            })
        ));
        let work = resolution_work();
        assert_eq!(work.drop_targets_assessed, 9);
        assert_eq!(work.geometric_winners, 1);
        assert_eq!(work.transaction_prepares, 0);
        assert_eq!(work.transaction_commands, 0);
        assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 0);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum WorkloadDistribution {
        Balanced,
        HotRootSkew,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ResolutionWorkloadSpec {
        panes: usize,
        surfaces: usize,
        distribution: WorkloadDistribution,
    }

    struct ResolutionWorkload {
        workspace: Workspace,
        workspace_version: WorkspaceVersion,
        workspace_index: WorkspaceIndex,
        scene: SurfaceSceneSet,
        payload: MovePayload,
        offer: SurfaceBackgroundRootOffer,
        expected_clone_volume: structural_work::WorkspaceCloneVolume,
        source_root_nodes: usize,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ResolutionWorkBaseline {
        spec: ResolutionWorkloadSpec,
        root_fingerprint_builds: usize,
        root_fingerprint_node_visits: usize,
        source_root_nodes: usize,
        transaction_clone_nodes: usize,
    }

    fn pane_distribution(spec: ResolutionWorkloadSpec) -> Vec<usize> {
        assert!(spec.surfaces > 0);
        assert!(spec.panes >= spec.surfaces * 2);
        match spec.distribution {
            WorkloadDistribution::Balanced => {
                let base = spec.panes / spec.surfaces;
                let remainder = spec.panes % spec.surfaces;
                (0..spec.surfaces)
                    .map(|index| base + usize::from(index < remainder))
                    .collect()
            }
            WorkloadDistribution::HotRootSkew => {
                let mut counts = vec![2; spec.surfaces];
                counts[0] += spec.panes - spec.surfaces * 2;
                counts
            }
        }
    }

    fn insert_balanced_root(
        builder: &mut WorkspaceBuilder,
        pane_count: usize,
        next_item: &mut u64,
    ) -> (NodeId, NodeId, ItemId) {
        let mut level = Vec::with_capacity(pane_count);
        let mut source = None;
        for _ in 0..pane_count {
            let item = ItemId::new(*next_item);
            *next_item += 1;
            let tabs = builder.insert_node(Node::tabs([item]));
            source.get_or_insert((tabs, item));
            level.push(tabs);
        }

        let mut depth = 0;
        while level.len() > 1 {
            let mut parents = Vec::with_capacity(level.len().div_ceil(2));
            for pair in level.chunks(2) {
                if let [left, right] = pair {
                    let axis = if depth % 2 == 0 {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    };
                    parents.push(
                        builder.insert_node(
                            Node::equal_split(axis, [*left, *right])
                                .expect("balanced workload split must be valid"),
                        ),
                    );
                } else {
                    parents.push(pair[0]);
                }
            }
            level = parents;
            depth += 1;
        }

        let (source_tabs, source_item) = source.expect("workload root must contain a pane");
        (level[0], source_tabs, source_item)
    }

    fn build_resolution_workload(spec: ResolutionWorkloadSpec) -> ResolutionWorkload {
        let pane_counts = pane_distribution(spec);
        let mut builder = Workspace::builder();
        let target_surface = TARGET_SURFACE;
        builder.set_surface(target_surface, SurfacePresentation::rootless());

        let mut next_item = 1_u64;
        let mut source = None;
        for (index, pane_count) in pane_counts.iter().copied().enumerate() {
            let index = u64::try_from(index).expect("workload index must fit u64");
            let root = RootId::new(100 + index);
            let (root_node, source_tabs, source_item) =
                insert_balanced_root(&mut builder, pane_count, &mut next_item);
            builder.set_root(root, RootRecord::new(root_node));
            if index == 0 {
                let floating = FloatingPresentationId::new(100);
                builder.set_contained_floating(
                    floating,
                    ContainedFloating::new(root, contained_rect()),
                );
                builder
                    .attach_contained(target_surface, floating)
                    .expect("target surface must exist");
                source = Some((root, source_tabs, source_item));
            } else {
                builder.set_surface(
                    SurfaceId::new(100 + index),
                    SurfacePresentation::with_main(root),
                );
            }
        }
        let workspace = builder.build().expect("scale workload must be valid");
        let (source_root, source_tabs, source_item) =
            source.expect("scale workload must contain a source root");
        let payload = MovePayload::Item(
            workspace
                .capture_item_source(source_root, source_tabs, source_item)
                .expect("scale workload source must be current"),
        );
        let source_root_nodes = match &payload {
            MovePayload::Item(source) => workspace
                .root_fingerprint(source.root())
                .unwrap()
                .0
                .nodes
                .len(),
            MovePayload::Tabs(_) | MovePayload::Subtree(_) => {
                unreachable!("the scale workload always moves one item")
            }
        };
        let expected_clone_volume = structural_work::WorkspaceCloneVolume::capture(&workspace);

        let mut ready = PresentationPlan::new(target_surface, rect());
        ready
            .set_surface_background(DropTargetRecord::surface_background(
                SurfaceBackground::new(target_surface),
                HitRegion::new(rect()),
                DropVisual::new(rect()),
            ))
            .expect("scale workload background must be unique");
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            FloatingPresentationId::new(100),
            HitRegion::new(contained_rect()),
            SceneLayerKey::new(2),
        ));
        ready.push_contained_minimum(contained_minimum(FloatingPresentationId::new(100)));
        let scene = seal_workspace(&workspace, ready);
        let workspace_version = WorkspaceVersion::initial();
        let workspace_index = WorkspaceIndex::build(&workspace, workspace_version)
            .expect("scale workload index must build");

        ResolutionWorkload {
            workspace,
            workspace_version,
            workspace_index,
            scene,
            payload,
            offer: SurfaceBackgroundRootOffer::new(RootId::new(1_000_000)),
            expected_clone_volume,
            source_root_nodes,
        }
    }

    fn run_resolution_workload(spec: ResolutionWorkloadSpec) -> ResolutionWorkBaseline {
        let workload = build_resolution_workload(spec);
        reset_resolution_work();

        let ready = workload
            .scene
            .ready_surface(TARGET_SURFACE)
            .expect("scale workload target must be presented");
        let query = resolve_presented_drop(
            ready.stamp(),
            ready.plan(),
            None,
            &workload.workspace,
            workload.workspace_version,
            &workload.workspace_index,
            &DockPolicySnapshot::default(),
            session(),
            workload.payload,
            Some(workload.offer),
            LogicalPoint::new(10.0, 10.0).expect("workload point must be finite"),
        )
        .expect("scale workload must not expose an invariant");
        let DropResolution::Resolved(resolved) = query.resolution() else {
            panic!("scale workload must resolve its background target");
        };
        assert!(matches!(
            resolved.command(),
            WorkspaceCommand::InstallMainRoot { .. }
        ));

        let work = resolution_work();
        assert_eq!(work.drop_targets_assessed, 1);
        assert_eq!(work.geometric_winners, 1);
        assert_eq!(work.transaction_prepares, 1);
        assert_eq!(work.transaction_commands, 1);
        assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
        let clone_volume = work.workspace_deep_clones.transaction_candidates.volume;
        assert_eq!(clone_volume, workload.expected_clone_volume);
        assert_eq!(clone_volume.tab_items, spec.panes);
        assert_eq!(clone_volume.tab_mru_items, spec.panes);
        assert_eq!(clone_volume.roots, spec.surfaces);
        assert_eq!(clone_volume.surfaces, spec.surfaces);
        assert_eq!(clone_volume.contained_records, 1);
        assert_eq!(clone_volume.contained_roster_entries, 1);
        // Cold InstallMainRoot snapshots its donor before/after plus birth (3),
        // then full canonical completion snapshots all R+1 roots twice.
        assert_eq!(work.root_fingerprint_builds, 2 * spec.surfaces + 5);
        // Removing this fixture's first singleton leaf collapses its binary
        // parent. The new root adds one leaf, so canonical volume is V-1.
        assert_eq!(
            resolved.mutation.candidate.nodes().count(),
            clone_volume.nodes - 1
        );
        // Donor before/after: 2S. Pre-normalization forest: V+1.
        // Post-normalization forest: V-1. Birth: 1. Total: 2S+2V+1.
        assert_eq!(
            work.root_fingerprint_node_visits,
            2 * workload.source_root_nodes + 2 * clone_volume.nodes + 1
        );

        ResolutionWorkBaseline {
            spec,
            root_fingerprint_builds: work.root_fingerprint_builds,
            root_fingerprint_node_visits: work.root_fingerprint_node_visits,
            source_root_nodes: workload.source_root_nodes,
            transaction_clone_nodes: work
                .workspace_deep_clones
                .transaction_candidates
                .volume
                .nodes,
        }
    }

    #[test]
    fn structural_drop_work_has_deterministic_scale_baselines() {
        let specs = [
            ResolutionWorkloadSpec {
                panes: 16,
                surfaces: 1,
                distribution: WorkloadDistribution::Balanced,
            },
            ResolutionWorkloadSpec {
                panes: 128,
                surfaces: 8,
                distribution: WorkloadDistribution::Balanced,
            },
            ResolutionWorkloadSpec {
                panes: 128,
                surfaces: 8,
                distribution: WorkloadDistribution::HotRootSkew,
            },
            ResolutionWorkloadSpec {
                panes: 1_024,
                surfaces: 32,
                distribution: WorkloadDistribution::Balanced,
            },
            ResolutionWorkloadSpec {
                panes: 1_024,
                surfaces: 32,
                distribution: WorkloadDistribution::HotRootSkew,
            },
        ];
        let baselines = specs.map(run_resolution_workload);

        assert_eq!(
            baselines,
            [
                ResolutionWorkBaseline {
                    spec: specs[0],
                    root_fingerprint_builds: 7,
                    root_fingerprint_node_visits: 125,
                    source_root_nodes: 31,
                    transaction_clone_nodes: 31,
                },
                ResolutionWorkBaseline {
                    spec: specs[1],
                    root_fingerprint_builds: 21,
                    root_fingerprint_node_visits: 559,
                    source_root_nodes: 31,
                    transaction_clone_nodes: 248,
                },
                ResolutionWorkBaseline {
                    spec: specs[2],
                    root_fingerprint_builds: 21,
                    root_fingerprint_node_visits: 947,
                    source_root_nodes: 226,
                    transaction_clone_nodes: 247,
                },
                ResolutionWorkBaseline {
                    spec: specs[3],
                    root_fingerprint_builds: 69,
                    root_fingerprint_node_visits: 4_159,
                    source_root_nodes: 63,
                    transaction_clone_nodes: 2_016,
                },
                ResolutionWorkBaseline {
                    spec: specs[4],
                    root_fingerprint_builds: 69,
                    root_fingerprint_node_visits: 7_875,
                    source_root_nodes: 1_922,
                    transaction_clone_nodes: 2_015,
                },
            ]
        );
        assert!(baselines[2].source_root_nodes > baselines[1].source_root_nodes);
        assert!(baselines[4].source_root_nodes > baselines[3].source_root_nodes);
        assert!(baselines[3].transaction_clone_nodes > baselines[1].transaction_clone_nodes);
    }

    fn complete_payloads(fixture: &Fixture) -> Vec<MovePayload> {
        let node_source = || {
            fixture
                .workspace
                .capture_node_source(COMPLETE_ROOT, fixture.complete_tabs)
                .expect("complete source must be current")
        };
        vec![
            MovePayload::Item(
                fixture
                    .workspace
                    .capture_item_source(COMPLETE_ROOT, fixture.complete_tabs, ItemId::new(3))
                    .expect("complete item source must be current"),
            ),
            MovePayload::Tabs(node_source()),
            MovePayload::Subtree(node_source()),
        ]
    }

    fn contained_complete_payloads(fixture: &Fixture) -> Vec<MovePayload> {
        let node_source = || {
            fixture
                .workspace
                .capture_node_source(CONTAINED_ROOT, fixture.contained_tabs)
                .expect("complete contained source must be current")
        };
        vec![
            MovePayload::Item(
                fixture
                    .workspace
                    .capture_item_source(CONTAINED_ROOT, fixture.contained_tabs, ItemId::new(4))
                    .expect("complete contained item source must be current"),
            ),
            MovePayload::Tabs(node_source()),
            MovePayload::Subtree(node_source()),
        ]
    }

    fn partial_payloads(fixture: &Fixture) -> Vec<MovePayload> {
        let node_source = || {
            fixture
                .workspace
                .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
                .expect("partial node source must be current")
        };
        vec![
            partial_payload(fixture),
            MovePayload::Tabs(node_source()),
            MovePayload::Subtree(node_source()),
        ]
    }

    fn resolve(
        fixture: &Fixture,
        scene: &SurfaceSceneSet,
        payload: MovePayload,
        offer: Option<SurfaceBackgroundRootOffer>,
    ) -> DropResolution {
        resolve_unguided_drop(
            scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            payload,
            offer,
            TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        )
        .expect("resolution cannot expose an invariant")
    }

    fn assert_same_surface_promotion(
        workspace: &Workspace,
        scene: &SurfaceSceneSet,
        payload: MovePayload,
        offer: Option<SurfaceBackgroundRootOffer>,
    ) {
        let resolution = resolve_unguided_drop(
            scene,
            workspace,
            &DockPolicySnapshot::default(),
            session(),
            payload,
            offer,
            TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        )
        .expect("promotion resolution cannot expose an invariant");
        let DropResolution::Resolved(resolved) = resolution else {
            panic!("same-surface complete root must resolve as a promotion");
        };
        assert_eq!(
            resolved.target_id(),
            DropTargetId::SurfaceBackground {
                surface: TARGET_SURFACE,
            }
        );
        assert!(matches!(
            resolved.command(),
            WorkspaceCommand::PromoteContained {
                source,
                surface: TARGET_SURFACE,
                floating: PROMOTION_FLOATING,
            } if source.root() == PROMOTION_ROOT
        ));

        let mut candidate = workspace.clone();
        let report = WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut candidate, &DockPolicySnapshot::default())
            .expect("resolved promotion must commit");
        assert_eq!(
            report.outcomes(),
            &[CommandOutcome::ContainedPromoted {
                surface: TARGET_SURFACE,
                root: PROMOTION_ROOT,
                floating: PROMOTION_FLOATING,
            }]
        );
        assert_eq!(
            candidate.surface(TARGET_SURFACE),
            Some(&SurfacePresentation {
                main_root: Some(PROMOTION_ROOT),
                contained: vec![PROMOTION_BACK, PROMOTION_FRONT],
            })
        );
        assert!(candidate.contained_floating(PROMOTION_FLOATING).is_none());
        assert!(candidate.contained_floating(PROMOTION_BACK).is_some());
        assert!(candidate.contained_floating(PROMOTION_FRONT).is_some());
        assert!(candidate.root(PROMOTION_ROOT).is_some());
    }

    #[test]
    fn complete_root_background_delivery_preserves_root_identity() {
        let fixture = fixture();
        let scene = seal(&fixture, ready_background(None));
        for payload in complete_payloads(&fixture) {
            let DropResolution::Resolved(resolved) = resolve(&fixture, &scene, payload, None)
            else {
                panic!("every complete-root payload must resolve without an offer");
            };
            assert!(matches!(
                resolved.command(),
                WorkspaceCommand::RehomeRoot {
                    target: RootPresentationTarget::Main {
                        surface: TARGET_SURFACE
                    },
                    ..
                }
            ));
            let mut candidate = fixture.workspace.clone();
            WorkspaceTransaction::from_commands([resolved.command().clone()])
                .apply(&mut candidate, &DockPolicySnapshot::default())
                .expect("resolved command must commit");
            assert_eq!(
                candidate
                    .surface(TARGET_SURFACE)
                    .expect("target remains")
                    .main_root,
                Some(COMPLETE_ROOT)
            );
        }
    }

    #[test]
    fn background_preflight_preserves_typed_policy_rejection() {
        let fixture = fixture();
        let scene = seal(&fixture, ready_background(None));
        let mut policy = DockPolicy::default();
        policy.set_allow_tiled_presentation(false);
        let policy = policy.snapshot(PolicyRevision::default());
        let resolution = resolve_unguided_drop(
            &scene,
            &fixture.workspace,
            &policy,
            session(),
            partial_payload(&fixture),
            Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
            TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        )
        .expect("a policy rejection is a normal resolution outcome");

        let DropResolution::Rejected(rejected) = resolution else {
            panic!("disabled tiled presentation must reject the background winner");
        };
        assert!(
            matches!(
                rejected.candidates(),
                [candidate]
                if matches!(
                    candidate.reason(),
                    DropRejectionReason::Policy(PolicyRejection::PresentationModeDisabled {
                        mode: DockPresentationMode::Tiled,
                    })
                )
            ),
            "unexpected background rejection: {:?}",
            rejected.candidates()
        );
    }

    #[test]
    fn same_surface_contained_complete_root_promotes_named_floating_atomically() {
        let back_rect = LogicalRect::new(300.0, 100.0, 120.0, 100.0).expect("valid back rect");
        let promoted_rect = LogicalRect::new(0.0, 0.0, 120.0, 100.0).expect("valid promoted rect");
        let front_rect = LogicalRect::new(500.0, 300.0, 120.0, 100.0).expect("valid front rect");
        let mut builder = Workspace::builder();
        let back_tabs = builder.insert_node(Node::tabs([ItemId::new(31)]));
        let promoted_tabs = builder.insert_node(Node::tabs([ItemId::new(32)]));
        let front_tabs = builder.insert_node(Node::tabs([ItemId::new(33)]));
        builder.set_root(PROMOTION_BACK_ROOT, RootRecord::new(back_tabs));
        builder.set_root(PROMOTION_ROOT, RootRecord::new(promoted_tabs));
        builder.set_root(PROMOTION_FRONT_ROOT, RootRecord::new(front_tabs));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
        for (floating, root, contained_rect) in [
            (PROMOTION_BACK, PROMOTION_BACK_ROOT, back_rect),
            (PROMOTION_FLOATING, PROMOTION_ROOT, promoted_rect),
            (PROMOTION_FRONT, PROMOTION_FRONT_ROOT, front_rect),
        ] {
            builder.set_contained_floating(floating, ContainedFloating::new(root, contained_rect));
            builder
                .attach_contained(TARGET_SURFACE, floating)
                .expect("target surface exists");
        }
        let workspace = builder.build().expect("workspace must be valid");

        let mut ready = PresentationPlan::new(TARGET_SURFACE, rect());
        ready
            .set_surface_background(DropTargetRecord::surface_background(
                SurfaceBackground::new(TARGET_SURFACE),
                HitRegion::new(rect()),
                DropVisual::new(rect()),
            ))
            .expect("background record is valid");
        for (floating, contained_rect, layer) in [
            (PROMOTION_BACK, back_rect, 2),
            (PROMOTION_FLOATING, promoted_rect, 3),
            (PROMOTION_FRONT, front_rect, 4),
        ] {
            ready.push_drop_occlusion(DropOcclusionRecord::new(
                floating,
                HitRegion::new(contained_rect),
                SceneLayerKey::new(layer),
            ));
            ready.push_contained_minimum(contained_minimum(floating));
        }
        let scene = seal_workspace(&workspace, ready);
        let node_source = || {
            workspace
                .capture_node_source(PROMOTION_ROOT, promoted_tabs)
                .expect("complete contained source must be current")
        };
        let payloads = [
            MovePayload::Item(
                workspace
                    .capture_item_source(PROMOTION_ROOT, promoted_tabs, ItemId::new(32))
                    .expect("complete item source must be current"),
            ),
            MovePayload::Tabs(node_source()),
            MovePayload::Subtree(node_source()),
        ];
        let irrelevant_offers = [
            None,
            Some(SurfaceBackgroundRootOffer::new(PROMOTION_BACK_ROOT)),
            Some(SurfaceBackgroundRootOffer::new(PROMOTION_ROOT)),
        ];

        for payload in payloads {
            for offer in irrelevant_offers {
                assert_same_surface_promotion(&workspace, &scene, payload.clone(), offer);
            }
        }
    }

    #[test]
    fn cross_surface_contained_item_tab_and_title_payloads_rehome_to_unique_background() {
        let fixture = fixture();
        let scene = seal_workspace(&fixture.workspace, cross_surface_background(None));

        for payload in contained_complete_payloads(&fixture) {
            let query = resolve_drop(
                &scene,
                &fixture.workspace,
                &DockPolicySnapshot::default(),
                session(),
                payload,
                None,
                CROSS_TARGET_SURFACE,
                LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
            )
            .expect("cross-surface resolution cannot expose an invariant");
            assert!(query.affordance().is_none());
            let DropResolution::Resolved(resolved) = query.resolution() else {
                panic!("every complete contained payload must resolve as a rehome");
            };
            assert_eq!(
                resolved.target_id(),
                DropTargetId::SurfaceBackground {
                    surface: CROSS_TARGET_SURFACE,
                }
            );
            assert!(matches!(
                resolved.command(),
                WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Main {
                        surface: CROSS_TARGET_SURFACE,
                    },
                } if source.root() == CONTAINED_ROOT
            ));

            let mut candidate = fixture.workspace.clone();
            WorkspaceTransaction::from_commands([resolved.command().clone()])
                .apply(&mut candidate, &DockPolicySnapshot::default())
                .expect("resolved cross-surface rehome must commit");
            assert_eq!(
                candidate
                    .surface(CROSS_TARGET_SURFACE)
                    .expect("cross target remains")
                    .main_root,
                Some(CONTAINED_ROOT)
            );
            assert!(candidate.contained_floating(CONTAINED).is_none());
            assert_eq!(
                candidate.presentation_for_root(CONTAINED_ROOT),
                Some(RootPresentationOwner::Main {
                    surface: CROSS_TARGET_SURFACE,
                })
            );
            assert_eq!(
                candidate
                    .surface(CROSS_TARGET_SURFACE)
                    .expect("cross target remains")
                    .contained,
                vec![CROSS_CONTAINED]
            );
        }
    }

    #[test]
    fn cross_surface_contained_background_rejection_never_falls_through() {
        let fixture = fixture();
        let engine = DockEngine::new(fixture.workspace.clone(), DockPolicy::default())
            .expect("cross-surface fixture engine must be valid");
        let mut plan = compile_fixture_surface_plan(&engine, CROSS_TARGET_SURFACE, rect());
        let guide = plan
            .drop_guide_clusters()
            .iter()
            .find(|cluster| {
                cluster.id().root == CROSS_CONTAINED_ROOT
                    && cluster.id().scope == DropGuideScope::Outer
            })
            .and_then(|cluster| cluster.target(DropGuideSlot::Edge(Edge::Bottom)))
            .expect("production compilation must publish the visible contained outer guide");
        let target_id = guide.id();
        let guide_draw = guide.draw();
        let target = plan
            .drop_target(target_id)
            .expect("production compilation must publish the contained outer target");
        let location = target.location();
        let target_region = target.region();
        let target_layer = target.layer();
        let target_visual = target.visual();
        let replacement = Arc::new(DropTargetRecord::new(
            location,
            DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable),
            target_region,
            target_layer,
            target_visual,
        ));
        assert!(plan.replace_drop_target_for_test(replacement));
        let scene = seal_workspace(&fixture.workspace, plan);
        let point = LogicalPoint::new(
            guide_draw.x() + guide_draw.width() * 0.5,
            guide_draw.y() + guide_draw.height() * 0.5,
        )
        .expect("visible guide midpoint must be finite");

        for payload in contained_complete_payloads(&fixture) {
            let query = resolve_drop(
                &scene,
                &fixture.workspace,
                &DockPolicySnapshot::default(),
                session(),
                payload,
                None,
                CROSS_TARGET_SURFACE,
                point,
            )
            .expect("expected target rejection cannot expose an invariant");
            assert!(
                matches!(
                    query.resolution(),
                    DropResolution::Rejected(rejected)
                        if rejected.candidates().len() == 1
                            && rejected.candidates()[0].target_id()
                                == target_id
                            && rejected.candidates()[0].reason()
                                == &DropRejectionReason::SceneUnavailable(
                                    DropTargetUnavailable::AdapterUnavailable,
                                )
                ),
                "unexpected visible-target resolution: {:#?}",
                query.resolution()
            );
        }
    }

    #[test]
    fn partial_background_delivery_requires_and_consumes_the_frozen_offer() {
        let fixture = fixture();
        let scene = seal(&fixture, ready_background(None));
        for payload in partial_payloads(&fixture) {
            let missing = resolve(&fixture, &scene, payload.clone(), None);
            assert!(matches!(
                missing,
                DropResolution::Rejected(rejected)
                    if rejected.candidates().len() == 1
                        && rejected.candidates()[0].reason()
                            == &DropRejectionReason::SurfaceBackgroundRootOfferMissing
            ));

            let offered = resolve(
                &fixture,
                &scene,
                payload,
                Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
            );
            let DropResolution::Resolved(resolved) = offered else {
                panic!("fresh offer must resolve every partial payload class");
            };
            assert!(matches!(
                resolved.command(),
                WorkspaceCommand::InstallMainRoot {
                    surface: TARGET_SURFACE,
                    root: OFFERED_ROOT,
                    ..
                }
            ));
        }
    }

    #[test]
    fn colliding_offer_rejects_atomically_without_fallthrough() {
        let fixture = fixture();
        let before = fixture.workspace.clone();
        let scene = seal(&fixture, ready_background(None));
        let resolution = resolve(
            &fixture,
            &scene,
            partial_payload(&fixture),
            Some(SurfaceBackgroundRootOffer::new(CONTAINED_ROOT)),
        );
        assert!(matches!(
            resolution,
            DropResolution::Rejected(rejected)
                if rejected.candidates().len() == 1
                    && matches!(
                        rejected.candidates()[0].reason(),
                        DropRejectionReason::Prevalidation(TransactionError::Command { .. })
                    )
        ));
        assert_eq!(fixture.workspace, before);
    }

    #[test]
    fn rejected_front_target_never_falls_through_to_background() {
        let fixture = fixture();
        let target = DropTargetRecord::new(
            DropTargetLocation::Center {
                surface: TARGET_SURFACE,
                root: CONTAINED_ROOT,
                tabs: fixture.contained_tabs,
            },
            DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable),
            HitRegion::new(rect()),
            SceneLayerKey::new(2),
            DropVisual::new(rect()),
        );
        let scene = seal(&fixture, ready_background(Some(target)));
        let resolution = resolve(
            &fixture,
            &scene,
            partial_payload(&fixture),
            Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
        );
        assert!(matches!(
            resolution,
            DropResolution::Rejected(rejected)
                if rejected.candidates().len() == 1
                    && rejected.candidates()[0].target_id().kind() == DropTargetKind::Center
        ));
    }

    #[test]
    fn contained_occlusion_prevents_background_pass_through() {
        let fixture = fixture();
        let scene = seal(&fixture, ready_background(None));
        let resolution = resolve_unguided_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            partial_payload(&fixture),
            Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
            TARGET_SURFACE,
            LogicalPoint::new(550.0, 400.0).expect("point must be finite"),
        )
        .expect("resolution cannot expose an invariant");

        assert!(matches!(resolution, DropResolution::KnownNone(_)));
    }

    #[test]
    fn unclipped_occlusion_hits_only_within_ready_surface_intersection() {
        let fixture = fixture();
        let surface_bounds =
            LogicalRect::new(0.0, 0.0, 600.0, 450.0).expect("surface bounds are valid");
        let outside_target = DropTargetRecord::new(
            DropTargetLocation::Center {
                surface: TARGET_SURFACE,
                root: CONTAINED_ROOT,
                tabs: fixture.contained_tabs,
            },
            DropTargetAvailability::Available,
            HitRegion::new(
                LogicalRect::new(600.0, 350.0, 100.0, 100.0).expect("outside hit region is valid"),
            ),
            SceneLayerKey::new(2),
            DropVisual::new(
                LogicalRect::new(500.0, 350.0, 100.0, 100.0).expect("inside preview is valid"),
            ),
        );
        let scene = seal(
            &fixture,
            ready_background_in(surface_bounds, Some(outside_target)),
        );
        let offer = Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT));
        let intersection = LogicalPoint::new(550.0, 400.0).expect("point is finite");
        let clear_inside = LogicalPoint::new(100.0, 100.0).expect("point is finite");
        let raw_only = LogicalPoint::new(650.0, 400.0).expect("point is finite");

        let blocked = resolve_unguided_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            partial_payload(&fixture),
            offer,
            TARGET_SURFACE,
            intersection,
        )
        .expect("intersection resolution cannot expose an invariant");
        assert!(matches!(blocked, DropResolution::KnownNone(_)));

        let clear = resolve_unguided_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            partial_payload(&fixture),
            offer,
            TARGET_SURFACE,
            clear_inside,
        )
        .expect("clear resolution cannot expose an invariant");
        assert!(matches!(clear, DropResolution::Resolved(_)));

        let outside = resolve_unguided_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            partial_payload(&fixture),
            offer,
            TARGET_SURFACE,
            raw_only,
        )
        .expect("outside resolution cannot expose an invariant");
        assert!(matches!(outside, DropResolution::KnownNone(_)));

        let queried = resolve_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            partial_payload(&fixture),
            offer,
            TARGET_SURFACE,
            raw_only,
        )
        .expect("outside query cannot expose an invariant");
        assert!(matches!(queried.resolution(), DropResolution::KnownNone(_)));
        assert!(queried.affordance().is_none());
    }

    #[test]
    fn complete_main_source_with_contained_siblings_still_suppresses_only_its_root() {
        let mut builder = Workspace::builder();
        let main_tabs = builder.insert_node(Node::tabs([ItemId::new(71)]));
        let sibling_tabs = builder.insert_node(Node::tabs([ItemId::new(72)]));
        let main_root = RootId::new(73);
        let sibling_root = RootId::new(74);
        let surface = SurfaceId::new(75);
        let sibling = FloatingPresentationId::new(76);
        builder.set_root(main_root, RootRecord::new(main_tabs));
        builder.set_root(sibling_root, RootRecord::new(sibling_tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(main_root));
        builder.set_contained_floating(sibling, ContainedFloating::new(sibling_root, rect()));
        builder
            .attach_contained(surface, sibling)
            .expect("surface exists");
        let workspace = builder.build().expect("workspace must be valid");
        let payload = MovePayload::Tabs(
            workspace
                .capture_node_source(main_root, main_tabs)
                .expect("main source must be current"),
        );

        let workspace_version = WorkspaceVersion::initial();
        let workspace_index = WorkspaceIndex::build(&workspace, workspace_version)
            .expect("suppression index must build");
        let source = workspace_index
            .capture_complete_root_source(&workspace, workspace_version, &payload)
            .expect("complete source analysis must succeed")
            .expect("complete root source must exist");
        let suppression =
            SourceSuppression::from_source(&workspace, &source).expect("complete root suppresses");
        assert!(suppression.excludes_target(DropTargetId::Center {
            surface,
            root: main_root,
            tabs: main_tabs,
        }));
        assert!(!suppression.excludes_target(DropTargetId::Center {
            surface,
            root: sibling_root,
            tabs: sibling_tabs,
        }));
        assert!(!suppression.excludes_occlusion(sibling));
    }

    #[test]
    fn same_root_edge_preview_matches_the_post_detach_payload_layout() {
        let surface = SurfaceId::new(91);
        let root = RootId::new(92);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(93)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(94)]));
        let split = builder.insert_node(
            Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
                .expect("weighted source layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("preview workspace must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("preview engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, surface, rect());
        assert!(plan.layout_facts().is_some());
        let target_id = DropTargetId::InnerEdge {
            surface,
            root,
            node: target_tabs,
            edge: Edge::Left,
        };
        let target = plan
            .drop_guide_clusters()
            .iter()
            .flat_map(DropGuideClusterRecord::targets)
            .map(|(_, target)| target)
            .find(|target| target.id() == target_id)
            .expect("the target pane must expose a left inner guide");
        let hit = target.hit().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("guide midpoint must be finite");
        let source = MovePayload::Tabs(
            workspace
                .capture_node_source(root, source_tabs)
                .expect("source tabs must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        assert!(
            scene
                .ready_surface(surface)
                .expect("sealed target surface must remain ready")
                .plan()
                .layout_facts()
                .is_some()
        );
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("same-root edge resolution must remain valid");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!("the exact inner guide must resolve");
        };

        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &DockPolicySnapshot::default())
            .expect("the prevalidated drop command must commit");
        let committed_engine = DockEngine::new(committed, DockPolicy::default())
            .expect("the committed workspace must remain valid");
        let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
        let payload_bounds = committed_plan
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == source_tabs)
            .expect("the moved tabs node must remain the payload leaf")
            .bounds();

        assert_eq!(resolved.visual().rect(), payload_bounds);
    }

    #[test]
    fn same_root_center_preview_matches_the_post_detach_content_layout() {
        let surface = SurfaceId::new(95);
        let root = RootId::new(96);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(97)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(98)]));
        let split = builder.insert_node(
            Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
                .expect("weighted center-preview layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder
            .build()
            .expect("center-preview workspace must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("center-preview engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, surface, rect());
        let target_id = DropTargetId::Center {
            surface,
            root,
            tabs: target_tabs,
        };
        let target = plan
            .drop_guide_clusters()
            .iter()
            .flat_map(DropGuideClusterRecord::targets)
            .map(|(_, target)| target)
            .find(|target| target.id() == target_id)
            .expect("the target pane must expose a center guide");
        let stale_visual = target.preview();
        let hit = target.hit().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("guide midpoint must be finite");
        let source = MovePayload::Tabs(
            workspace
                .capture_node_source(root, source_tabs)
                .expect("source tabs must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("same-root center resolution must remain valid");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!("the exact center guide must resolve");
        };

        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &DockPolicySnapshot::default())
            .expect("the prevalidated center command must commit");
        let committed_engine = DockEngine::new(committed, DockPolicy::default())
            .expect("the committed center workspace must remain valid");
        let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
        let content_bounds = committed_plan
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == target_tabs)
            .expect("the target tabs must remain after center merge")
            .content_bounds();

        assert_ne!(stale_visual, content_bounds);
        assert_eq!(resolved.visual().rect(), content_bounds);
    }

    #[test]
    fn non_guide_center_basin_resolves_center_target() {
        let surface = SurfaceId::new(99);
        let root = RootId::new(100);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(101)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(102)]));
        let split = builder.insert_node(
            Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
                .expect("weighted magnetic-basin layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder
            .build()
            .expect("magnetic-basin workspace must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("magnetic-basin engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, surface, rect());
        let target_id = DropTargetId::Center {
            surface,
            root,
            tabs: target_tabs,
        };
        let content = plan
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == target_tabs)
            .expect("the target pane must be presented")
            .content_bounds();
        let point = LogicalPoint::new(
            content.x() + content.width() * 0.65,
            content.y() + content.height() * 0.65,
        )
        .expect("the magnetic-basin point must be finite");
        assert!(plan.drop_guide_clusters().iter().all(|cluster| {
            cluster
                .targets()
                .all(|(_, target)| !target.hit().contains(point))
        }));
        let source = MovePayload::Tabs(
            workspace
                .capture_node_source(root, source_tabs)
                .expect("source tabs must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("magnetic-basin resolution must remain valid");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!(
                "the ordinary pane center basin must resolve a docking target, got {:?}",
                resolved.resolution()
            );
        };

        assert_eq!(resolved.target_id(), target_id);
    }

    #[test]
    fn magnetic_center_resolution_does_not_require_guide_records() {
        let surface = SurfaceId::new(103);
        let root = RootId::new(104);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(105)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(106)]));
        let split = builder.insert_node(
            Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
                .expect("guide-independent magnetic layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder
            .build()
            .expect("guide-independent magnetic workspace must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("guide-independent magnetic engine must be valid");
        let guided = compile_fixture_surface_plan(&engine, surface, rect());
        let edge_target = DropTargetId::InnerEdge {
            surface,
            root,
            node: target_tabs,
            edge: Edge::Left,
        };
        let former_guide_hit = guided
            .drop_guide_clusters()
            .iter()
            .flat_map(|cluster| cluster.targets())
            .find_map(|(_, target)| (target.id() == edge_target).then(|| target.hit().rect()))
            .expect("guided compilation must expose the target pane's left guide hit");
        let point = LogicalPoint::new(
            former_guide_hit.x() + former_guide_hit.width() * 0.5,
            former_guide_hit.y() + former_guide_hit.height() * 0.5,
        )
        .expect("former guide center must be finite");

        let plan = compile_fixture_surface_plan_without_guides(&engine, surface, rect());
        let content = plan
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == target_tabs)
            .expect("the target pane must be presented")
            .content_bounds();
        assert!(plan.drop_guide_clusters().is_empty());
        assert!(
            crate::target_field::pane_center_basin(content, engine.presentation_config())
                .expect("pane center basin must be valid")
                .contains(point),
            "the former edge-guide button must overlap the magnetic center basin for this proof"
        );

        let source = MovePayload::Tabs(
            workspace
                .capture_node_source(root, source_tabs)
                .expect("source tabs must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let query = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("guide-independent magnetic resolution must remain valid");
        let DropResolution::Resolved(resolved) = query.resolution() else {
            panic!("the guide-independent center basin must resolve: {query:?}");
        };
        assert_eq!(
            resolved.target_id(),
            DropTargetId::Center {
                surface,
                root,
                tabs: target_tabs,
            }
        );
        assert!(query.affordance().is_none());
    }

    #[test]
    fn same_root_tab_gap_preview_tracks_the_inserted_tab_in_the_future_strip() {
        let surface = SurfaceId::new(111);
        let root = RootId::new(112);
        let moved = ItemId::new(113);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([moved, ItemId::new(116)]));
        let target_tabs = builder.insert_node(Node::tabs_with_selection(
            [ItemId::new(114), ItemId::new(115)],
            Some(ItemId::new(115)),
        ));
        let split = builder.insert_node(
            Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.8, 0.2])
                .expect("weighted tab-gap layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split).with_central(target_tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder
            .build()
            .expect("tab-gap preview workspace must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("tab-gap preview engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, surface, rect());
        let target_id = DropTargetId::TabGap {
            surface,
            root,
            tabs: target_tabs,
            index: 2,
        };
        assert!(
            plan.presentation_menu_anchor_records()
                .iter()
                .any(|anchor| {
                    anchor.host()
                        == crate::scene::PresentationMenuAnchorHost::TabBar(
                            crate::scene::TabBarSceneId {
                                root,
                                tabs: target_tabs,
                            },
                        )
                })
        );
        let target = plan
            .drop_targets()
            .iter()
            .find(|target| target.id() == target_id)
            .expect("the target strip must expose its middle tab gap");
        let stale_visual = target.visual().rect();
        let hit = target.region().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("tab-gap midpoint must be finite");
        let source = MovePayload::Item(
            workspace
                .capture_item_source(root, source_tabs, moved)
                .expect("source item must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("same-root tab-gap resolution must remain valid");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!("the exact tab gap must resolve");
        };

        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &DockPolicySnapshot::default())
            .expect("the prevalidated tab-gap command must commit");
        let committed_engine = DockEngine::new(committed, DockPolicy::default())
            .expect("the committed tab-gap workspace must remain valid");
        let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
        let bar = committed_plan
            .tab_bar_records()
            .iter()
            .find(|bar| bar.id().root == root && bar.id().tabs == target_tabs)
            .expect("the target tab bar must remain after insertion");
        let inserted = bar
            .members()
            .iter()
            .find(|member| member.tab().item == moved)
            .expect("the future strip must contain the moved item")
            .full_bounds();
        let width = stale_visual.width().min(bar.viewport().width());
        let marker_x = (inserted.x() - width * 0.5).clamp(
            bar.viewport().x(),
            (bar.viewport().max().x() - width).max(bar.viewport().x()),
        );
        let expected =
            LogicalRect::new(marker_x, bar.viewport().y(), width, bar.viewport().height())
                .expect("future tab-gap marker must be valid");

        assert_ne!(stale_visual, expected);
        assert_eq!(resolved.visual().rect(), expected);
    }

    #[test]
    fn future_tab_gap_preview_reselects_the_menu_host_after_the_old_host_is_pruned() {
        let surface = SurfaceId::new(121);
        let root = RootId::new(122);
        let moved = ItemId::new(123);
        let mut builder = Workspace::builder();
        let old_host = builder.insert_node(Node::tabs([moved]));
        let target_tabs = builder.insert_node(Node::tabs_with_selection(
            [ItemId::new(124), ItemId::new(125)],
            Some(ItemId::new(125)),
        ));
        let trailing_tabs = builder.insert_node(Node::tabs([ItemId::new(126)]));
        let split = builder.insert_node(
            Node::split(
                Axis::Horizontal,
                [old_host, target_tabs, trailing_tabs],
                [0.2, 0.14, 0.66],
            )
            .expect("weighted menu-host layout must be valid"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("menu-host fixture must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("menu-host preview engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, surface, rect());
        assert!(
            plan.presentation_menu_anchor_records()
                .iter()
                .any(|anchor| {
                    anchor.host()
                        == crate::scene::PresentationMenuAnchorHost::TabBar(
                            crate::scene::TabBarSceneId {
                                root,
                                tabs: old_host,
                            },
                        )
                })
        );

        let target_id = DropTargetId::TabGap {
            surface,
            root,
            tabs: target_tabs,
            index: 2,
        };
        let target = plan
            .drop_targets()
            .iter()
            .find(|target| target.id() == target_id)
            .expect("the narrow target strip must expose its trailing gap");
        let hit = target.region().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("tab-gap midpoint must be finite");
        let source = MovePayload::Item(
            workspace
                .capture_item_source(root, old_host, moved)
                .expect("source item must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            surface,
            point,
        )
        .expect("candidate menu-host resolution must remain valid");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!("the exact trailing tab gap must resolve");
        };

        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &DockPolicySnapshot::default())
            .expect("the prevalidated menu-host command must commit");
        let committed_engine = DockEngine::new(committed, DockPolicy::default())
            .expect("the committed menu-host workspace must remain valid");
        let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
        assert!(
            committed_plan
                .presentation_menu_anchor_records()
                .iter()
                .any(|anchor| {
                    anchor.host()
                        == crate::scene::PresentationMenuAnchorHost::TabBar(
                            crate::scene::TabBarSceneId {
                                root,
                                tabs: target_tabs,
                            },
                        )
                        && anchor.is_ready()
                }),
            "candidate anchors: {:?}; bars: {:?}",
            committed_plan.presentation_menu_anchor_records(),
            committed_plan.tab_bar_records(),
        );
        let bar = committed_plan
            .tab_bar_records()
            .iter()
            .find(|bar| bar.id().root == root && bar.id().tabs == target_tabs)
            .expect("the target tab bar must survive menu-host reselection");
        let inserted = bar
            .members()
            .iter()
            .find(|member| member.tab().item == moved)
            .expect("the future strip must contain the moved item")
            .full_bounds();
        let marker_width = resolved.visual().rect().width().min(bar.viewport().width());
        let marker_x = (inserted.x() - marker_width * 0.5).clamp(
            bar.viewport().x(),
            (bar.viewport().max().x() - marker_width).max(bar.viewport().x()),
        );
        let expected = LogicalRect::new(
            marker_x,
            bar.viewport().y(),
            marker_width,
            bar.viewport().height(),
        )
        .expect("candidate-aware tab-gap marker must be valid");

        assert_eq!(resolved.visual().rect(), expected);
    }

    #[test]
    fn cross_surface_unselected_item_preview_uses_retained_source_measurements() {
        let source_surface = SurfaceId::new(101);
        let target_surface = SurfaceId::new(102);
        let source_root = RootId::new(103);
        let target_root = RootId::new(104);
        let moved = ItemId::new(105);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs_with_selection(
            [moved, ItemId::new(106)],
            Some(ItemId::new(106)),
        ));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(107)]));
        builder.set_root(source_root, RootRecord::new(source_tabs));
        builder.set_root(target_root, RootRecord::new(target_tabs));
        builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
        builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
        let workspace = builder
            .build()
            .expect("cross-surface fixture must be valid");
        let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("cross-surface preview engine must be valid");
        let plan = compile_fixture_surface_plan(&engine, target_surface, rect());
        let target_id = DropTargetId::InnerEdge {
            surface: target_surface,
            root: target_root,
            node: target_tabs,
            edge: Edge::Right,
        };
        let target = plan
            .drop_guide_clusters()
            .iter()
            .flat_map(DropGuideClusterRecord::targets)
            .map(|(_, target)| target)
            .find(|target| target.id() == target_id)
            .expect("target pane must expose a right inner guide");
        let hit = target.hit().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("guide midpoint must be finite");
        let source = MovePayload::Item(
            workspace
                .capture_item_source(source_root, source_tabs, moved)
                .expect("unselected source item must be current"),
        );
        let scene = seal_workspace(&workspace, plan);
        let resolved = resolve_drop(
            &scene,
            &workspace,
            &DockPolicySnapshot::default(),
            session(),
            source,
            None,
            target_surface,
            point,
        )
        .expect("cross-surface edge resolution must use retained source measurements");
        let DropResolution::Resolved(resolved) = resolved.resolution() else {
            panic!("the exact cross-surface guide must resolve");
        };

        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &DockPolicySnapshot::default())
            .expect("the cross-surface drop command must commit");
        let committed_engine = DockEngine::new(committed, DockPolicy::default())
            .expect("the committed cross-surface workspace must remain valid");
        let committed_plan =
            compile_fixture_surface_plan(&committed_engine, target_surface, rect());
        let payload_bounds = committed_plan
            .pane_records()
            .iter()
            .find(|pane| committed_plan.pane_selected(pane) == Some(moved))
            .expect("the moved item must own its new target leaf")
            .bounds();

        assert_eq!(resolved.visual().rect(), payload_bounds);
    }
}

#[cfg(test)]
#[path = "drop_resolver/contract_tests.rs"]
mod contract_tests;
