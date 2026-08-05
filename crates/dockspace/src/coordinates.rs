//! Typed conversions between one surface's logical space and desktop physical pixels.

use thiserror::Error;

use crate::geometry::{
    GeometryError, LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{ObservedWorkArea, WindowCoordinateObservation};
use crate::platform_provider::PlatformObservationLease;
use crate::pointer_journal::PointerInputLease;
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, ViewportBinding, WorkAreaGeneration,
    WorkAreaToken,
};

/// Explicit geometry inputs for one native tear-off placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TearOffPlacementRequest {
    cursor_offset: LogicalSize,
    preferred_size: LogicalSize,
    minimum_size: LogicalSize,
    work_area: WorkAreaToken,
}

impl TearOffPlacementRequest {
    #[must_use]
    pub const fn new(
        cursor_offset: LogicalSize,
        preferred_size: LogicalSize,
        minimum_size: LogicalSize,
        work_area: WorkAreaToken,
    ) -> Self {
        Self {
            cursor_offset,
            preferred_size,
            minimum_size,
            work_area,
        }
    }

    #[must_use]
    pub const fn cursor_offset(self) -> LogicalSize {
        self.cursor_offset
    }

    #[must_use]
    pub const fn preferred_size(self) -> LogicalSize {
        self.preferred_size
    }

    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }

    #[must_use]
    pub const fn work_area(self) -> WorkAreaToken {
        self.work_area
    }
}

/// Why exact tear-off placement could not be derived from authoritative facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TearOffPlacementUnavailable {
    #[error("work-area token is absent from the current authoritative roster: {0:?}")]
    UnknownWorkArea(WorkAreaToken),
    #[error("tear-off placement geometry is not representable")]
    Geometry,
}

/// Exact native tear-off placement derived from one desktop pointer provider.
///
/// The proof is minted internally from one validated desktop journal edge and
/// remains bound to that pointer-provider incarnation and the platform
/// provider which supplied the selected work area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TearOffPlacementProof {
    pointer_provider: PointerInputLease,
    platform_provider: PlatformObservationLease,
    pointer: crate::intent::PointerId,
    desktop_position: PhysicalPoint,
    work_area: WorkAreaToken,
    work_area_generation: WorkAreaGeneration,
    requested_rect: LogicalRect,
    physical_rect: PhysicalRect,
}

impl TearOffPlacementProof {
    /// Returns the exact desktop provider incarnation which authorized the placement.
    #[must_use]
    pub const fn pointer_provider(self) -> PointerInputLease {
        self.pointer_provider
    }

    /// Returns the exact platform-provider incarnation which owned the work area.
    #[must_use]
    pub const fn platform_provider(self) -> PlatformObservationLease {
        self.platform_provider
    }

    /// Returns the exact pointer identity which authorized the placement.
    #[must_use]
    pub const fn pointer(self) -> crate::intent::PointerId {
        self.pointer
    }

    /// Returns the exact desktop-physical pointer anchor.
    #[must_use]
    pub const fn desktop_position(self) -> PhysicalPoint {
        self.desktop_position
    }

    /// Returns the explicitly selected work area.
    #[must_use]
    pub const fn work_area(self) -> WorkAreaToken {
        self.work_area
    }

    /// Returns the work-area roster generation used for placement.
    #[must_use]
    pub const fn work_area_generation(self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    /// Returns the unclamped logical request in the selected work-area space.
    #[must_use]
    pub const fn requested_rect(self) -> LogicalRect {
        self.requested_rect
    }

    /// Returns the final clamped desktop-physical placement.
    #[must_use]
    pub const fn physical_rect(self) -> PhysicalRect {
        self.physical_rect
    }
}

/// Solves a journal-authorized native tear-off.
pub(crate) fn solve_tear_off_placement(
    pointer_provider: PointerInputLease,
    platform_provider: PlatformObservationLease,
    pointer: crate::intent::PointerId,
    desktop_position: PhysicalPoint,
    work_area: ObservedWorkArea,
    work_area_generation: WorkAreaGeneration,
    request: TearOffPlacementRequest,
) -> Result<TearOffPlacementProof, TearOffPlacementUnavailable> {
    if request.work_area() != work_area.token() {
        return Err(TearOffPlacementUnavailable::UnknownWorkArea(
            request.work_area(),
        ));
    }
    let (requested_rect, physical_rect) =
        solve_tear_off_geometry(desktop_position, work_area, request)?;
    Ok(TearOffPlacementProof {
        pointer_provider,
        platform_provider,
        pointer,
        desktop_position,
        work_area: work_area.token(),
        work_area_generation,
        requested_rect,
        physical_rect,
    })
}

fn solve_tear_off_geometry(
    release: PhysicalPoint,
    work_area: ObservedWorkArea,
    request: TearOffPlacementRequest,
) -> Result<(LogicalRect, PhysicalRect), TearOffPlacementUnavailable> {
    let scale = work_area.scale_factor();
    let width = request
        .preferred_size
        .width()
        .max(request.minimum_size.width());
    let height = request
        .preferred_size
        .height()
        .max(request.minimum_size.height());
    let physical_size = scale
        .logical_size_to_physical(
            LogicalSize::new(width, height).map_err(|_| TearOffPlacementUnavailable::Geometry)?,
        )
        .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    let physical_offset = scale
        .logical_size_to_physical(request.cursor_offset)
        .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    let requested_min = PhysicalPoint::new(
        release.x() - physical_offset.width(),
        release.y() - physical_offset.height(),
    )
    .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    let requested = PhysicalRect::from_min_size(requested_min, physical_size)
        .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    let physical_rect = clamp_physical_rect(requested, work_area.bounds())
        .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    let requested_rect = requested
        .to_target_logical(work_area.bounds().min(), scale)
        .map_err(|_| TearOffPlacementUnavailable::Geometry)?;
    Ok((requested_rect, physical_rect))
}

/// A complete coordinate snapshot acknowledged for one exact native-window binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoordinateSnapshot {
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
    observation_generation: CoordinateObservationGeneration,
    content_bounds: PhysicalRect,
    outer_bounds: Option<PhysicalRect>,
    native_scale_factor: ScaleFactor,
    presentation_scale_factor: ScaleFactor,
}

/// Exact-binding geometry retained only for destruction recovery.
///
/// Content projection and the outer-window anchor are independent platform
/// facts. A provider may temporarily lose the latter while continuing to
/// report content coordinates. Keeping them separately prevents recovery from
/// either fabricating an outer rectangle from content bounds or discarding the
/// last exact outer placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RecoveryCoordinateSnapshot {
    content: CoordinateSnapshot,
    outer_bounds: Option<PhysicalRect>,
}

impl RecoveryCoordinateSnapshot {
    pub(crate) const fn new(content: CoordinateSnapshot) -> Self {
        Self {
            outer_bounds: content.outer_bounds(),
            content,
        }
    }

    /// Advances current content geometry while retaining the last exact outer
    /// anchor when the new observation reports that fact as unavailable.
    pub(crate) const fn observe(mut self, content: CoordinateSnapshot) -> Self {
        self.content = content;
        if let Some(outer_bounds) = content.outer_bounds() {
            self.outer_bounds = Some(outer_bounds);
        }
        self
    }

    pub(crate) const fn content(self) -> CoordinateSnapshot {
        self.content
    }

    pub(crate) const fn outer_bounds(self) -> Option<PhysicalRect> {
        self.outer_bounds
    }
}

impl CoordinateSnapshot {
    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }

    pub(crate) const fn native_scale_factor(self) -> ScaleFactor {
        self.native_scale_factor
    }

    pub(crate) const fn presentation_scale_factor(self) -> ScaleFactor {
        self.presentation_scale_factor
    }

    pub(crate) const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    pub(crate) const fn observation_generation(self) -> CoordinateObservationGeneration {
        self.observation_generation
    }

    pub(crate) fn from_observation(
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
        observation: WindowCoordinateObservation,
    ) -> Result<Self, CoordinateUnavailable> {
        if binding != observation.binding() {
            return Err(CoordinateUnavailable::BindingMismatch {
                expected: binding,
                observed: observation.binding(),
            });
        }
        let content_bounds =
            require_fact(observation.content_bounds(), CoordinateFact::ContentBounds)?;
        let native_scale_factor = require_fact(
            observation.native_scale_factor(),
            CoordinateFact::NativeScaleFactor,
        )?;
        let presentation_scale_factor = require_fact(
            observation.presentation_scale_factor(),
            CoordinateFact::PresentationScaleFactor,
        )?;
        Ok(Self {
            binding,
            coordinate_generation,
            observation_generation: observation.generation(),
            content_bounds,
            outer_bounds: observation.outer_bounds().known().copied(),
            native_scale_factor,
            presentation_scale_factor,
        })
    }

    pub(crate) const fn content_bounds(self) -> PhysicalRect {
        self.content_bounds
    }

    pub(crate) const fn outer_bounds(self) -> Option<PhysicalRect> {
        self.outer_bounds
    }

    pub(crate) const fn with_generation(mut self, generation: CoordinateGeneration) -> Self {
        self.coordinate_generation = generation;
        self
    }

    pub(crate) fn same_facts(self, other: Self) -> bool {
        self.binding == other.binding
            && self.content_bounds == other.content_bounds
            && self.outer_bounds == other.outer_bounds
            && self.native_scale_factor == other.native_scale_factor
            && self.presentation_scale_factor == other.presentation_scale_factor
    }

    pub(crate) fn same_projection_authority(self, other: Self) -> bool {
        self.coordinate_generation == other.coordinate_generation && self.same_facts(other)
    }

    #[cfg(test)]
    pub(crate) fn same_placement_facts(self, other: Self) -> bool {
        self.binding == other.binding
            && self.content_bounds == other.content_bounds
            && self.native_scale_factor == other.native_scale_factor
    }

    pub(crate) fn desktop_to_surface(
        self,
        point: PhysicalPoint,
    ) -> Result<LogicalPoint, GeometryError> {
        point.to_target_logical(self.content_bounds.min(), self.presentation_scale_factor)
    }

    pub(crate) fn desktop_rect_to_surface(
        self,
        rect: PhysicalRect,
    ) -> Result<LogicalRect, GeometryError> {
        rect.to_target_logical(self.content_bounds.min(), self.presentation_scale_factor)
    }

    pub(crate) fn surface_rect_to_desktop(
        self,
        rect: LogicalRect,
    ) -> Result<PhysicalRect, GeometryError> {
        rect.to_desktop_physical(self.content_bounds.min(), self.presentation_scale_factor)
    }

    pub(crate) fn placement(
        self,
        logical_rect: LogicalRect,
        work_area: ObservedWorkArea,
        work_area_generation: WorkAreaGeneration,
    ) -> Result<ViewportPlacementProof, CoordinateUnavailable> {
        let physical_min = logical_rect
            .min()
            .to_desktop_physical(self.content_bounds.min(), self.native_scale_factor)
            .map_err(CoordinateUnavailable::Geometry)?;
        let physical_size = work_area
            .scale_factor()
            .logical_size_to_physical(logical_rect.size())
            .map_err(CoordinateUnavailable::Geometry)?;
        let requested = PhysicalRect::from_min_size(physical_min, physical_size)
            .map_err(CoordinateUnavailable::Geometry)?;
        let physical_rect = clamp_physical_rect(requested, work_area.bounds())?;
        Ok(ViewportPlacementProof {
            binding: self.binding,
            coordinate_generation: self.coordinate_generation,
            work_area_generation,
            work_area: work_area.token(),
            logical_rect,
            physical_rect,
        })
    }
}

fn require_fact<T: Copy>(
    authority: &Authority<T>,
    fact: CoordinateFact,
) -> Result<T, CoordinateUnavailable> {
    match authority {
        Authority::Known(value) => Ok(*value),
        Authority::Unknown(reason) => Err(CoordinateUnavailable::FactUnavailable {
            fact,
            reason: *reason,
        }),
    }
}

fn clamp_physical_rect(
    requested: PhysicalRect,
    work_area: PhysicalRect,
) -> Result<PhysicalRect, CoordinateUnavailable> {
    let width = requested.width().min(work_area.width());
    let height = requested.height().min(work_area.height());
    let maximum_x = work_area.max().x() - width;
    let maximum_y = work_area.max().y() - height;
    let x = requested.x().clamp(work_area.x(), maximum_x);
    let y = requested.y().clamp(work_area.y(), maximum_y);
    PhysicalRect::new(x, y, width, height).map_err(CoordinateUnavailable::Geometry)
}

/// Opaque proof that one placement used current binding, content bounds, scale, and work area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportPlacementProof {
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
    work_area_generation: WorkAreaGeneration,
    work_area: WorkAreaToken,
    logical_rect: LogicalRect,
    physical_rect: PhysicalRect,
}

impl ViewportPlacementProof {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn coordinate_generation(&self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    #[must_use]
    pub const fn work_area_generation(&self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    #[must_use]
    pub const fn work_area(&self) -> WorkAreaToken {
        self.work_area
    }

    #[must_use]
    pub const fn logical_rect(&self) -> LogicalRect {
        self.logical_rect
    }

    #[must_use]
    pub const fn physical_rect(&self) -> PhysicalRect {
        self.physical_rect
    }

    pub(crate) fn is_current(
        &self,
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
        work_area_generation: WorkAreaGeneration,
    ) -> bool {
        self.binding == binding
            && self.coordinate_generation == coordinate_generation
            && self.work_area_generation == work_area_generation
    }
}

/// Coordinate fact required for an exact conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoordinateFact {
    ContentBounds,
    NativeScaleFactor,
    PresentationScaleFactor,
}

/// Why an exact coordinate conversion or placement proof cannot be produced.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CoordinateUnavailable {
    #[error("window observation binding does not match the registered binding")]
    BindingMismatch {
        expected: ViewportBinding,
        observed: ViewportBinding,
    },
    #[error("coordinate fact {fact:?} is unavailable: {reason:?}")]
    FactUnavailable {
        fact: CoordinateFact,
        reason: AuthorityUnavailableReason,
    },
    #[error("work-area token is absent from the current authoritative roster: {token:?}")]
    UnknownWorkArea { token: WorkAreaToken },
    #[error(transparent)]
    Geometry(GeometryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::platform::{ObservedWorkArea, WindowCoordinateObservation};
    use crate::viewport::{
        CoordinateObservationGeneration, WindowIncarnation, WindowToken, WorkAreaGeneration,
        WorkAreaToken,
    };

    fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
        PhysicalRect::new(x, y, width, height).expect("test physical rect must be valid")
    }

    fn authority_domain() -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(1)
    }

    fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect::new(x, y, width, height).expect("test logical rect must be valid")
    }

    fn work_area(token: u64, bounds: PhysicalRect, scale: f64) -> ObservedWorkArea {
        ObservedWorkArea::new(
            WorkAreaToken::new(token),
            bounds,
            ScaleFactor::new(scale).expect("test scale must be valid"),
        )
    }

    fn snapshot_with_scales(
        native_scale: f64,
        presentation_scale: f64,
        origin_x: f64,
    ) -> CoordinateSnapshot {
        let token = WindowToken::new(7);
        let binding = ViewportBinding::new(
            authority_domain(),
            WorkspaceEpoch::new(2),
            SurfaceId::new(3),
            token,
            WindowIncarnation::new(5),
        );
        let native_scale = ScaleFactor::new(native_scale).expect("test scale must be valid");
        let presentation_scale =
            ScaleFactor::new(presentation_scale).expect("test scale must be valid");
        let observation = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(7),
            Authority::Known(rect(origin_x, -300.0, 1200.0, 900.0)),
            Authority::Known(rect(origin_x - 8.0, -330.0, 1216.0, 938.0)),
            Authority::Known(native_scale),
            Authority::Known(presentation_scale),
        );
        CoordinateSnapshot::from_observation(binding, CoordinateGeneration::new(11), observation)
            .expect("test observation must be route ready")
    }

    fn snapshot(scale: f64, origin_x: f64) -> CoordinateSnapshot {
        snapshot_with_scales(scale, scale, origin_x)
    }

    #[test]
    fn mixed_dpi_conversion_uses_the_target_scale_exactly_once() {
        let desktop = PhysicalPoint::new(400.0, 0.0).expect("test point must be valid");
        let scales = [(1.0, 400.0), (1.5, 266.666_666_666_666_7), (2.0, 200.0)];

        for (scale, expected_x) in scales {
            let logical = snapshot(scale, 0.0)
                .desktop_to_surface(desktop)
                .expect("conversion must succeed");
            assert!((logical.x() - expected_x).abs() < 1.0e-9);
            assert!((logical.y() - (300.0 / scale)).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn surface_conversion_uses_presentation_scale_without_changing_native_placement_scale() {
        let snapshot = snapshot_with_scales(2.0, 2.5, 100.0);
        let logical = snapshot
            .desktop_to_surface(PhysicalPoint::new(350.0, -50.0).expect("test point must be valid"))
            .expect("presentation conversion must succeed");
        assert_eq!(logical, LogicalPoint::new(100.0, 100.0).unwrap());

        let proof = snapshot
            .placement(
                logical_rect(100.0, 100.0, 200.0, 120.0),
                work_area(4, rect(0.0, -400.0, 2_000.0, 1_400.0), 2.0),
                WorkAreaGeneration::new(8),
            )
            .expect("native placement must remain available");
        assert_eq!(proof.physical_rect().min().x(), 300.0);
    }

    #[test]
    fn conversion_supports_negative_desktop_origins() {
        let logical = snapshot(2.0, -1600.0)
            .desktop_to_surface(
                PhysicalPoint::new(-1200.0, -100.0).expect("test point must be valid"),
            )
            .expect("conversion must succeed");
        assert!((logical.x() - 200.0).abs() < f64::EPSILON);
        assert!((logical.y() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn placement_is_scaled_once_and_clamped_to_the_acknowledged_work_area() {
        let proof = snapshot(2.0, 0.0)
            .placement(
                logical_rect(900.0, 500.0, 600.0, 400.0),
                work_area(4, rect(-1920.0, -400.0, 3840.0, 1400.0), 2.0),
                WorkAreaGeneration::new(8),
            )
            .expect("placement must be available");
        assert_eq!(proof.physical_rect(), rect(720.0, 200.0, 1200.0, 800.0));
    }

    #[test]
    fn proof_is_bound_to_coordinate_generation_and_incarnation() {
        let snapshot = snapshot(1.0, 0.0);
        let proof = snapshot
            .placement(
                logical_rect(10.0, 20.0, 100.0, 80.0),
                work_area(4, rect(-1920.0, -400.0, 3840.0, 1400.0), 1.0),
                WorkAreaGeneration::new(8),
            )
            .expect("placement must be available");
        assert!(proof.is_current(
            snapshot.binding,
            snapshot.coordinate_generation,
            WorkAreaGeneration::new(8),
        ));
        assert!(!proof.is_current(
            snapshot.binding,
            CoordinateGeneration::new(12),
            WorkAreaGeneration::new(8),
        ));
        assert!(!proof.is_current(
            snapshot.binding,
            snapshot.coordinate_generation,
            WorkAreaGeneration::new(9),
        ));
        assert!(!proof.is_current(
            ViewportBinding::new(
                snapshot.binding.authority_domain(),
                snapshot.binding.epoch(),
                snapshot.binding.surface(),
                snapshot.binding.token(),
                WindowIncarnation::new(6),
            ),
            snapshot.coordinate_generation,
            WorkAreaGeneration::new(8),
        ));
    }

    #[test]
    fn placement_facts_reject_a_content_resize_at_the_same_origin_and_scale() {
        let previous = snapshot(1.0, 0.0);
        let resized = CoordinateSnapshot {
            content_bounds: rect(0.0, -300.0, 1400.0, 700.0),
            ..previous
        };

        assert!(!previous.same_placement_facts(resized));
    }

    #[test]
    fn placement_facts_accept_identical_geometry_with_a_different_generation() {
        let previous = snapshot(1.0, 0.0);
        let refreshed = previous.with_generation(CoordinateGeneration::new(12));

        assert!(previous.same_placement_facts(refreshed));
    }

    #[test]
    fn projection_authority_ignores_observation_freshness_but_not_authority_generation() {
        let previous = snapshot(1.0, 0.0);
        let refreshed = CoordinateSnapshot {
            observation_generation: CoordinateObservationGeneration::new(8),
            ..previous
        };
        assert!(previous.same_projection_authority(refreshed));

        let replaced = refreshed.with_generation(CoordinateGeneration::new(12));
        assert!(!previous.same_projection_authority(replaced));

        let resized = CoordinateSnapshot {
            content_bounds: rect(0.0, -300.0, 1400.0, 700.0),
            ..refreshed
        };
        assert!(!previous.same_projection_authority(resized));
    }

    #[test]
    fn outer_bounds_are_not_placement_facts() {
        let previous = snapshot(1.0, 0.0);
        let decorated = CoordinateSnapshot {
            outer_bounds: Some(rect(-20.0, -350.0, 1240.0, 970.0)),
            ..previous
        };

        assert!(previous.same_placement_facts(decorated));
    }

    #[test]
    fn missing_scale_fails_closed() {
        let token = WindowToken::new(1);
        let binding = ViewportBinding::new(
            authority_domain(),
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            WindowIncarnation::new(1),
        );
        let observation = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(rect(0.0, 0.0, 100.0, 100.0)),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );
        assert!(matches!(
            CoordinateSnapshot::from_observation(
                binding,
                CoordinateGeneration::new(1),
                observation,
            ),
            Err(CoordinateUnavailable::FactUnavailable {
                fact: CoordinateFact::NativeScaleFactor,
                ..
            })
        ));
    }

    #[test]
    fn missing_presentation_scale_fails_closed_even_with_native_geometry() {
        let binding = ViewportBinding::new(
            authority_domain(),
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let observation = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(rect(0.0, 0.0, 100.0, 100.0)),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(2.0).expect("test scale must be valid")),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );
        assert!(matches!(
            CoordinateSnapshot::from_observation(
                binding,
                CoordinateGeneration::new(1),
                observation,
            ),
            Err(CoordinateUnavailable::FactUnavailable {
                fact: CoordinateFact::PresentationScaleFactor,
                ..
            })
        ));
    }

    #[test]
    fn recycled_token_observation_cannot_authorize_a_new_incarnation() {
        let token = WindowToken::new(9);
        let current = ViewportBinding::new(
            authority_domain(),
            WorkspaceEpoch::new(3),
            SurfaceId::new(4),
            token,
            WindowIncarnation::new(2),
        );
        let delayed = ViewportBinding::new(
            authority_domain(),
            WorkspaceEpoch::new(2),
            SurfaceId::new(4),
            token,
            WindowIncarnation::new(1),
        );
        let observation = WindowCoordinateObservation::new(
            delayed,
            CoordinateObservationGeneration::new(1),
            Authority::Known(rect(20.0, 30.0, 400.0, 300.0)),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        );

        assert_eq!(
            CoordinateSnapshot::from_observation(
                current,
                CoordinateGeneration::new(1),
                observation,
            ),
            Err(CoordinateUnavailable::BindingMismatch {
                expected: current,
                observed: delayed,
            })
        );
    }

    #[test]
    fn foreign_authority_domain_cannot_authorize_coordinates() {
        let epoch = WorkspaceEpoch::new(3);
        let surface = SurfaceId::new(4);
        let token = WindowToken::new(9);
        let incarnation = WindowIncarnation::new(2);
        let current = ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            epoch,
            surface,
            token,
            incarnation,
        );
        let foreign = ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(2),
            epoch,
            surface,
            token,
            incarnation,
        );
        let observation = WindowCoordinateObservation::new(
            foreign,
            CoordinateObservationGeneration::new(1),
            Authority::Known(rect(20.0, 30.0, 400.0, 300.0)),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        );

        assert_eq!(
            CoordinateSnapshot::from_observation(
                current,
                CoordinateGeneration::new(1),
                observation,
            ),
            Err(CoordinateUnavailable::BindingMismatch {
                expected: current,
                observed: foreign,
            })
        );
    }
}
