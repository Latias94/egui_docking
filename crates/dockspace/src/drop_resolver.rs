//! Deterministic resolution of authoritative drop locations.

use crate::RootPresentationOwner;
use crate::command::{MovePayload, WorkspaceCommand};
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideScope, DropGuideSlot,
};
use crate::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetKind, DropTargetRecord, DropTargetUnavailable,
    DropVisual, SceneLayerKey,
};
use crate::error::{ReferenceRole, TransactionError};
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::DragSessionId;
use crate::policy::{DockPolicy, PolicyRejection};
use crate::scene::{ReadySurfaceScene, SceneStamp, SealedScene, SurfaceScene};
use crate::transaction::WorkspaceTransaction;
use thiserror::Error;

/// Queries both the exact drop outcome and the complete visible guide affordance.
///
/// A scene with explicit guide clusters uses only exact, half-open guide-button
/// hits plus standalone tab-gap targets. An exact guide hit wins over a tab gap,
/// and an ineligible exact winner never falls through to another target. If the
/// point activates a cluster without hitting a button, the result is
/// [`DropResolution::KnownNone`] with a visible [`DropAffordance`]. Scenes with
/// no guide clusters retain [`resolve_drop`] compatibility.
///
/// Every selected guide target is checked against scene availability, current
/// policy, and a cloned U3 transaction before the owned affordance is returned.
/// If removing the frozen payload consumes its complete source root, that root's
/// targets and contained presentation are suppressed before occlusion and hit
/// resolution so the presentation being removed cannot hide its destination.
/// The result therefore remains valid for painting after the borrowed scene has
/// been released, while preview acknowledgement stays solely in
/// [`DropResolution::Resolved`].
///
/// # Errors
///
/// Returns [`DropResolutionError`] if prevalidating any visible guide or exact
/// tab-gap target exposes an unexpected transaction failure.
pub fn query_drop(
    scene: &SealedScene,
    workspace: &Workspace,
    policy: &DockPolicy,
    session: DragSessionId,
    source: MovePayload,
    surface: SurfaceId,
    point: LogicalPoint,
) -> Result<DropQuery, DropResolutionError> {
    let Some(surface_scene) = scene.surface(surface) else {
        return Ok(DropQuery::new(
            DropResolution::Unavailable(UnavailableDrop::new(
                scene.stamp(),
                surface,
                DropSurfaceUnavailable::MissingSurface,
            )),
            None,
        ));
    };
    let SurfaceScene::Ready(ready) = surface_scene else {
        return Ok(DropQuery::new(
            DropResolution::Unavailable(UnavailableDrop::new(
                scene.stamp(),
                surface,
                DropSurfaceUnavailable::Bootstrap,
            )),
            None,
        ));
    };

    if !HitRegion::new(ready.bounds()).contains(point) {
        return Ok(DropQuery::new(
            DropResolution::KnownNone(KnownDropAbsence::new(scene.stamp(), surface, point)),
            None,
        ));
    }

    if ready.drop_guide_clusters().is_empty() {
        return resolve_drop(scene, workspace, policy, session, source, surface, point)
            .map(|resolution| DropQuery::new(resolution, None));
    }

    let suppression = SourceSuppression::from_payload(workspace, &source);
    let occluding_layer = occluding_layer(ready, point, suppression);
    let selected = select_guide_clusters(ready, point, occluding_layer, suppression);
    let assessment = DropAssessment {
        workspace,
        policy,
        source: &source,
    };
    let (affordance, exact) =
        build_affordance(scene.stamp(), surface, point, selected, assessment)?;
    let resolution = match exact {
        Some(ExactGuidePreparation::Eligible {
            target,
            visual,
            command,
        }) => DropResolution::Resolved(ResolvedDrop {
            scene: scene.stamp(),
            session,
            source,
            target,
            visual,
            command,
        }),
        Some(ExactGuidePreparation::Rejected { target, reason }) => {
            DropResolution::Rejected(RejectedDrop::new(
                scene.stamp(),
                surface,
                point,
                vec![DropCandidateRejection::new(target, reason)],
            ))
        }
        None => resolve_exact_tab_gap(
            &DropLocation {
                scene,
                ready,
                workspace,
                policy,
                session,
                surface,
                point,
                occluding_layer,
                suppression,
            },
            source,
        )?,
    };

    Ok(DropQuery::new(resolution, affordance))
}

/// Resolves one authoritative surface-local point against an immutable scene.
///
/// Every geometric candidate is checked against explicit availability, current
/// policy, and a complete U3 candidate transaction. Visible candidates are
/// first restricted by the frontmost explicit occlusion
/// at the point: targets below that layer cannot participate or become fallback
/// candidates. Remaining resolution order is exactly
/// `TabGap > Center > InnerEdge > OuterEdge`, then larger explicit layer, then
/// the lowest structural target identity. Geometry area, scene insertion order,
/// focus, time, and pointer delta never participate.
///
/// If removing the frozen payload consumes its complete source root, that
/// root's targets and contained presentation are excluded before occlusion and
/// candidate ordering.
///
/// # Errors
///
/// Returns [`DropResolutionError`] if U3 candidate preparation exposes an
/// invariant, canonicalization, validation, or reconciliation failure. Expected
/// checked-command rejections are represented by [`DropResolution::Rejected`].
pub fn resolve_drop(
    scene: &SealedScene,
    workspace: &Workspace,
    policy: &DockPolicy,
    session: DragSessionId,
    source: MovePayload,
    surface: SurfaceId,
    point: LogicalPoint,
) -> Result<DropResolution, DropResolutionError> {
    let Some(surface_scene) = scene.surface(surface) else {
        return Ok(DropResolution::Unavailable(UnavailableDrop::new(
            scene.stamp(),
            surface,
            DropSurfaceUnavailable::MissingSurface,
        )));
    };
    let SurfaceScene::Ready(surface_scene) = surface_scene else {
        return Ok(DropResolution::Unavailable(UnavailableDrop::new(
            scene.stamp(),
            surface,
            DropSurfaceUnavailable::Bootstrap,
        )));
    };

    if !HitRegion::new(surface_scene.bounds()).contains(point) {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            scene.stamp(),
            surface,
            point,
        )));
    }

    let suppression = SourceSuppression::from_payload(workspace, &source);
    let occluding_layer = occluding_layer(surface_scene, point, suppression);
    let mut hits: Vec<&DropTargetRecord> = surface_scene
        .drop_targets()
        .iter()
        .filter(|target| {
            suppression.is_none_or(|entry| !entry.excludes_target(target.id()))
                && target.region().contains(point)
                && occluding_layer.is_none_or(|layer| target.layer() >= layer)
        })
        .collect();
    if hits.is_empty() {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            scene.stamp(),
            surface,
            point,
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

    let assessment = DropAssessment {
        workspace,
        policy,
        source: &source,
    };
    let mut rejections = Vec::with_capacity(hits.len());
    for target in hits {
        match prepare_drop_target(assessment, target)? {
            PreparedDropTarget::Eligible(command) => {
                return Ok(DropResolution::Resolved(ResolvedDrop {
                    scene: scene.stamp(),
                    session,
                    source,
                    target: target.id(),
                    visual: target.visual(),
                    command,
                }));
            }
            PreparedDropTarget::Rejected(reason) => {
                rejections.push(DropCandidateRejection::new(target.id(), reason));
            }
        }
    }

    Ok(DropResolution::Rejected(RejectedDrop::new(
        scene.stamp(),
        surface,
        point,
        rejections,
    )))
}

fn occluding_layer(
    ready: &ReadySurfaceScene,
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
    ready: &ReadySurfaceScene,
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
        return [Some(inner), outer].into_iter().flatten().collect();
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
    scene: SceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
    clusters: Vec<&DropGuideClusterRecord>,
    assessment: DropAssessment<'_>,
) -> Result<(Option<DropAffordance>, Option<ExactGuidePreparation>), DropResolutionError> {
    if clusters.is_empty() {
        return Ok((None, None));
    }

    let mut active = None;
    let mut exact = None;
    let mut owned_clusters = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        let cluster_id = cluster.id();
        let mut targets = Vec::with_capacity(cluster.targets().count());
        for (slot, guide_target) in cluster.targets() {
            let target = guide_target.target();
            let key = DropGuideTargetKey::new(cluster_id, slot, target.id());
            let preparation = prepare_drop_target(assessment, target)?;
            let is_exact_hit = active.is_none() && target.region().contains(point);
            let eligibility = match preparation {
                PreparedDropTarget::Eligible(command) => {
                    if is_exact_hit {
                        exact = Some(ExactGuidePreparation::Eligible {
                            target: target.id(),
                            visual: target.visual(),
                            command,
                        });
                    }
                    DropGuideEligibility::Eligible
                }
                PreparedDropTarget::Rejected(reason) => {
                    if is_exact_hit {
                        exact = Some(ExactGuidePreparation::Rejected {
                            target: target.id(),
                            reason: reason.clone(),
                        });
                    }
                    DropGuideEligibility::Rejected(reason)
                }
            };
            if is_exact_hit {
                active = Some(key);
            }
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
    // Selection is inner-first for exact hit precedence; paint back-to-front.
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

fn resolve_exact_tab_gap(
    location: &DropLocation<'_>,
    source: MovePayload,
) -> Result<DropResolution, DropResolutionError> {
    let target = location
        .ready
        .drop_targets()
        .iter()
        .filter(|target| target.id().kind() == DropTargetKind::TabGap)
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
                .layer()
                .cmp(&left.layer())
                .then_with(|| left.id().cmp(&right.id()))
        });
    let Some(target) = target else {
        return Ok(DropResolution::KnownNone(KnownDropAbsence::new(
            location.scene.stamp(),
            location.surface,
            location.point,
        )));
    };

    let assessment = DropAssessment {
        workspace: location.workspace,
        policy: location.policy,
        source: &source,
    };
    match prepare_drop_target(assessment, target)? {
        PreparedDropTarget::Eligible(command) => Ok(DropResolution::Resolved(ResolvedDrop {
            scene: location.scene.stamp(),
            session: location.session,
            source,
            target: target.id(),
            visual: target.visual(),
            command,
        })),
        PreparedDropTarget::Rejected(reason) => Ok(DropResolution::Rejected(RejectedDrop::new(
            location.scene.stamp(),
            location.surface,
            location.point,
            vec![DropCandidateRejection::new(target.id(), reason)],
        ))),
    }
}

fn prepare_drop_target(
    assessment: DropAssessment<'_>,
    target: &DropTargetRecord,
) -> Result<PreparedDropTarget, DropResolutionError> {
    if let DropTargetAvailability::Unavailable(reason) = target.availability() {
        return Ok(PreparedDropTarget::Rejected(
            DropRejectionReason::SceneUnavailable(reason),
        ));
    }
    if let Err(reason) = check_policy(assessment.policy, target.id().kind()) {
        return Ok(PreparedDropTarget::Rejected(DropRejectionReason::Policy(
            reason,
        )));
    }

    let command = WorkspaceCommand::Move {
        payload: assessment.source.clone(),
        target: target.target().clone(),
    };
    let mut candidate = assessment.workspace.clone();
    match WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut candidate, assessment.policy)
    {
        Ok(_) => Ok(PreparedDropTarget::Eligible(command)),
        Err(error) => match &error {
            TransactionError::Command { source, .. } if source.is_expected_rejection() => Ok(
                PreparedDropTarget::Rejected(DropRejectionReason::Prevalidation(error)),
            ),
            _ => Err(DropResolutionError::UnexpectedPrevalidation(error)),
        },
    }
}

#[derive(Clone, Copy)]
struct DropAssessment<'a> {
    workspace: &'a Workspace,
    policy: &'a DockPolicy,
    source: &'a MovePayload,
}

struct DropLocation<'a> {
    scene: &'a SealedScene,
    ready: &'a ReadySurfaceScene,
    workspace: &'a Workspace,
    policy: &'a DockPolicy,
    session: DragSessionId,
    surface: SurfaceId,
    point: LogicalPoint,
    occluding_layer: Option<SceneLayerKey>,
    suppression: Option<SourceSuppression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceSuppression {
    root: RootId,
    floating: Option<FloatingPresentationId>,
}

impl SourceSuppression {
    fn from_payload(workspace: &Workspace, payload: &MovePayload) -> Option<Self> {
        let (root, node, fingerprint) = match payload {
            MovePayload::Item(source) => (source.root(), source.tabs(), source.fingerprint()),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                (source.root(), source.node(), source.fingerprint())
            }
        };
        workspace
            .verify_reference(root, node, fingerprint, ReferenceRole::Source)
            .ok()?;
        let record = workspace.root(root)?;
        // Keep this predicate aligned with operation::move_removes_complete_root
        // and cleanup_item_source_root: suppression represents presentation
        // removal, not merely content removal.
        let removes_complete_root = match payload {
            MovePayload::Item(source) => {
                record.central.is_none()
                    && workspace.collect_items_in_subtree(record.node).len() == 1
                    && matches!(
                        workspace.node(source.tabs()),
                        Some(Node::Tabs { items, .. }) if items.contains(&source.item())
                    )
            }
            MovePayload::Tabs(source) => {
                source.node() == record.node
                    && matches!(
                        workspace.node(source.node()),
                        Some(Node::Tabs { items, .. }) if !items.is_empty()
                    )
            }
            MovePayload::Subtree(source) => {
                source.node() == record.node
                    && !workspace.collect_items_in_subtree(source.node()).is_empty()
            }
        };
        if !removes_complete_root {
            return None;
        }

        let floating = match workspace.presentation_for_root(root)? {
            RootPresentationOwner::Main { surface } => {
                if !workspace.surface(surface)?.contained.is_empty() {
                    return None;
                }
                None
            }
            RootPresentationOwner::Contained { floating, .. } => Some(floating),
        };
        Some(Self { root, floating })
    }

    fn excludes_target(self, target: DropTargetId) -> bool {
        self.root == drop_target_root(target)
    }

    fn excludes_occlusion(self, floating: FloatingPresentationId) -> bool {
        matches!(self.floating, Some(source) if source == floating)
    }
}

const fn drop_target_root(target: DropTargetId) -> RootId {
    match target {
        DropTargetId::TabGap { root, .. }
        | DropTargetId::Center { root, .. }
        | DropTargetId::InnerEdge { root, .. }
        | DropTargetId::OuterEdge { root, .. } => root,
    }
}

enum PreparedDropTarget {
    Eligible(WorkspaceCommand),
    Rejected(DropRejectionReason),
}

enum ExactGuidePreparation {
    Eligible {
        target: DropTargetId,
        visual: DropVisual,
        command: WorkspaceCommand,
    },
    Rejected {
        target: DropTargetId,
        reason: DropRejectionReason,
    },
}

fn check_policy(policy: &DockPolicy, kind: DropTargetKind) -> Result<(), PolicyRejection> {
    match kind {
        DropTargetKind::TabGap | DropTargetKind::Center => policy.check_tab_merge(),
        DropTargetKind::InnerEdge | DropTargetKind::OuterEdge => policy.check_edge_split(),
    }
}

/// Complete semantic classification of one authoritative drop location.
#[derive(Debug, Clone, PartialEq)]
pub enum DropResolution {
    /// One exact command survived all deterministic checks.
    Resolved(ResolvedDrop),
    /// The ready surface contained no target geometry at this point.
    KnownNone(KnownDropAbsence),
    /// Geometry hit one or more targets, but every candidate was ineligible.
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
}

impl DropQuery {
    const fn new(resolution: DropResolution, affordance: Option<DropAffordance>) -> Self {
        Self {
            resolution,
            affordance,
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
}

/// Complete owned guide state selected at one authoritative point.
#[derive(Debug, Clone, PartialEq)]
pub struct DropAffordance {
    scene: SceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
    clusters: Vec<DropAffordanceCluster>,
    active: Option<DropGuideTargetKey>,
}

impl DropAffordance {
    const fn new(
        scene: SceneStamp,
        surface: SurfaceId,
        point: LogicalPoint,
        clusters: Vec<DropAffordanceCluster>,
        active: Option<DropGuideTargetKey>,
    ) -> Self {
        Self {
            scene,
            surface,
            point,
            clusters,
            active,
        }
    }

    /// Returns the sealed scene from which these painting facts were copied.
    #[must_use]
    pub const fn scene(&self) -> SceneStamp {
        self.scene
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
    /// A matching outer cluster precedes its inner cluster, so painting this
    /// slice in order leaves local controls on top. Targets within a cluster
    /// retain canonical `Center, Left, Right, Top, Bottom` order; outer clusters
    /// omit center. Exact hit resolution separately gives the local inner button
    /// precedence if an adapter publishes overlapping inner and outer geometry.
    /// No area or proximity heuristic participates.
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
}

/// Private commit proof paired with renderer-visible target and preview geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDrop {
    scene: SceneStamp,
    session: DragSessionId,
    source: MovePayload,
    target: DropTargetId,
    visual: DropVisual,
    command: WorkspaceCommand,
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

    pub(crate) const fn scene_stamp(&self) -> SceneStamp {
        self.scene
    }

    pub(crate) const fn session(&self) -> DragSessionId {
        self.session
    }

    pub(crate) const fn source(&self) -> &MovePayload {
        &self.source
    }

    pub(crate) fn into_command(self) -> WorkspaceCommand {
        self.command
    }
}

/// Proof that a ready surface had no geometric target at an authoritative point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnownDropAbsence {
    scene: SceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
}

impl KnownDropAbsence {
    const fn new(scene: SceneStamp, surface: SurfaceId, point: LogicalPoint) -> Self {
        Self {
            scene,
            surface,
            point,
        }
    }

    /// Returns the sealed scene queried by the resolver.
    #[must_use]
    pub const fn scene(&self) -> SceneStamp {
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
}

/// Explicit unavailable result for an authoritative surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnavailableDrop {
    scene: SceneStamp,
    surface: SurfaceId,
    reason: DropSurfaceUnavailable,
}

impl UnavailableDrop {
    const fn new(scene: SceneStamp, surface: SurfaceId, reason: DropSurfaceUnavailable) -> Self {
        Self {
            scene,
            surface,
            reason,
        }
    }

    /// Returns the sealed scene queried by the resolver.
    #[must_use]
    pub const fn scene(&self) -> SceneStamp {
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

/// All ineligible candidates at one geometrically known point.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedDrop {
    scene: SceneStamp,
    surface: SurfaceId,
    point: LogicalPoint,
    candidates: Vec<DropCandidateRejection>,
}

impl RejectedDrop {
    const fn new(
        scene: SceneStamp,
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
    pub const fn scene(&self) -> SceneStamp {
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

    /// Returns rejections in the same normative order used for resolution.
    #[must_use]
    pub fn candidates(&self) -> &[DropCandidateRejection] {
        &self.candidates
    }
}
