//! Frozen, atomic recovery plans for complete logical surface rosters.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::RootPresentationOwner;
use crate::command::{
    ContainedPosition, ContainedRootSource, DockTarget, MovePayload, NodeSource,
    RootPresentationTarget, SurfaceRosterSource, WorkspaceCommand,
};
use crate::coordinates::{CoordinateSnapshot, RecoveryCoordinateSnapshot};
use crate::geometry::{GeometryError, LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::{Node, Workspace};
use crate::ids::EngineAuthorityDomainId;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::policy::{DockPresentationMode, DockSurfaceRecoveryPolicyRequest, PolicyRevision};
use crate::scene::{ContainedMinimumMeasurement, SurfaceSceneStamp};
use crate::transaction::TransactionReport;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct RootRecoveryAnchorId(u64);

impl RootRecoveryAnchorId {
    pub(crate) const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Engine-issued durable authority naming one logical root recovery anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootRecoveryAnchor {
    authority_domain: EngineAuthorityDomainId,
    id: RootRecoveryAnchorId,
    surface: SurfaceId,
}

impl RootRecoveryAnchor {
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        id: RootRecoveryAnchorId,
        surface: SurfaceId,
    ) -> Self {
        Self {
            authority_domain,
            id,
            surface,
        }
    }

    pub(crate) const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    pub(crate) const fn id(self) -> RootRecoveryAnchorId {
        self.id
    }

    /// Returns the stable logical surface owned by this anchor.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Engine-local identity of one durable child-surface recovery authorization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct SurfaceRecoveryObligationId(u64);

impl SurfaceRecoveryObligationId {
    pub(crate) const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn new_for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Exact fallback reservation for converting one child surface's main root
/// into a contained presentation on its recovery host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConvertedMainRecovery {
    source_root: RootId,
    floating: FloatingPresentationId,
    minimum_size: LogicalSize,
}

impl ConvertedMainRecovery {
    /// Creates one exact converted-main reservation.
    #[must_use]
    pub const fn new(
        source_root: RootId,
        floating: FloatingPresentationId,
        minimum_size: LogicalSize,
    ) -> Self {
        Self {
            source_root,
            floating,
            minimum_size,
        }
    }

    /// Returns the main root which this reservation may convert.
    #[must_use]
    pub const fn source_root(self) -> RootId {
        self.source_root
    }

    /// Returns the reserved contained-presentation identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the frozen minimum outer size for the converted main root.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }
}

/// Adapter-supplied facts for enrolling an existing child surface for the first time.
///
/// The adapter names only the recovery host. The engine resolves the current host anchor, applies
/// its presentation configuration, and reserves any converted-main presentation identity while
/// reducing the registration, so adapters cannot mint or reuse semantic identities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceRecoveryBootstrap {
    host_surface: SurfaceId,
}

impl SurfaceRecoveryBootstrap {
    /// Creates one initial child recovery description.
    #[must_use]
    pub const fn new(host_surface: SurfaceId) -> Self {
        Self { host_surface }
    }

    /// Returns the root surface which receives this child if its native window is destroyed.
    #[must_use]
    pub const fn host_surface(self) -> SurfaceId {
        self.host_surface
    }
}

/// Durable recovery destination for one child surface binding.
///
/// The target never contains the source surface roster or placement geometry.
/// Those facts are captured atomically by [`SurfaceRosterDisposition`] at the
/// lifecycle edge where recovery is required.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceRecoveryTarget {
    anchor: RootRecoveryAnchor,
    converted_main: Option<ConvertedMainRecovery>,
}

impl SurfaceRecoveryTarget {
    /// Targets a host for a rootless child's complete contained forest.
    #[must_use]
    pub const fn forest_only(anchor: RootRecoveryAnchor) -> Self {
        Self {
            anchor,
            converted_main: None,
        }
    }

    /// Targets a host for a rooted child's converted main plus contained forest.
    #[must_use]
    pub const fn with_converted_main(
        anchor: RootRecoveryAnchor,
        converted_main: ConvertedMainRecovery,
    ) -> Self {
        Self {
            anchor,
            converted_main: Some(converted_main),
        }
    }

    /// Returns the exact engine-issued root recovery anchor.
    #[must_use]
    pub const fn anchor(self) -> RootRecoveryAnchor {
        self.anchor
    }

    /// Returns the logical surface which receives recovered content.
    #[must_use]
    pub const fn host_surface(self) -> SurfaceId {
        self.anchor.surface()
    }

    /// Returns the optional exact main-root conversion reservation.
    #[must_use]
    pub const fn converted_main(self) -> Option<ConvertedMainRecovery> {
        self.converted_main
    }
}

/// Durable policy authorization held by one live docking-owned child surface.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRecoveryObligation {
    id: SurfaceRecoveryObligationId,
    authority_domain: EngineAuthorityDomainId,
    request: DockSurfaceRecoveryPolicyRequest,
    target: SurfaceRecoveryTarget,
    policy_revision: PolicyRevision,
}

impl SurfaceRecoveryObligation {
    pub(crate) fn new(
        id: SurfaceRecoveryObligationId,
        authority_domain: EngineAuthorityDomainId,
        request: DockSurfaceRecoveryPolicyRequest,
        target: SurfaceRecoveryTarget,
        policy_revision: PolicyRevision,
    ) -> Result<Self, SurfaceRecoveryObligationError> {
        validate_obligation_shape(&request, target)?;
        Ok(Self {
            id,
            authority_domain,
            request,
            target,
            policy_revision,
        })
    }

    pub(crate) const fn id(&self) -> SurfaceRecoveryObligationId {
        self.id
    }

    pub(crate) const fn authority_domain(&self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    pub(crate) const fn request(&self) -> &DockSurfaceRecoveryPolicyRequest {
        &self.request
    }

    pub(crate) const fn target(&self) -> SurfaceRecoveryTarget {
        self.target
    }

    /// Preserves identity while authorizing changed semantic facts against a newer policy.
    pub(crate) fn reauthorized_for(
        &self,
        request: DockSurfaceRecoveryPolicyRequest,
        policy_revision: PolicyRevision,
    ) -> Result<Self, SurfaceRecoveryObligationError> {
        if request.source_surface() != self.request.source_surface()
            || request.host_surface() != self.request.host_surface()
        {
            return Err(SurfaceRecoveryObligationError::IdentityChanged);
        }
        let previous_main = recovery_native_root(&self.request)?;
        let next_main = recovery_native_root(&request)?;
        let target = match (self.target.converted_main(), previous_main, next_main) {
            (Some(converted), Some(previous), Some(next)) if previous == next => {
                if converted.source_root() != next {
                    return Err(SurfaceRecoveryObligationError::TargetShapeMismatch);
                }
                self.target
            }
            (Some(_), Some(_), None) => SurfaceRecoveryTarget::forest_only(self.target.anchor()),
            (None, None, None) => self.target,
            (Some(_), Some(_), Some(_)) | (None, None, Some(_)) => {
                return Err(SurfaceRecoveryObligationError::NewMainReservationRequired);
            }
            _ => return Err(SurfaceRecoveryObligationError::TargetShapeMismatch),
        };
        validate_obligation_shape(&request, target)?;
        Ok(Self {
            id: self.id,
            authority_domain: self.authority_domain,
            request,
            target,
            policy_revision,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum SurfaceRecoveryObligationError {
    #[error("recovery obligation source or host identity changed")]
    IdentityChanged,
    #[error("recovery obligation contains more than one native main root")]
    MultipleNativeRoots,
    #[error("recovery obligation target does not match its normalized source shape")]
    TargetShapeMismatch,
    #[error("rootless-to-rooted recovery requires a new converted-main reservation")]
    NewMainReservationRequired,
}

fn validate_obligation_shape(
    request: &DockSurfaceRecoveryPolicyRequest,
    target: SurfaceRecoveryTarget,
) -> Result<(), SurfaceRecoveryObligationError> {
    if request.host_surface() != target.host_surface() {
        return Err(SurfaceRecoveryObligationError::TargetShapeMismatch);
    }
    match (recovery_native_root(request)?, target.converted_main()) {
        (None, None) => Ok(()),
        (Some(root), Some(converted)) if root == converted.source_root() => Ok(()),
        (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => {
            Err(SurfaceRecoveryObligationError::TargetShapeMismatch)
        }
    }
}

fn recovery_native_root(
    request: &DockSurfaceRecoveryPolicyRequest,
) -> Result<Option<RootId>, SurfaceRecoveryObligationError> {
    let mut roots = request.roots().iter().filter_map(|(root, facts)| {
        (facts.source_presentation() == DockPresentationMode::Native).then_some(*root)
    });
    let root = roots.next();
    if roots.next().is_some() {
        return Err(SurfaceRecoveryObligationError::MultipleNativeRoots);
    }
    Ok(root)
}

/// Policy-independent atomic program for an already-obligated complete surface recovery.
///
/// Only this module's complete-roster compilers can construct the type. It deliberately exposes
/// no command mutation or conversion back to [`WorkspaceTransaction`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRecoveryTransaction {
    roster: SurfaceRosterSource,
    target_surface: SurfaceId,
    commands: Vec<WorkspaceCommand>,
}

impl SurfaceRecoveryTransaction {
    fn new(
        roster: SurfaceRosterSource,
        target_surface: SurfaceId,
        commands: Vec<WorkspaceCommand>,
    ) -> Option<Self> {
        let transaction = Self {
            roster,
            target_surface,
            commands,
        };
        transaction.is_exact_program().then_some(transaction)
    }

    pub(crate) fn commands(&self) -> &[WorkspaceCommand] {
        &self.commands
    }

    pub(crate) const fn roster(&self) -> &SurfaceRosterSource {
        &self.roster
    }

    /// Returns the exact destination surface frozen by this complete-roster
    /// transaction. Engine lifecycle code uses this only to create a
    /// post-commit focus intent; it never recomputes the target from current
    /// topology.
    pub(crate) const fn target_surface(&self) -> SurfaceId {
        self.target_surface
    }

    pub(crate) fn is_exact_program(&self) -> bool {
        self.target_surface != self.roster.surface() && self.is_exact_roster_rehome_program()
    }

    pub(crate) fn apply(
        &self,
        workspace: &mut Workspace,
    ) -> Result<TransactionReport, crate::error::TransactionError> {
        let prepared = crate::operation::prepare_surface_recovery_transaction(workspace, self)?;
        Ok(prepared.publish(workspace))
    }

    fn is_exact_roster_rehome_program(&self) -> bool {
        let mut commands = self.commands.iter();
        if let Some(main) = self.roster.main_source()
            && !commands
                .next()
                .is_some_and(|command| self.is_exact_main_rehome(command, main))
        {
            return false;
        }
        self.roster.contained().iter().all(|contained| {
            commands
                .next()
                .is_some_and(|command| self.is_exact_contained_rehome(command, contained))
        }) && commands.next().is_none()
    }

    fn is_exact_main_rehome(&self, command: &WorkspaceCommand, expected: &NodeSource) -> bool {
        match command {
            WorkspaceCommand::Move { payload, target }
                if target.surface() == self.target_surface =>
            {
                matches!(
                    payload,
                    MovePayload::Tabs(source) | MovePayload::Subtree(source)
                        if source == expected
                )
            }
            WorkspaceCommand::RehomeRoot { source, target } => {
                source == expected && root_presentation_surface(*target) == self.target_surface
            }
            _ => false,
        }
    }

    fn is_exact_contained_rehome(
        &self,
        command: &WorkspaceCommand,
        expected: &ContainedRootSource,
    ) -> bool {
        matches!(
            command,
            WorkspaceCommand::RehomeRoot {
                source,
                target: RootPresentationTarget::Contained {
                    surface,
                    floating,
                    position: ContainedPosition::Front,
                    ..
                },
            } if source == expected.source()
                && *surface == self.target_surface
                && *floating == expected.floating()
        )
    }
}

const fn root_presentation_surface(target: RootPresentationTarget) -> SurfaceId {
    match target {
        RootPresentationTarget::NewSurface { surface }
        | RootPresentationTarget::Main { surface }
        | RootPresentationTarget::Contained { surface, .. } => surface,
    }
}

/// Exact host platform and scene facts frozen at one reducer batch boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SurfaceRecoveryHostFacts {
    surface: SurfaceId,
    scene: SurfaceSceneStamp,
    coordinates: CoordinateSnapshot,
    scene_bounds: LogicalRect,
}

impl SurfaceRecoveryHostFacts {
    pub(crate) fn new(
        surface: SurfaceId,
        scene: SurfaceSceneStamp,
        coordinates: CoordinateSnapshot,
        scene_bounds: LogicalRect,
    ) -> Result<Self, SurfaceRecoveryError> {
        if scene.surface() != surface {
            return Err(SurfaceRecoveryError::HostSceneSurfaceMismatch {
                host_surface: surface,
                scene_surface: scene.surface(),
            });
        }
        Ok(Self {
            surface,
            scene,
            coordinates,
            scene_bounds,
        })
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn scene(self) -> SurfaceSceneStamp {
        self.scene
    }

    pub(crate) const fn coordinates(self) -> CoordinateSnapshot {
        self.coordinates
    }

    pub(crate) const fn scene_bounds(self) -> LogicalRect {
        self.scene_bounds
    }
}

/// Why one complete surface roster cannot be compiled into an exact recovery.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceRecoveryError {
    /// A surface cannot recover into itself.
    #[error("surface {surface} cannot recover into itself")]
    SourceIsRecoveryHost { surface: SurfaceId },
    /// Frozen host facts name a different surface from the durable target.
    #[error("recovery target names host {target_surface}, but frozen facts name {facts_surface}")]
    HostFactsMismatch {
        target_surface: SurfaceId,
        facts_surface: SurfaceId,
    },
    /// The frozen scene belongs to a different logical surface.
    #[error("recovery host {host_surface} cannot use scene for surface {scene_surface}")]
    HostSceneSurfaceMismatch {
        host_surface: SurfaceId,
        scene_surface: SurfaceId,
    },
    /// The logical host no longer exists.
    #[error("recovery host surface {surface} is missing")]
    MissingHostSurface { surface: SurfaceId },
    /// The frozen complete source roster no longer matches workspace topology.
    #[error("surface {surface} roster changed after recovery was frozen")]
    SourceRosterChanged { surface: SurfaceId },
    /// Rooted and rootless source/target shapes differ.
    #[error(
        "surface {surface} recovery shape mismatch: source main={source_has_main}, converted main={target_has_main}"
    )]
    ShapeMismatch {
        surface: SurfaceId,
        source_has_main: bool,
        target_has_main: bool,
    },
    /// The converted-main reservation names a different root.
    #[error("surface {surface} main root {source_root} does not match recovery root {target_root}")]
    ConvertedMainRootMismatch {
        surface: SurfaceId,
        source_root: RootId,
        target_root: RootId,
    },
    /// The reserved converted-main floating identity is already live.
    #[error("recovery floating identity {floating} is already in use")]
    FloatingCollision { floating: FloatingPresentationId },
    /// Source coordinates were not authoritative at the lifecycle edge.
    #[error("surface {surface} recovery lacks authoritative source coordinates")]
    SourceCoordinatesUnavailable { surface: SurfaceId },
    /// No exact outer-window anchor was ever observed for a rooted source surface.
    #[error("surface {surface} recovery lacks an authoritative source outer rectangle")]
    SourceOuterBoundsUnavailable { surface: SurfaceId },
    /// Contained minimum measurements were not authoritative at the lifecycle edge.
    #[error("surface {surface} recovery lacks complete contained minimum measurements")]
    ContainedMinimumsUnavailable { surface: SurfaceId },
    /// Frozen minimum measurements no longer align with the complete roster.
    #[error("surface {surface} contained minimum roster does not match its disposition")]
    ContainedMinimumRosterMismatch { surface: SurfaceId },
    /// Coordinate conversion failed despite frozen source and host facts.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// Deterministic clamping could not produce a representable contained rectangle.
    #[error("surface {surface} cannot represent recovered contained geometry")]
    UnrepresentableGeometry { surface: SurfaceId },
    /// The exact generated command program did not satisfy lifecycle preflight.
    #[error(transparent)]
    Transaction(#[from] crate::error::TransactionError),
    /// An internally generated program failed its exact-roster invariant.
    #[error("surface {surface} recovery compiler produced an invalid exact program")]
    InvalidProgram { surface: SurfaceId },
}

impl SurfaceRecoveryError {
    /// Returns whether a later authoritative frame may make this recovery compilable.
    #[must_use]
    pub const fn is_authority_gap(&self) -> bool {
        matches!(
            self,
            Self::SourceCoordinatesUnavailable { .. }
                | Self::SourceOuterBoundsUnavailable { .. }
                | Self::ContainedMinimumsUnavailable { .. }
        )
    }
}

fn clamp_recovery_rect(
    surface: SurfaceId,
    bounds: LogicalRect,
    requested: LogicalRect,
    minimum: LogicalSize,
) -> Result<LogicalRect, SurfaceRecoveryError> {
    let bounds_width = bounds.width();
    let bounds_height = bounds.height();
    let requested_width = requested.width();
    let requested_height = requested.height();
    if !bounds_width.is_finite()
        || !bounds_height.is_finite()
        || !requested_width.is_finite()
        || !requested_height.is_finite()
        || bounds_width <= 0.0
        || bounds_height <= 0.0
    {
        return Err(SurfaceRecoveryError::UnrepresentableGeometry { surface });
    }

    let width = requested_width.max(minimum.width()).min(bounds_width);
    let height = requested_height.max(minimum.height()).min(bounds_height);
    if width <= 0.0 || height <= 0.0 {
        return Err(SurfaceRecoveryError::UnrepresentableGeometry { surface });
    }
    let (min_x, max_x) = clamp_recovery_axis(bounds.x(), bounds.max().x(), requested.x(), width)
        .ok_or(SurfaceRecoveryError::UnrepresentableGeometry { surface })?;
    let (min_y, max_y) = clamp_recovery_axis(bounds.y(), bounds.max().y(), requested.y(), height)
        .ok_or(SurfaceRecoveryError::UnrepresentableGeometry { surface })?;
    let min = LogicalPoint::new(min_x, min_y)?;
    let max = LogicalPoint::new(max_x, max_y)?;
    let clamped = LogicalRect::from_min_max(min, max)?;
    if clamped.width() <= 0.0 || clamped.height() <= 0.0 {
        return Err(SurfaceRecoveryError::UnrepresentableGeometry { surface });
    }
    Ok(clamped)
}

fn clamp_recovery_axis(
    bounds_min: f64,
    bounds_max: f64,
    requested_min: f64,
    extent: f64,
) -> Option<(f64, f64)> {
    let latest_min = bounds_max - extent;
    let (minimum, maximum) = if requested_min <= bounds_min {
        (bounds_min, bounds_min + extent)
    } else if requested_min >= latest_min {
        (latest_min, bounds_max)
    } else {
        (requested_min, requested_min + extent)
    };
    (minimum.is_finite() && maximum.is_finite() && minimum <= maximum).then_some((minimum, maximum))
}

/// Exact source-side facts frozen for one contained root on a logical surface.
pub type ContainedRootDisposition = ContainedRootSource;

/// Exact optional-main plus contained-root ownership frozen at one lifecycle edge.
///
/// The contained sequence is normative. Recovery may compile only while the
/// complete current roster, ownership, geometry, and every
/// root's exact node fingerprint still match this snapshot. Transactions reuse
/// these frozen sources instead of recapturing changed topology.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRosterDisposition {
    roster: SurfaceRosterSource,
    source_coordinates: Option<RecoveryCoordinateSnapshot>,
    contained_minimum_authority: Option<ContainedMinimumAuthority>,
}

/// Source-scene authority frozen only for recovery placement.
#[derive(Debug, Clone, PartialEq)]
struct ContainedMinimumAuthority {
    scene: SurfaceSceneStamp,
    measurements: Vec<ContainedMinimumMeasurement>,
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
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    ) -> Result<Self, SurfaceRosterCaptureError> {
        let presentation = workspace
            .surface(surface)
            .ok_or(SurfaceRosterCaptureError::MissingSurface { surface })?;
        let main = presentation
            .main_root
            .map(|root| Self::capture_root_source(workspace, root))
            .transpose()?;

        let mut contained = Vec::with_capacity(presentation.contained.len());
        for floating in presentation.contained.iter().copied() {
            let record = workspace
                .contained_floating(floating)
                .ok_or(SurfaceRosterCaptureError::MissingFloating { floating })?;
            if workspace.presentation_for_root(record.root)
                != Some(RootPresentationOwner::Contained { surface, floating })
            {
                return Err(SurfaceRosterCaptureError::ContainedOwnershipMismatch {
                    floating,
                    expected_surface: surface,
                });
            }
            contained.push(ContainedRootSource::new(
                floating,
                Self::capture_root_source(workspace, record.root)?,
                record.rect,
            ));
        }

        Ok(Self {
            roster: SurfaceRosterSource::new(surface, main, contained),
            source_coordinates,
            contained_minimum_authority: None,
        })
    }

    /// Returns the source surface whose complete roster was frozen.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.roster.surface()
    }

    pub(crate) const fn roster(&self) -> &SurfaceRosterSource {
        &self.roster
    }

    /// Returns the stable main root frozen for the source surface, when present.
    #[must_use]
    pub fn main_root(&self) -> Option<RootId> {
        self.roster.main_root()
    }

    /// Returns contained dispositions in the source surface's normative order.
    #[must_use]
    pub fn contained(&self) -> &[ContainedRootDisposition] {
        self.roster.contained()
    }

    /// Returns whether exact source content coordinates were authoritative at capture.
    #[must_use]
    pub const fn source_geometry_available(&self) -> bool {
        self.source_coordinates.is_some()
    }

    pub(crate) const fn source_coordinates(&self) -> Option<RecoveryCoordinateSnapshot> {
        self.source_coordinates
    }

    /// Freezes exact logical minimum sizes from one authoritative source scene.
    ///
    /// Measurements are normalized into the source surface's normative contained
    /// order. Once frozen, later scenes cannot replace the close/destruction-edge
    /// facts retained by this disposition.
    pub(crate) fn freeze_contained_minimums(
        &mut self,
        scene: SurfaceSceneStamp,
        measurements: &[ContainedMinimumMeasurement],
    ) -> bool {
        if self.contained_minimum_authority.is_some() {
            return true;
        }
        if measurements.len() != self.contained().len() {
            return false;
        }

        let mut by_floating = BTreeMap::new();
        for measurement in measurements.iter().copied() {
            if by_floating
                .insert(measurement.floating(), measurement)
                .is_some()
            {
                return false;
            }
        }
        let mut ordered = Vec::with_capacity(self.contained().len());
        for disposition in self.contained() {
            let Some(measurement) = by_floating.remove(&disposition.floating()) else {
                return false;
            };
            ordered.push(measurement);
        }
        if !by_floating.is_empty() {
            return false;
        }

        self.contained_minimum_authority = Some(ContainedMinimumAuthority {
            scene,
            measurements: ordered,
        });
        true
    }

    /// Returns the exact source scene whose contained minima were frozen.
    #[must_use]
    pub const fn contained_minimum_scene(&self) -> Option<SurfaceSceneStamp> {
        match &self.contained_minimum_authority {
            Some(authority) => Some(authority.scene),
            None => None,
        }
    }

    pub(crate) fn contained_minimums(&self) -> Option<&[ContainedMinimumMeasurement]> {
        self.contained_minimum_authority
            .as_ref()
            .map(|authority| authority.measurements.as_slice())
    }

    pub(crate) fn has_contained_minimum_authority(&self) -> bool {
        self.roster.contained().is_empty() || self.contained_minimum_authority.is_some()
    }

    /// Compiles direct destruction recovery for the optional main and complete forest.
    ///
    /// This is the only production compiler for unplanned destruction recovery.
    /// It derives every destination rectangle from the frozen complete source
    /// roster and one exact host fact set; callers cannot supply partial or
    /// reordered placement records.
    pub(crate) fn compile_recovery_transaction(
        &self,
        workspace: &Workspace,
        target: SurfaceRecoveryTarget,
        host: SurfaceRecoveryHostFacts,
    ) -> Result<SurfaceRecoveryTransaction, SurfaceRecoveryError> {
        let host_surface = target.host_surface();
        if host_surface == self.surface() {
            return Err(SurfaceRecoveryError::SourceIsRecoveryHost {
                surface: self.surface(),
            });
        }
        if host.surface() != host_surface {
            return Err(SurfaceRecoveryError::HostFactsMismatch {
                target_surface: host_surface,
                facts_surface: host.surface(),
            });
        }
        if host.scene().surface() != host_surface {
            return Err(SurfaceRecoveryError::HostSceneSurfaceMismatch {
                host_surface,
                scene_surface: host.scene().surface(),
            });
        }
        if workspace.surface(host_surface).is_none() {
            return Err(SurfaceRecoveryError::MissingHostSurface {
                surface: host_surface,
            });
        }
        if !self.matches_workspace(workspace) {
            return Err(SurfaceRecoveryError::SourceRosterChanged {
                surface: self.surface(),
            });
        }

        let source_coordinates = (self.roster.main_source().is_some()
            || !self.contained().is_empty())
        .then(|| {
            self.source_coordinates
                .ok_or(SurfaceRecoveryError::SourceCoordinatesUnavailable {
                    surface: self.surface(),
                })
        })
        .transpose()?;
        let converted_main =
            self.compile_converted_main_placement(workspace, target, host, source_coordinates)?;
        let contained = self.compile_recovery_contained_placements(host, source_coordinates)?;

        let mut commands = Vec::with_capacity(self.contained().len().saturating_add(1));
        match (self.roster.main_source(), converted_main) {
            (Some(source), Some(target)) => {
                commands.push(Self::compile_contained_command(
                    source.clone(),
                    host_surface,
                    target,
                ));
            }
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {
                return Err(SurfaceRecoveryError::ShapeMismatch {
                    surface: self.surface(),
                    source_has_main: self.roster.main_source().is_some(),
                    target_has_main: converted_main.is_some(),
                });
            }
        }
        commands.extend(
            self.compile_contained_commands(host_surface, &contained)
                .ok_or(SurfaceRecoveryError::ContainedMinimumRosterMismatch {
                    surface: self.surface(),
                })?,
        );
        let transaction =
            SurfaceRecoveryTransaction::new(self.roster.clone(), host_surface, commands).ok_or(
                SurfaceRecoveryError::InvalidProgram {
                    surface: self.surface(),
                },
            )?;
        crate::operation::prepare_surface_recovery_transaction(workspace, &transaction)?;
        Ok(transaction)
    }

    fn compile_converted_main_placement(
        &self,
        workspace: &Workspace,
        target: SurfaceRecoveryTarget,
        host: SurfaceRecoveryHostFacts,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    ) -> Result<Option<ContainedRootPlacement>, SurfaceRecoveryError> {
        let converted = match (self.roster.main_source(), target.converted_main()) {
            (None, None) => return Ok(None),
            (Some(source), Some(converted)) => {
                if source.root() != converted.source_root() {
                    return Err(SurfaceRecoveryError::ConvertedMainRootMismatch {
                        surface: self.surface(),
                        source_root: source.root(),
                        target_root: converted.source_root(),
                    });
                }
                converted
            }
            (source, converted) => {
                return Err(SurfaceRecoveryError::ShapeMismatch {
                    surface: self.surface(),
                    source_has_main: source.is_some(),
                    target_has_main: converted.is_some(),
                });
            }
        };
        if workspace.contained_floating(converted.floating()).is_some() {
            return Err(SurfaceRecoveryError::FloatingCollision {
                floating: converted.floating(),
            });
        }
        let source_coordinates =
            source_coordinates.ok_or(SurfaceRecoveryError::SourceCoordinatesUnavailable {
                surface: self.surface(),
            })?;
        let source_outer_bounds = source_coordinates.outer_bounds().ok_or(
            SurfaceRecoveryError::SourceOuterBoundsUnavailable {
                surface: self.surface(),
            },
        )?;
        let requested = host
            .coordinates()
            .desktop_rect_to_surface(source_outer_bounds)?;
        let rect = clamp_recovery_rect(
            host.surface(),
            host.scene_bounds(),
            requested,
            converted.minimum_size(),
        )?;
        Ok(Some(ContainedRootPlacement::new(
            converted.floating(),
            converted.source_root(),
            rect,
        )))
    }

    fn compile_recovery_contained_placements(
        &self,
        host: SurfaceRecoveryHostFacts,
        source_coordinates: Option<RecoveryCoordinateSnapshot>,
    ) -> Result<Vec<ContainedRootPlacement>, SurfaceRecoveryError> {
        if self.contained().is_empty() {
            return Ok(Vec::new());
        }
        let source_coordinates =
            source_coordinates.ok_or(SurfaceRecoveryError::SourceCoordinatesUnavailable {
                surface: self.surface(),
            })?;
        let minimums = self.contained_minimums().ok_or(
            SurfaceRecoveryError::ContainedMinimumsUnavailable {
                surface: self.surface(),
            },
        )?;
        if minimums.len() != self.contained().len() {
            return Err(SurfaceRecoveryError::ContainedMinimumRosterMismatch {
                surface: self.surface(),
            });
        }

        let mut placements = Vec::with_capacity(self.contained().len());
        for (disposition, minimum) in self.contained().iter().zip(minimums) {
            if disposition.floating() != minimum.floating() {
                return Err(SurfaceRecoveryError::ContainedMinimumRosterMismatch {
                    surface: self.surface(),
                });
            }
            let desktop = source_coordinates
                .content()
                .surface_rect_to_desktop(disposition.rect())?;
            let requested = host.coordinates().desktop_rect_to_surface(desktop)?;
            let rect = clamp_recovery_rect(
                host.surface(),
                host.scene_bounds(),
                requested,
                minimum.minimum_size(),
            )?;
            placements.push(ContainedRootPlacement::new(
                disposition.floating(),
                disposition.root(),
                rect,
            ));
        }
        Ok(placements)
    }

    #[cfg(test)]
    fn compile_recovery_placement_transaction(
        &self,
        workspace: &Workspace,
        placement: &SurfaceRosterPlacement,
    ) -> Option<SurfaceRecoveryTransaction> {
        if placement.target_surface == self.surface()
            || !self.matches_workspace(workspace)
            || workspace.surface(placement.target_surface).is_none()
            || placement.contained.len() != self.contained().len()
        {
            return None;
        }

        let mut commands = Vec::with_capacity(self.contained().len().saturating_add(1));
        match (self.roster.main_source(), placement.converted_main) {
            (Some(source), Some(target))
                if source.root() == target.root
                    && workspace.contained_floating(target.floating).is_none() =>
            {
                commands.push(Self::compile_contained_command(
                    source.clone(),
                    placement.target_surface,
                    target,
                ));
            }
            (None, None) => {}
            (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => return None,
        }
        commands.extend(
            self.compile_contained_commands(placement.target_surface, &placement.contained)?,
        );
        SurfaceRecoveryTransaction::new(self.roster.clone(), placement.target_surface, commands)
    }

    /// Compiles one planned complete-roster rehome.
    ///
    /// The optional main root has one explicit destination: either an exact
    /// existing docking target or one exact presentation owner. Every contained
    /// root remains a contained presentation with its frozen identity and a
    /// caller-supplied destination rectangle. The candidate is preflighted
    /// without policy so invalid target facts fail before a lifecycle plan is
    /// accepted; publication repeats the frozen roster validation atomically.
    pub(crate) fn compile_rehome_transaction(
        &self,
        workspace: &Workspace,
        placement: &SurfaceRehomePlacement,
    ) -> Option<SurfaceRecoveryTransaction> {
        if placement.target_surface == self.surface()
            || !self.matches_workspace(workspace)
            || placement.contained.len() != self.contained().len()
        {
            return None;
        }

        let mut commands = Vec::with_capacity(self.contained().len().saturating_add(1));
        match (self.roster.main_source(), placement.main.as_ref()) {
            (Some(source), Some(main)) => commands.push(Self::compile_main_rehome_command(
                workspace,
                placement.target_surface,
                source,
                main,
            )?),
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => return None,
        }
        commands.extend(
            self.compile_contained_commands(placement.target_surface, &placement.contained)?,
        );
        let transaction = SurfaceRecoveryTransaction::new(
            self.roster.clone(),
            placement.target_surface,
            commands,
        )?;
        crate::operation::prepare_surface_recovery_transaction(workspace, &transaction).ok()?;
        Some(transaction)
    }

    fn compile_main_rehome_command(
        workspace: &Workspace,
        target_surface: SurfaceId,
        source: &NodeSource,
        placement: &SurfaceMainRehome,
    ) -> Option<WorkspaceCommand> {
        match placement {
            SurfaceMainRehome::Dock(target) if target.surface() == target_surface => {
                let payload = match workspace.node(source.node())? {
                    Node::Tabs { .. } => MovePayload::Tabs(source.clone()),
                    Node::Split { .. } => MovePayload::Subtree(source.clone()),
                };
                Some(WorkspaceCommand::Move {
                    payload,
                    target: target.clone(),
                })
            }
            SurfaceMainRehome::Present(target)
                if root_presentation_surface(*target) == target_surface =>
            {
                Some(WorkspaceCommand::RehomeRoot {
                    source: source.clone(),
                    target: *target,
                })
            }
            SurfaceMainRehome::Dock(_) | SurfaceMainRehome::Present(_) => None,
        }
    }

    fn compile_contained_commands(
        &self,
        target_surface: SurfaceId,
        placements: &[ContainedRootPlacement],
    ) -> Option<Vec<WorkspaceCommand>> {
        if placements.len() != self.contained().len() {
            return None;
        }
        let mut commands = Vec::with_capacity(self.contained().len() + 1);
        for (disposition, target) in self.contained().iter().zip(placements) {
            if target.floating != disposition.floating() || target.root != disposition.root() {
                return None;
            }
            commands.push(Self::compile_contained_command(
                disposition.source().clone(),
                target_surface,
                *target,
            ));
        }
        Some(commands)
    }

    fn compile_contained_command(
        source: NodeSource,
        target_surface: SurfaceId,
        target: ContainedRootPlacement,
    ) -> WorkspaceCommand {
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: target_surface,
                floating: target.floating,
                rect: target.rect,
                position: ContainedPosition::Front,
            },
        }
    }

    pub(crate) fn matches_workspace(&self, workspace: &Workspace) -> bool {
        workspace.matches_surface_roster_source(&self.roster)
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
}

/// Target-local placement facts validated before one roster transaction is compiled.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRosterPlacement {
    target_surface: SurfaceId,
    converted_main: Option<ContainedRootPlacement>,
    contained: Vec<ContainedRootPlacement>,
}

/// Explicit destination for the optional main root during a complete-surface rehome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SurfaceMainRehome {
    /// Merge the exact complete main node into an existing, frozen docking target.
    Dock(DockTarget),
    /// Preserve the complete root and move it to one explicit presentation owner.
    Present(RootPresentationTarget),
}

/// Complete placement for a planned rehome of one logical surface roster.
///
/// `main` is present if and only if the frozen source roster has a main root.
/// Contained entries are normative back-to-front and preserve each frozen
/// floating-presentation identity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRehomePlacement {
    target_surface: SurfaceId,
    main: Option<SurfaceMainRehome>,
    contained: Vec<ContainedRootPlacement>,
}

#[cfg(test)]
impl SurfaceRosterPlacement {
    pub(crate) fn new(
        target_surface: SurfaceId,
        converted_main: Option<ContainedRootPlacement>,
        contained: Vec<ContainedRootPlacement>,
    ) -> Self {
        Self {
            target_surface,
            converted_main,
            contained,
        }
    }
}

impl SurfaceRehomePlacement {
    pub(crate) fn new(
        target_surface: SurfaceId,
        main: Option<SurfaceMainRehome>,
        contained: Vec<ContainedRootPlacement>,
    ) -> Self {
        Self {
            target_surface,
            main,
            contained,
        }
    }
}

/// One sibling's checked target-local geometry and structural-order assignment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedRootPlacement {
    floating: FloatingPresentationId,
    root: RootId,
    rect: LogicalRect,
}

impl ContainedRootPlacement {
    pub(crate) const fn new(
        floating: FloatingPresentationId,
        root: RootId,
        rect: LogicalRect,
    ) -> Self {
        Self {
            floating,
            root,
            rect,
        }
    }
}

/// Engine-owned pending state for complete-roster unplanned destruction recovery.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SurfaceRecoveryState {
    pending: BTreeMap<SurfaceId, SurfaceRosterDisposition>,
    blocked: BTreeMap<SurfaceId, SurfaceRecoveryBlockedReason>,
}

impl SurfaceRecoveryState {
    pub(crate) fn defer(&mut self, roster: SurfaceRosterDisposition) -> bool {
        if let Some(current) = self.pending.get(&roster.surface()) {
            current == &roster
        } else {
            self.pending.insert(roster.surface(), roster);
            true
        }
    }

    pub(crate) fn pending(&self, surface: SurfaceId) -> Option<&SurfaceRosterDisposition> {
        self.pending.get(&surface)
    }

    pub(crate) fn mark_blocked(
        &mut self,
        surface: SurfaceId,
        reason: SurfaceRecoveryBlockedReason,
    ) {
        self.blocked.insert(surface, reason);
    }

    pub(crate) fn blocked(&self, surface: SurfaceId) -> Option<&SurfaceRecoveryBlockedReason> {
        self.blocked.get(&surface)
    }

    pub(crate) fn update_pending_roster(&mut self, roster: SurfaceRosterDisposition) -> bool {
        let Some(pending) = self.pending.get_mut(&roster.surface()) else {
            return false;
        };
        if pending.roster != roster.roster
            || pending.source_coordinates != roster.source_coordinates
        {
            return false;
        }
        match (
            &pending.contained_minimum_authority,
            &roster.contained_minimum_authority,
        ) {
            (Some(current), Some(candidate)) if current == candidate => true,
            (Some(_), _) => false,
            (None, _) => {
                *pending = roster;
                true
            }
        }
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
        self.pending
            .values()
            .filter(|roster| Some(roster.surface()) != excluded)
            .find_map(|roster| (!roster.matches_workspace(workspace)).then_some(roster.surface()))
    }

    pub(crate) fn complete_pending(
        &mut self,
        surface: SurfaceId,
    ) -> Option<SurfaceRosterDisposition> {
        self.blocked.remove(&surface);
        self.pending.remove(&surface)
    }

    pub(crate) fn retain_pending(&mut self, mut retain: impl FnMut(SurfaceId) -> bool) {
        self.pending.retain(|surface, _| retain(*surface));
        self.blocked
            .retain(|surface, _| self.pending.contains_key(surface));
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.blocked.clear();
    }
}

/// Typed reason why a destroyed child surface remains retained for a later retry.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceRecoveryBlockedReason {
    /// The destruction edge did not match the engine-owned durable obligation.
    #[error("destroyed surface recovery obligation is missing or mismatched")]
    ObligationMismatch,
    /// The exact recovery host has no current measured platform authority.
    #[error("recovery host surface {surface} is not currently authoritative")]
    HostAuthorityUnavailable {
        /// Exact root recovery anchor surface.
        surface: SurfaceId,
    },
    /// Complete-roster recovery compilation is deterministically blocked.
    #[error(transparent)]
    Compile(#[from] SurfaceRecoveryError),
    /// The generated atomic recovery program could not be published yet.
    #[error("the exact recovery program is currently rejected")]
    ProgramRejected,
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
    /// A contained record is not owned by its declaring surface roster.
    #[error("floating presentation {floating} is not owned by surface {expected_surface}")]
    ContainedOwnershipMismatch {
        floating: FloatingPresentationId,
        expected_surface: SurfaceId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DockTarget;
    use crate::error::{TransactionError, TransactionPreconditionError};
    use crate::graph::{
        ContainedFloating, Node, RootRecord, SurfacePresentation, WorkspaceBuilder,
    };
    use crate::ids::{ItemId, NodeId};
    use crate::policy::{
        DockPolicy, DockPolicySnapshot, DockSurfaceRule, PolicyRejection, PolicyRevision,
    };
    use crate::transaction::WorkspaceTransaction;

    const SOURCE: SurfaceId = SurfaceId::new(1);
    const TARGET: SurfaceId = SurfaceId::new(2);
    const MISSING_TARGET: SurfaceId = SurfaceId::new(3);

    const MAIN: RootId = RootId::new(10);
    const ROOT_A: RootId = RootId::new(11);
    const ROOT_B: RootId = RootId::new(12);
    const TARGET_MAIN: RootId = RootId::new(20);
    const TARGET_EXISTING_ROOT: RootId = RootId::new(21);

    const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(102);
    const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(101);
    const TARGET_EXISTING: FloatingPresentationId = FloatingPresentationId::new(201);
    const CONVERTED_MAIN: FloatingPresentationId = FloatingPresentationId::new(202);

    fn rect(offset: f64) -> LogicalRect {
        LogicalRect::new(offset, offset, 320.0, 240.0).expect("test rectangle must be valid")
    }

    fn insert_root(builder: &mut WorkspaceBuilder, root: RootId, item: u64) -> NodeId {
        let node = builder.insert_node(Node::tabs([ItemId::new(item)]));
        builder.set_root(root, RootRecord::new(node));
        node
    }

    fn workspace_with_surfaces(
        source_main: Option<RootId>,
        target_main: Option<RootId>,
    ) -> Workspace {
        let mut builder = Workspace::builder();
        if let Some(root) = source_main {
            insert_root(&mut builder, root, 1);
        }
        insert_root(&mut builder, ROOT_A, 2);
        insert_root(&mut builder, ROOT_B, 3);
        if let Some(root) = target_main {
            insert_root(&mut builder, root, 4);
        }
        insert_root(&mut builder, TARGET_EXISTING_ROOT, 5);

        builder.set_surface(
            SOURCE,
            source_main.map_or_else(
                SurfacePresentation::rootless,
                SurfacePresentation::with_main,
            ),
        );
        builder.set_surface(
            TARGET,
            target_main.map_or_else(
                SurfacePresentation::rootless,
                SurfacePresentation::with_main,
            ),
        );
        for (surface, floating, root, bounds) in [
            (SOURCE, FLOATING_A, ROOT_A, rect(20.0)),
            (SOURCE, FLOATING_B, ROOT_B, rect(10.0)),
            (TARGET, TARGET_EXISTING, TARGET_EXISTING_ROOT, rect(30.0)),
        ] {
            builder.set_contained_floating(floating, ContainedFloating::new(root, bounds));
            builder
                .attach_contained(surface, floating)
                .expect("fixture surface must exist");
        }
        builder.build().expect("fixture workspace must be valid")
    }

    fn workspace_with_source(main: Option<RootId>) -> Workspace {
        workspace_with_surfaces(main, Some(TARGET_MAIN))
    }

    fn contained_placements() -> Vec<ContainedRootPlacement> {
        vec![
            ContainedRootPlacement::new(FLOATING_A, ROOT_A, rect(120.0)),
            ContainedRootPlacement::new(FLOATING_B, ROOT_B, rect(110.0)),
        ]
    }

    fn converted_main() -> ContainedRootPlacement {
        ContainedRootPlacement::new(CONVERTED_MAIN, MAIN, rect(100.0))
    }

    fn assert_front_rehome(
        command: &WorkspaceCommand,
        expected_root: RootId,
        expected_floating: FloatingPresentationId,
    ) {
        assert!(matches!(
            command,
            WorkspaceCommand::RehomeRoot {
                source,
                target: RootPresentationTarget::Contained {
                    surface: TARGET,
                    floating,
                    position: ContainedPosition::Front,
                    ..
                },
            } if source.root() == expected_root && *floating == expected_floating
        ));
    }

    #[test]
    fn rooted_recovery_appends_converted_main_then_frozen_contained_roster() {
        let mut workspace = workspace_with_source(Some(MAIN));
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let placement =
            SurfaceRosterPlacement::new(TARGET, Some(converted_main()), contained_placements());

        let transaction = roster
            .compile_recovery_placement_transaction(&workspace, &placement)
            .expect("exact rooted roster must compile");
        assert_eq!(transaction.commands().len(), 3);
        assert_front_rehome(&transaction.commands()[0], MAIN, CONVERTED_MAIN);
        assert_front_rehome(&transaction.commands()[1], ROOT_A, FLOATING_A);
        assert_front_rehome(&transaction.commands()[2], ROOT_B, FLOATING_B);

        transaction
            .apply(&mut workspace)
            .expect("rooted recovery must publish atomically");
        assert!(workspace.surface(SOURCE).is_none());
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive")
                .contained,
            [TARGET_EXISTING, CONVERTED_MAIN, FLOATING_A, FLOATING_B]
        );
    }

    #[test]
    fn rootless_recovery_appends_only_the_frozen_contained_roster() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let placement = SurfaceRosterPlacement::new(TARGET, None, contained_placements());

        let transaction = roster
            .compile_recovery_placement_transaction(&workspace, &placement)
            .expect("exact rootless roster must compile without inventing a main root");
        assert_eq!(transaction.commands().len(), 2);
        assert_front_rehome(&transaction.commands()[0], ROOT_A, FLOATING_A);
        assert_front_rehome(&transaction.commands()[1], ROOT_B, FLOATING_B);

        transaction
            .apply(&mut workspace)
            .expect("rootless recovery must publish atomically");
        assert!(workspace.surface(SOURCE).is_none());
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive")
                .contained,
            [TARGET_EXISTING, FLOATING_A, FLOATING_B]
        );
    }

    #[test]
    fn rootless_rehome_moves_the_complete_forest_without_a_main_command() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let placement = SurfaceRehomePlacement::new(TARGET, None, contained_placements());

        let transaction = roster
            .compile_rehome_transaction(&workspace, &placement)
            .expect("rootless RehomeAll must compile without inventing a main command");
        assert_eq!(transaction.commands().len(), 2);
        assert_front_rehome(&transaction.commands()[0], ROOT_A, FLOATING_A);
        assert_front_rehome(&transaction.commands()[1], ROOT_B, FLOATING_B);

        transaction
            .apply(&mut workspace)
            .expect("rootless RehomeAll must publish atomically");
        assert!(workspace.surface(SOURCE).is_none());
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive")
                .contained,
            [TARGET_EXISTING, FLOATING_A, FLOATING_B]
        );
    }

    #[test]
    fn rehome_can_present_the_main_on_an_exact_rootless_target() {
        let mut workspace = workspace_with_surfaces(Some(MAIN), None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let placement = SurfaceRehomePlacement::new(
            TARGET,
            Some(SurfaceMainRehome::Present(RootPresentationTarget::Main {
                surface: TARGET,
            })),
            contained_placements(),
        );

        let transaction = roster
            .compile_rehome_transaction(&workspace, &placement)
            .expect("an exact rootless main target must compile");
        assert!(matches!(
            transaction.commands().first(),
            Some(WorkspaceCommand::RehomeRoot {
                source,
                target: RootPresentationTarget::Main { surface: TARGET },
            }) if source == roster.roster().main_source().expect("source main must be frozen")
        ));

        transaction
            .apply(&mut workspace)
            .expect("main presentation plus contained forest must publish atomically");
        assert!(workspace.surface(SOURCE).is_none());
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive"),
            &SurfacePresentation {
                main_root: Some(MAIN),
                contained: vec![TARGET_EXISTING, FLOATING_A, FLOATING_B],
            }
        );
    }

    #[test]
    fn rehome_can_dock_the_exact_complete_main_payload() {
        let mut workspace = workspace_with_source(Some(MAIN));
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let target_tabs = workspace
            .root(TARGET_MAIN)
            .expect("target main root must exist")
            .node;
        let target = DockTarget::Center(
            workspace
                .capture_tab_target(TARGET_MAIN, target_tabs)
                .expect("exact target tabs must be capturable"),
        );
        let placement = SurfaceRehomePlacement::new(
            TARGET,
            Some(SurfaceMainRehome::Dock(target.clone())),
            contained_placements(),
        );

        let transaction = roster
            .compile_rehome_transaction(&workspace, &placement)
            .expect("the exact complete main payload must compile");
        assert!(matches!(
            transaction.commands().first(),
            Some(WorkspaceCommand::Move {
                payload: MovePayload::Tabs(source),
                target: actual,
            }) if source == roster.roster().main_source().expect("source main must be frozen")
                && actual == &target
        ));

        transaction
            .apply(&mut workspace)
            .expect("docked main plus contained forest must publish atomically");
        assert!(workspace.surface(SOURCE).is_none());
        assert!(workspace.root(MAIN).is_none());
        assert_eq!(
            workspace.collect_items_in_subtree(
                workspace
                    .root(TARGET_MAIN)
                    .expect("target main root must survive")
                    .node,
            ),
            [ItemId::new(4), ItemId::new(1)]
        );
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive")
                .contained,
            [TARGET_EXISTING, FLOATING_A, FLOATING_B]
        );
    }

    #[test]
    fn lifecycle_apply_rejects_an_injected_command_before_mutating_the_roster() {
        let mut workspace = workspace_with_source(Some(MAIN));
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let target_tabs = workspace
            .root(TARGET_MAIN)
            .expect("target main root must exist")
            .node;
        let target = DockTarget::Center(
            workspace
                .capture_tab_target(TARGET_MAIN, target_tabs)
                .expect("exact target tabs must be capturable"),
        );
        let transaction = roster
            .compile_rehome_transaction(
                &workspace,
                &SurfaceRehomePlacement::new(
                    TARGET,
                    Some(SurfaceMainRehome::Dock(target)),
                    contained_placements(),
                ),
            )
            .expect("valid RehomeAll must compile");
        let mut forged = transaction.clone();
        forged.commands.push(
            transaction
                .commands()
                .first()
                .expect("main command must exist")
                .clone(),
        );
        let before = workspace.clone();

        assert!(!forged.is_exact_program());
        let error = forged
            .apply(&mut workspace)
            .expect_err("lifecycle authority must reject arbitrary command injection");
        assert!(matches!(
            error,
            TransactionError::Command {
                source: crate::error::CommandError::Invariant {
                    stage: "authorize exact surface recovery program",
                },
                ..
            }
        ));
        assert_eq!(workspace, before);
    }

    #[test]
    fn rehome_compile_rejects_missing_mismatched_or_partial_placement() {
        let rooted = workspace_with_source(Some(MAIN));
        let rooted_roster = SurfaceRosterDisposition::capture(&rooted, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let target_tabs = rooted
            .root(TARGET_MAIN)
            .expect("target main root must exist")
            .node;
        let target = DockTarget::Center(
            rooted
                .capture_tab_target(TARGET_MAIN, target_tabs)
                .expect("exact target tabs must be capturable"),
        );

        assert!(
            rooted_roster
                .compile_rehome_transaction(
                    &rooted,
                    &SurfaceRehomePlacement::new(TARGET, None, contained_placements()),
                )
                .is_none(),
            "a rooted roster requires one explicit main placement"
        );
        assert!(
            rooted_roster
                .compile_rehome_transaction(
                    &rooted,
                    &SurfaceRehomePlacement::new(
                        MISSING_TARGET,
                        Some(SurfaceMainRehome::Dock(target)),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "the explicit dock target must belong to the declared target surface"
        );
        assert!(
            rooted_roster
                .compile_rehome_transaction(
                    &rooted,
                    &SurfaceRehomePlacement::new(
                        TARGET,
                        Some(SurfaceMainRehome::Present(RootPresentationTarget::Main {
                            surface: TARGET,
                        })),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "an occupied main target cannot accept a presented main root"
        );
        assert!(
            rooted_roster
                .compile_rehome_transaction(
                    &rooted,
                    &SurfaceRehomePlacement::new(
                        TARGET,
                        Some(SurfaceMainRehome::Present(
                            RootPresentationTarget::Contained {
                                surface: MISSING_TARGET,
                                floating: CONVERTED_MAIN,
                                rect: rect(100.0),
                                position: ContainedPosition::Front,
                            }
                        )),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "a presentation target cannot be relabeled onto another surface"
        );
        assert!(
            rooted_roster
                .compile_rehome_transaction(
                    &rooted,
                    &SurfaceRehomePlacement::new(
                        TARGET,
                        Some(SurfaceMainRehome::Present(
                            RootPresentationTarget::Contained {
                                surface: TARGET,
                                floating: CONVERTED_MAIN,
                                rect: rect(100.0),
                                position: ContainedPosition::Front,
                            },
                        )),
                        contained_placements()[..1].to_vec(),
                    ),
                )
                .is_none(),
            "every contained root must have a normative placement"
        );

        let rootless = workspace_with_source(None);
        let rootless_roster = SurfaceRosterDisposition::capture(&rootless, SOURCE, None)
            .expect("rootless source roster must be capturable");
        assert!(
            rootless_roster
                .compile_rehome_transaction(
                    &rootless,
                    &SurfaceRehomePlacement::new(MISSING_TARGET, None, contained_placements(),),
                )
                .is_none(),
            "a rootless source cannot create an absent target surface"
        );
        assert!(
            rootless_roster
                .compile_rehome_transaction(
                    &rootless,
                    &SurfaceRehomePlacement::new(
                        TARGET,
                        Some(SurfaceMainRehome::Present(RootPresentationTarget::Main {
                            surface: TARGET,
                        })),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "a rootless source cannot fabricate a main command"
        );
    }

    #[test]
    fn pending_recovery_keeps_the_first_complete_roster_authority() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let mut state = SurfaceRecoveryState::default();

        assert!(state.defer(roster.clone()));
        assert!(state.defer(roster.clone()));

        WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
            surface: SOURCE,
            root: ROOT_A,
            floating: FLOATING_A,
            expected_rect: rect(20.0),
            rect: rect(21.0),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture geometry mutation must succeed");
        let changed = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("changed source roster must remain capturable");

        assert!(!state.defer(changed.clone()));
        assert!(!state.update_pending_roster(changed));
        assert_eq!(state.pending(SOURCE), Some(&roster));
        assert_eq!(state.first_workspace_mismatch(&workspace), Some(SOURCE));
        assert_eq!(
            state.first_workspace_mismatch_excluding(&workspace, SOURCE),
            None
        );
        assert_eq!(state.complete_pending(SOURCE), Some(roster));
        assert_eq!(state.pending(SOURCE), None);
    }

    #[test]
    fn copied_recovery_commands_cannot_forge_typed_policy_authority() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let transaction = roster
            .compile_recovery_placement_transaction(
                &workspace,
                &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
            )
            .expect("exact complete roster must compile");

        let mut source_surface = DockSurfaceRule::new();
        source_surface.set_source_enabled(false);
        let mut policy = DockPolicy::new();
        policy.set_surface_rule(SOURCE, source_surface);
        let policy = policy.snapshot(PolicyRevision::new(1));
        let mut ordinary_workspace = workspace.clone();
        let before = ordinary_workspace.clone();
        let error = WorkspaceTransaction::from_commands(transaction.commands().to_vec())
            .apply(&mut ordinary_workspace, &policy)
            .expect_err("copied commands must remain subject to ordinary transaction policy");
        assert!(matches!(
            error,
            crate::error::TransactionError::Command {
                index: 0,
                source: crate::error::CommandError::Policy(
                    PolicyRejection::SurfaceSourceDisabled { surface: SOURCE }
                ),
            }
        ));
        assert_eq!(ordinary_workspace, before);

        transaction
            .apply(&mut workspace)
            .expect("the compiler-minted recovery obligation bypasses later user policy");
        assert!(workspace.surface(SOURCE).is_none());
    }

    #[test]
    fn recovery_compile_requires_exact_optional_main_and_complete_roster() {
        let rooted = workspace_with_source(Some(MAIN));
        let rooted_roster = SurfaceRosterDisposition::capture(&rooted, SOURCE, None)
            .expect("rooted source roster must be capturable");
        assert!(
            rooted_roster
                .compile_recovery_placement_transaction(
                    &rooted,
                    &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
                )
                .is_none(),
            "a rooted source cannot omit its converted main"
        );
        assert!(
            rooted_roster
                .compile_recovery_placement_transaction(
                    &rooted,
                    &SurfaceRosterPlacement::new(
                        TARGET,
                        Some(ContainedRootPlacement::new(
                            CONVERTED_MAIN,
                            ROOT_A,
                            rect(100.0),
                        )),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "the converted-main placement must name the frozen main root"
        );

        let rootless = workspace_with_source(None);
        let rootless_roster = SurfaceRosterDisposition::capture(&rootless, SOURCE, None)
            .expect("rootless source roster must be capturable");
        assert_eq!(rootless_roster.main_root(), None);
        assert!(
            rootless_roster
                .compile_recovery_placement_transaction(
                    &rootless,
                    &SurfaceRosterPlacement::new(
                        TARGET,
                        Some(converted_main()),
                        contained_placements(),
                    ),
                )
                .is_none(),
            "a rootless source cannot acquire a fabricated converted main"
        );

        let placements = contained_placements();
        for incomplete_or_extra in [
            placements[..1].to_vec(),
            [placements.as_slice(), &placements[..1]].concat(),
        ] {
            assert!(
                rootless_roster
                    .compile_recovery_placement_transaction(
                        &rootless,
                        &SurfaceRosterPlacement::new(TARGET, None, incomplete_or_extra),
                    )
                    .is_none(),
                "the placement must cover every frozen contained member exactly once"
            );
        }
        let mut reordered = placements;
        reordered.reverse();
        assert!(
            rootless_roster
                .compile_recovery_placement_transaction(
                    &rootless,
                    &SurfaceRosterPlacement::new(TARGET, None, reordered),
                )
                .is_none(),
            "compile must not sort placements back into identity or geometry order"
        );
    }

    #[test]
    fn recovery_compile_rejects_missing_target_and_converted_main_collision() {
        let workspace = workspace_with_source(Some(MAIN));
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let before = workspace.clone();

        assert!(
            roster
                .compile_recovery_placement_transaction(
                    &workspace,
                    &SurfaceRosterPlacement::new(
                        MISSING_TARGET,
                        Some(converted_main()),
                        contained_placements(),
                    ),
                )
                .is_none()
        );
        assert!(
            roster
                .compile_recovery_placement_transaction(
                    &workspace,
                    &SurfaceRosterPlacement::new(
                        TARGET,
                        Some(ContainedRootPlacement::new(
                            TARGET_EXISTING,
                            MAIN,
                            rect(100.0),
                        )),
                        contained_placements(),
                    ),
                )
                .is_none()
        );
        assert_eq!(workspace, before, "compile rejection must not mutate state");
    }

    #[test]
    fn recovery_compile_rejects_stale_frozen_source_geometry() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");

        WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
            surface: SOURCE,
            root: ROOT_A,
            floating: FLOATING_A,
            expected_rect: rect(20.0),
            rect: rect(21.0),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture geometry mutation must succeed");
        let before_compile = workspace.clone();

        assert!(!roster.matches_workspace(&workspace));
        assert!(
            roster
                .compile_recovery_placement_transaction(
                    &workspace,
                    &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
                )
                .is_none()
        );
        assert_eq!(workspace, before_compile);
    }

    #[test]
    fn compiled_recovery_rejects_a_later_roster_raise_before_staging_commands() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let transaction = roster
            .compile_recovery_placement_transaction(
                &workspace,
                &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
            )
            .expect("initial exact roster must compile");
        let source_node = workspace.root(ROOT_A).expect("root A must exist").node;
        let source = workspace
            .capture_node_source(ROOT_A, source_node)
            .expect("root A source must be capturable");
        let expected_roster = workspace
            .capture_contained_roster(SOURCE)
            .expect("source roster must be capturable");
        WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
            source,
            floating: FLOATING_A,
            expected_roster,
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture raise must succeed");
        let before_recovery = workspace.clone();

        let error = transaction
            .apply(&mut workspace)
            .expect_err("a delayed recovery must reject the changed roster order");
        assert_eq!(
            error,
            TransactionError::Precondition {
                index: 0,
                source: TransactionPreconditionError::StaleSurfaceRoster { surface: SOURCE },
            }
        );
        assert_eq!(workspace, before_recovery);
    }

    #[test]
    fn compiled_recovery_rejects_a_later_rect_update_before_staging_commands() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let transaction = roster
            .compile_recovery_placement_transaction(
                &workspace,
                &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
            )
            .expect("initial exact roster must compile");
        WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
            surface: SOURCE,
            root: ROOT_A,
            floating: FLOATING_A,
            expected_rect: rect(20.0),
            rect: rect(21.0),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture geometry update must succeed");
        let before_recovery = workspace.clone();

        let error = transaction
            .apply(&mut workspace)
            .expect_err("a delayed recovery must reject changed source geometry");
        assert_eq!(
            error,
            TransactionError::Precondition {
                index: 0,
                source: TransactionPreconditionError::StaleSurfaceRoster { surface: SOURCE },
            }
        );
        assert_eq!(workspace, before_recovery);
    }

    #[test]
    fn source_roster_precondition_does_not_reject_an_unrelated_surface_change() {
        let mut workspace = workspace_with_source(None);
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rootless source roster must be capturable");
        let transaction = roster
            .compile_recovery_placement_transaction(
                &workspace,
                &SurfaceRosterPlacement::new(TARGET, None, contained_placements()),
            )
            .expect("initial exact roster must compile");
        WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
            surface: TARGET,
            root: TARGET_EXISTING_ROOT,
            floating: TARGET_EXISTING,
            expected_rect: rect(30.0),
            rect: rect(31.0),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("unrelated target geometry update must succeed");

        transaction
            .apply(&mut workspace)
            .expect("the source-scoped roster precondition must remain satisfied");
        assert_eq!(
            workspace
                .contained_floating(TARGET_EXISTING)
                .expect("existing target floating must survive")
                .rect,
            rect(31.0)
        );
        assert_eq!(
            workspace
                .surface(TARGET)
                .expect("target surface must survive")
                .contained,
            [TARGET_EXISTING, FLOATING_A, FLOATING_B]
        );
    }

    #[test]
    fn stale_final_member_rolls_back_the_complete_recovery_candidate() {
        let mut workspace = workspace_with_source(Some(MAIN));
        let roster = SurfaceRosterDisposition::capture(&workspace, SOURCE, None)
            .expect("rooted source roster must be capturable");
        let transaction = roster
            .compile_recovery_placement_transaction(
                &workspace,
                &SurfaceRosterPlacement::new(
                    TARGET,
                    Some(converted_main()),
                    contained_placements(),
                ),
            )
            .expect("initial exact roster must compile");

        let tabs = workspace
            .root(ROOT_B)
            .expect("floating B root must exist")
            .node;
        let target = workspace
            .capture_tab_target(ROOT_B, tabs)
            .expect("floating B tabs must be capturable");
        WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
            item: ItemId::new(999),
            target: DockTarget::Center(target),
        }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture topology mutation must succeed");
        let before_recovery = workspace.clone();

        let error = transaction
            .apply(&mut workspace)
            .expect_err("the final frozen source must now be stale");
        assert!(matches!(
            error,
            TransactionError::Precondition {
                index: 0,
                source: TransactionPreconditionError::StaleSurfaceRoster { surface: SOURCE },
            }
        ));
        assert_eq!(
            workspace, before_recovery,
            "staging converted main and A must not leak when B rejects"
        );
    }
}
