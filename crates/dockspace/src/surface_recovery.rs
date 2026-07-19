//! Frozen, atomic recovery plans for complete logical surface rosters.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::RootPresentationOwner;
use crate::command::{
    DockTarget, MovePayload, NodeSource, RootPresentationTarget, WorkspaceCommand,
};
use crate::coordinates::CoordinateSnapshot;
use crate::frame::{PanelFocus, ViewportCloseRequestId, ViewportMergeBackPlan};
use crate::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use crate::graph::{Node, Workspace};
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::scene::SceneStamp;
use crate::transaction::WorkspaceTransaction;
use crate::viewport::{CoordinateGeneration, ViewportBinding};

/// Target platform and scene facts frozen at one reducer batch boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SurfaceRecoveryTargetFacts {
    surface: SurfaceId,
    scene: SceneStamp,
    coordinates: CoordinateSnapshot,
    scene_bounds: LogicalRect,
}

impl SurfaceRecoveryTargetFacts {
    pub(crate) const fn new(
        surface: SurfaceId,
        scene: SceneStamp,
        coordinates: CoordinateSnapshot,
        scene_bounds: LogicalRect,
    ) -> Self {
        Self {
            surface,
            scene,
            coordinates,
            scene_bounds,
        }
    }

    pub(crate) const fn coordinates(self) -> CoordinateSnapshot {
        self.coordinates
    }

    pub(crate) const fn scene_bounds(self) -> LogicalRect {
        self.scene_bounds
    }

    pub(crate) fn dependency(self) -> SurfaceRecoveryTargetDependency {
        SurfaceRecoveryTargetDependency {
            surface: self.surface,
            scene: self.scene,
            binding: self.coordinates.binding(),
            coordinate_generation: self.coordinates.coordinate_generation(),
            content_bounds: self.coordinates.content_bounds(),
            scale_factor: self.coordinates.scale_factor(),
            scene_bounds: self.scene_bounds,
        }
    }

    pub(crate) fn satisfies(self, dependency: SurfaceRecoveryTargetDependency) -> bool {
        let current = self.dependency();
        current.surface == dependency.surface
            && current.scene >= dependency.scene
            && current.binding == dependency.binding
            && current.coordinate_generation == dependency.coordinate_generation
            && current.content_bounds == dependency.content_bounds
            && current.scale_factor == dependency.scale_factor
            && current.scene_bounds == dependency.scene_bounds
    }
}

/// Exact target facts on which an accepted merge-back geometry plan depends.
///
/// Binding, coordinate generation, the complete physical content rectangle,
/// scale, and target-local scene bounds must remain exact. The scene stamp is a
/// causal floor: a newer current scene may re-prove unchanged target facts after
/// an unrelated surface changes, but an older scene can never satisfy a later
/// dependency. Target topology remains frozen independently by
/// [`ViewportMergeBackPlan::target`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SurfaceRecoveryTargetDependency {
    surface: SurfaceId,
    scene: SceneStamp,
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
    content_bounds: PhysicalRect,
    scale_factor: ScaleFactor,
    scene_bounds: LogicalRect,
}

/// Exact source-side facts frozen for one contained root on a logical surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainedRootDisposition {
    floating: FloatingPresentationId,
    source: NodeSource,
    surface: SurfaceId,
    rect: LogicalRect,
    z_order: u64,
}

impl ContainedRootDisposition {
    /// Returns the stable contained-presentation identity.
    #[must_use]
    pub const fn floating(&self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the stable root presented by the contained window.
    #[must_use]
    pub fn root(&self) -> RootId {
        self.source.root()
    }

    /// Returns the exact source surface which owned the presentation.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the source-local rectangle frozen at the lifecycle edge.
    #[must_use]
    pub const fn rect(&self) -> LogicalRect {
        self.rect
    }

    /// Returns the explicit stacking order frozen at the lifecycle edge.
    #[must_use]
    pub const fn z_order(&self) -> u64 {
        self.z_order
    }
}

/// Exact `main_root + contained roots` ownership frozen at one lifecycle edge.
///
/// The contained sequence is normative. Recovery may compile only while the
/// complete current roster, ownership, geometry, stacking facts, and every
/// root's exact node fingerprint still match this snapshot. Transactions reuse
/// these close-edge sources instead of recapturing changed topology.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRosterDisposition {
    surface: SurfaceId,
    main: NodeSource,
    contained: Vec<ContainedRootDisposition>,
    source_coordinates: Option<CoordinateSnapshot>,
}

impl SurfaceRosterDisposition {
    /// Freezes the complete roster currently owned by `surface`.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRosterCaptureError`] when the requested surface or one
    /// of its declared contained records is absent or internally inconsistent.
    pub(crate) fn capture(
        workspace: &Workspace,
        surface: SurfaceId,
        source_coordinates: Option<CoordinateSnapshot>,
    ) -> Result<Self, SurfaceRosterCaptureError> {
        let presentation = workspace
            .surface(surface)
            .ok_or(SurfaceRosterCaptureError::MissingSurface { surface })?;
        let main = Self::capture_root_source(workspace, presentation.main_root)?;

        let mut contained = Vec::with_capacity(presentation.contained.len());
        for floating in presentation.contained.iter().copied() {
            let record = workspace
                .contained_floating(floating)
                .ok_or(SurfaceRosterCaptureError::MissingFloating { floating })?;
            if record.id != floating || record.surface != surface {
                return Err(SurfaceRosterCaptureError::ContainedOwnershipMismatch {
                    floating,
                    expected_surface: surface,
                    actual_surface: record.surface,
                });
            }
            contained.push(ContainedRootDisposition {
                floating,
                source: Self::capture_root_source(workspace, record.root)?,
                surface,
                rect: record.rect,
                z_order: record.z_order,
            });
        }

        Ok(Self {
            surface,
            main,
            contained,
            source_coordinates,
        })
    }

    /// Returns the source surface whose complete roster was frozen.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the stable main root frozen for the source surface.
    #[must_use]
    pub fn main_root(&self) -> RootId {
        self.main.root()
    }

    /// Returns contained dispositions in the source surface's normative order.
    #[must_use]
    pub fn contained(&self) -> &[ContainedRootDisposition] {
        &self.contained
    }

    /// Returns whether exact source content coordinates were authoritative at capture.
    #[must_use]
    pub const fn source_geometry_available(&self) -> bool {
        self.source_coordinates.is_some()
    }

    pub(crate) const fn source_coordinates(&self) -> Option<CoordinateSnapshot> {
        self.source_coordinates
    }

    /// Compiles direct destruction recovery for the main root and complete forest.
    pub(crate) fn compile_recovery_transaction(
        &self,
        workspace: &Workspace,
        placement: &SurfaceRosterPlacement,
        main_target: RootPresentationTarget,
    ) -> Option<WorkspaceTransaction> {
        if placement.target_surface == self.surface
            || !self.matches_workspace(workspace)
            || placement.contained.len() != self.contained.len()
        {
            return None;
        }

        let mut commands =
            self.compile_contained_commands(placement.target_surface, &placement.contained)?;
        commands.push(WorkspaceCommand::RehomeRoot {
            source: self.main.clone(),
            target: main_target,
        });
        Some(WorkspaceTransaction::from_commands(commands))
    }

    /// Compiles one Open-GPUI-compatible merge of main tabs plus floating forest.
    pub(crate) fn compile_merge_back_transaction(
        &self,
        workspace: &Workspace,
        placement: &SurfaceForestPlacement,
        plan: &ViewportMergeBackPlan,
    ) -> Option<WorkspaceTransaction> {
        if placement.target_surface == self.surface
            || placement.target_surface != plan.target_surface()
            || !self.matches_workspace(workspace)
            || placement.contained.len() != self.contained.len()
            || workspace.presentation_for_root(plan.target().root())
                != Some(RootPresentationOwner::Main {
                    surface: plan.target_surface(),
                })
            || workspace
                .capture_tab_target(plan.target().root(), plan.target().tabs())
                .ok()
                .as_ref()
                != Some(plan.target())
        {
            return None;
        }

        if !matches!(workspace.node(self.main.node()), Some(Node::Tabs { .. })) {
            return None;
        }
        let mut commands =
            self.compile_contained_commands(placement.target_surface, &placement.contained)?;
        commands.push(WorkspaceCommand::Move {
            payload: MovePayload::Tabs(self.main.clone()),
            target: DockTarget::Center(plan.target().clone()),
        });
        Some(WorkspaceTransaction::from_commands(commands))
    }

    fn compile_contained_commands(
        &self,
        target_surface: SurfaceId,
        placements: &[ContainedRootPlacement],
    ) -> Option<Vec<WorkspaceCommand>> {
        if placements.len() != self.contained.len() {
            return None;
        }
        let mut commands = Vec::with_capacity(self.contained.len() + 1);
        for (disposition, target) in self.contained.iter().zip(placements) {
            if target.floating != disposition.floating || target.root != disposition.root() {
                return None;
            }
            commands.push(WorkspaceCommand::RehomeRoot {
                source: disposition.source.clone(),
                target: RootPresentationTarget::Contained {
                    surface: target_surface,
                    floating: disposition.floating,
                    rect: target.rect,
                    z_order: target.z_order,
                },
            });
        }
        Some(commands)
    }

    pub(crate) fn contains_item(&self, workspace: &Workspace, item: crate::ids::ItemId) -> bool {
        std::iter::once(&self.main)
            .chain(self.contained.iter().map(|entry| &entry.source))
            .any(|source| {
                workspace.root(source.root()).is_some_and(|record| {
                    workspace
                        .collect_items_in_subtree(record.node)
                        .contains(&item)
                })
            })
    }

    pub(crate) fn matches_workspace(&self, workspace: &Workspace) -> bool {
        let Some(presentation) = workspace.surface(self.surface) else {
            return false;
        };
        let main_matches = presentation.main_root == self.main.root()
            && workspace.presentation_for_root(self.main.root())
                == Some(RootPresentationOwner::Main {
                    surface: self.surface,
                })
            && Self::source_matches_workspace(workspace, &self.main);
        if !main_matches || presentation.contained.len() != self.contained.len() {
            return false;
        }

        presentation
            .contained
            .iter()
            .zip(&self.contained)
            .all(|(floating, disposition)| {
                *floating == disposition.floating
                    && workspace
                        .contained_floating(*floating)
                        .is_some_and(|record| {
                            record.id == disposition.floating
                                && record.root == disposition.root()
                                && record.surface == disposition.surface
                                && record.rect == disposition.rect
                                && record.z_order == disposition.z_order
                                && workspace.presentation_for_root(record.root)
                                    == Some(RootPresentationOwner::Contained {
                                        surface: disposition.surface,
                                        floating: disposition.floating,
                                    })
                                && Self::source_matches_workspace(workspace, &disposition.source)
                        })
            })
    }

    fn capture_root_source(
        workspace: &Workspace,
        root: RootId,
    ) -> Result<NodeSource, SurfaceRosterCaptureError> {
        let record = workspace
            .root(root)
            .ok_or(SurfaceRosterCaptureError::MissingRoot { root })?;
        workspace
            .capture_node_source(root, record.node)
            .map_err(|_| SurfaceRosterCaptureError::RootFingerprintUnavailable { root })
    }

    fn source_matches_workspace(workspace: &Workspace, source: &NodeSource) -> bool {
        workspace
            .capture_node_source(source.root(), source.node())
            .is_ok_and(|current| &current == source)
    }
}

/// Target-local placement facts validated before one roster transaction is compiled.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRosterPlacement {
    target_surface: SurfaceId,
    main_rect: LogicalRect,
    main_z_order: u64,
    contained: Vec<ContainedRootPlacement>,
}

/// Target-local placement for the contained forest evacuated during merge-back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceForestPlacement {
    target_surface: SurfaceId,
    contained: Vec<ContainedRootPlacement>,
}

impl SurfaceForestPlacement {
    pub(crate) fn new(target_surface: SurfaceId, contained: Vec<ContainedRootPlacement>) -> Self {
        Self {
            target_surface,
            contained,
        }
    }
}

impl SurfaceRosterPlacement {
    pub(crate) fn new(
        target_surface: SurfaceId,
        main_rect: LogicalRect,
        main_z_order: u64,
        contained: Vec<ContainedRootPlacement>,
    ) -> Self {
        Self {
            target_surface,
            main_rect,
            main_z_order,
            contained,
        }
    }

    pub(crate) const fn target_surface(&self) -> SurfaceId {
        self.target_surface
    }

    pub(crate) const fn main_rect(&self) -> LogicalRect {
        self.main_rect
    }

    pub(crate) const fn main_z_order(&self) -> u64 {
        self.main_z_order
    }
}

/// One sibling's checked target-local geometry and stacking assignment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedRootPlacement {
    floating: FloatingPresentationId,
    root: RootId,
    rect: LogicalRect,
    z_order: u64,
}

impl ContainedRootPlacement {
    pub(crate) const fn new(
        floating: FloatingPresentationId,
        root: RootId,
        rect: LogicalRect,
        z_order: u64,
    ) -> Self {
        Self {
            floating,
            root,
            rect,
            z_order,
        }
    }
}

/// Engine-owned semantic state spanning native lifecycle observations.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SurfaceRecoveryState {
    close_focus: BTreeMap<ViewportCloseRequestId, PanelFocus>,
    accepted_closes: BTreeMap<ViewportCloseRequestId, AcceptedSurfaceMergeBack>,
    pending: BTreeMap<SurfaceId, PendingSurfaceRecovery>,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingSurfaceRecovery {
    roster: SurfaceRosterDisposition,
    disposition: PendingSurfaceRecoveryDisposition,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AcceptedSurfaceMergeBack {
    roster: SurfaceRosterDisposition,
    dependency: SurfaceRecoveryTargetDependency,
    focus: PanelFocus,
}

impl AcceptedSurfaceMergeBack {
    pub(crate) const fn roster(&self) -> &SurfaceRosterDisposition {
        &self.roster
    }

    pub(crate) const fn dependency(&self) -> SurfaceRecoveryTargetDependency {
        self.dependency
    }

    pub(crate) const fn focus(&self) -> PanelFocus {
        self.focus
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PendingSurfaceRecoveryDisposition {
    Contained,
    MergeBack {
        request: ViewportCloseRequestId,
        plan: ViewportMergeBackPlan,
        dependency: SurfaceRecoveryTargetDependency,
        focus: PanelFocus,
    },
}

impl SurfaceRecoveryState {
    pub(crate) fn freeze_close_focus(
        &mut self,
        request: ViewportCloseRequestId,
        focus: PanelFocus,
    ) {
        self.close_focus.entry(request).or_insert(focus);
    }

    pub(crate) fn close_focus(&self, request: ViewportCloseRequestId) -> Option<PanelFocus> {
        self.close_focus.get(&request).copied()
    }

    pub(crate) fn remove_close_focus(
        &mut self,
        request: ViewportCloseRequestId,
    ) -> Option<PanelFocus> {
        self.close_focus.remove(&request)
    }

    pub(crate) fn retain_close_focus(
        &mut self,
        mut retain: impl FnMut(ViewportCloseRequestId) -> bool,
    ) {
        self.close_focus.retain(|request, _| retain(*request));
    }

    pub(crate) fn freeze_accepted_close(
        &mut self,
        request: ViewportCloseRequestId,
        roster: SurfaceRosterDisposition,
        dependency: SurfaceRecoveryTargetDependency,
        focus: PanelFocus,
    ) {
        self.accepted_closes.insert(
            request,
            AcceptedSurfaceMergeBack {
                roster,
                dependency,
                focus,
            },
        );
    }

    pub(crate) fn remove_accepted_close(
        &mut self,
        request: ViewportCloseRequestId,
    ) -> Option<AcceptedSurfaceMergeBack> {
        self.accepted_closes.remove(&request)
    }

    pub(crate) fn accepted_close(
        &self,
        request: ViewportCloseRequestId,
    ) -> Option<&AcceptedSurfaceMergeBack> {
        self.accepted_closes.get(&request)
    }

    pub(crate) fn retain_accepted_closes(
        &mut self,
        mut retain: impl FnMut(ViewportCloseRequestId) -> bool,
    ) {
        self.accepted_closes.retain(|request, _| retain(*request));
    }

    pub(crate) fn defer(
        &mut self,
        roster: SurfaceRosterDisposition,
        disposition: PendingSurfaceRecoveryDisposition,
    ) -> bool {
        if let Some(current) = self.pending.get(&roster.surface()) {
            current.roster == roster && current.disposition == disposition
        } else {
            self.pending.insert(
                roster.surface(),
                PendingSurfaceRecovery {
                    roster,
                    disposition,
                },
            );
            true
        }
    }

    pub(crate) fn pending(&self, surface: SurfaceId) -> Option<&SurfaceRosterDisposition> {
        self.pending.get(&surface).map(|pending| &pending.roster)
    }

    pub(crate) fn pending_disposition(
        &self,
        surface: SurfaceId,
    ) -> Option<&PendingSurfaceRecoveryDisposition> {
        self.pending
            .get(&surface)
            .map(|pending| &pending.disposition)
    }

    pub(crate) fn first_workspace_mismatch(&self, workspace: &Workspace) -> Option<SurfaceId> {
        self.first_workspace_mismatch_other_than(workspace, None)
    }

    pub(crate) fn first_workspace_mismatch_excluding(
        &self,
        workspace: &Workspace,
        excluded: SurfaceId,
    ) -> Option<SurfaceId> {
        self.first_workspace_mismatch_other_than(workspace, Some(excluded))
    }

    fn first_workspace_mismatch_other_than(
        &self,
        workspace: &Workspace,
        excluded: Option<SurfaceId>,
    ) -> Option<SurfaceId> {
        self.accepted_closes
            .values()
            .map(AcceptedSurfaceMergeBack::roster)
            .chain(self.pending.values().map(|pending| &pending.roster))
            .filter(|roster| Some(roster.surface()) != excluded)
            .find_map(|roster| (!roster.matches_workspace(workspace)).then_some(roster.surface()))
    }

    pub(crate) fn complete_pending(
        &mut self,
        surface: SurfaceId,
    ) -> Option<SurfaceRosterDisposition> {
        self.pending.remove(&surface).map(|pending| pending.roster)
    }

    pub(crate) fn retain_pending(&mut self, mut retain: impl FnMut(SurfaceId) -> bool) {
        self.pending.retain(|surface, _| retain(*surface));
    }

    pub(crate) fn clear(&mut self) {
        self.close_focus.clear();
        self.accepted_closes.clear();
        self.pending.clear();
    }
}

/// Failure to freeze a complete source surface roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SurfaceRosterCaptureError {
    /// The source surface no longer exists.
    #[error("surface {surface} does not exist")]
    MissingSurface { surface: SurfaceId },
    /// A root referenced by the roster no longer exists.
    #[error("surface roster references missing root {root}")]
    MissingRoot { root: RootId },
    /// A declared root could not produce one complete, acyclic fingerprint.
    #[error("surface roster root {root} has no capturable fingerprint")]
    RootFingerprintUnavailable { root: RootId },
    /// A contained backlink references an absent presentation record.
    #[error("surface roster references missing floating presentation {floating}")]
    MissingFloating { floating: FloatingPresentationId },
    /// A contained record claims a different source surface than its backlink.
    #[error(
        "floating presentation {floating} belongs to surface {actual_surface}, not {expected_surface}"
    )]
    ContainedOwnershipMismatch {
        floating: FloatingPresentationId,
        expected_surface: SurfaceId,
        actual_surface: SurfaceId,
    },
}
