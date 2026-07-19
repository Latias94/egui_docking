//! Deterministic resolution of authoritative drop locations.

use crate::command::{MovePayload, WorkspaceCommand};
use crate::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetKind, DropTargetRecord, DropTargetUnavailable,
    DropVisual,
};
use crate::error::TransactionError;
use crate::geometry::LogicalPoint;
use crate::graph::Workspace;
use crate::hit_region::HitRegion;
use crate::ids::SurfaceId;
use crate::interaction::DragSessionId;
use crate::policy::{DockPolicy, PolicyRejection};
use crate::scene::{SceneStamp, SealedScene, SurfaceScene};
use crate::transaction::WorkspaceTransaction;
use thiserror::Error;

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

    let occluding_layer = surface_scene
        .drop_occlusions()
        .iter()
        .filter(|occlusion| occlusion.region().contains(point))
        .map(|occlusion| occlusion.layer())
        .max();
    let mut hits: Vec<&DropTargetRecord> = surface_scene
        .drop_targets()
        .iter()
        .filter(|target| {
            target.region().contains(point)
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

    let mut rejections = Vec::with_capacity(hits.len());
    for target in hits {
        if let DropTargetAvailability::Unavailable(reason) = target.availability() {
            rejections.push(DropCandidateRejection::new(
                target.id(),
                DropRejectionReason::SceneUnavailable(reason),
            ));
            continue;
        }

        if let Err(reason) = check_policy(policy, target.id().kind()) {
            rejections.push(DropCandidateRejection::new(
                target.id(),
                DropRejectionReason::Policy(reason),
            ));
            continue;
        }

        let command = WorkspaceCommand::Move {
            payload: source.clone(),
            target: target.target().clone(),
        };
        let mut candidate = workspace.clone();
        match WorkspaceTransaction::from_commands([command.clone()]).apply(&mut candidate, policy) {
            Ok(_) => {
                return Ok(DropResolution::Resolved(ResolvedDrop {
                    scene: scene.stamp(),
                    session,
                    source,
                    target: target.id(),
                    visual: target.visual(),
                    command,
                }));
            }
            Err(error) => match &error {
                TransactionError::Command { source, .. } if source.is_expected_rejection() => {
                    rejections.push(DropCandidateRejection::new(
                        target.id(),
                        DropRejectionReason::Prevalidation(error),
                    ));
                }
                _ => return Err(DropResolutionError::UnexpectedPrevalidation(error)),
            },
        }
    }

    Ok(DropResolution::Rejected(RejectedDrop::new(
        scene.stamp(),
        surface,
        point,
        rejections,
    )))
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
