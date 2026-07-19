//! Typed conversions between one surface's logical space and desktop physical pixels.

use thiserror::Error;

use crate::geometry::{
    GeometryError, LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    ObservedWindow, ObservedWorkArea, WindowInputState, WindowPresentationState,
};
use crate::viewport::{CoordinateGeneration, ViewportBinding, WorkAreaGeneration, WorkAreaToken};
use crate::viewport_route::{ViewportRouteProof, ViewportRouteStamp};

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
    #[error("the route proof is not current")]
    StaleRoute,
    #[error("the route has no authoritative desktop release position: {0:?}")]
    ReleasePositionUnavailable(AuthorityUnavailableReason),
    #[error("work-area token is absent from the current authoritative roster: {0:?}")]
    UnknownWorkArea(WorkAreaToken),
    #[error("tear-off placement geometry is not representable")]
    Geometry,
}

/// Opaque placement proof bound to one exact route and work-area generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TearOffPlacementProof {
    pointer: crate::intent::PointerId,
    route: ViewportRouteStamp,
    work_area: WorkAreaToken,
    work_area_generation: WorkAreaGeneration,
    requested_rect: LogicalRect,
    physical_rect: PhysicalRect,
}

impl TearOffPlacementProof {
    #[must_use]
    pub const fn pointer(self) -> crate::intent::PointerId {
        self.pointer
    }

    #[must_use]
    pub const fn route(self) -> ViewportRouteStamp {
        self.route
    }

    #[must_use]
    pub const fn work_area(self) -> WorkAreaToken {
        self.work_area
    }

    #[must_use]
    pub const fn work_area_generation(self) -> WorkAreaGeneration {
        self.work_area_generation
    }

    #[must_use]
    pub const fn requested_rect(self) -> LogicalRect {
        self.requested_rect
    }

    #[must_use]
    pub const fn physical_rect(self) -> PhysicalRect {
        self.physical_rect
    }
}

/// Solves one explicit release anchor into a clamped desktop placement.
pub(crate) fn solve_tear_off_placement(
    route: &ViewportRouteProof,
    work_area: ObservedWorkArea,
    work_area_generation: WorkAreaGeneration,
    request: TearOffPlacementRequest,
) -> Result<TearOffPlacementProof, TearOffPlacementUnavailable> {
    let release = match route.desktop_position() {
        Authority::Known(position) => *position,
        Authority::Unknown(reason) => {
            return Err(TearOffPlacementUnavailable::ReleasePositionUnavailable(
                *reason,
            ));
        }
    };
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
    Ok(TearOffPlacementProof {
        pointer: route.pointer(),
        route: route.stamp(),
        work_area: work_area.token(),
        work_area_generation,
        requested_rect,
        physical_rect,
    })
}

/// A complete coordinate snapshot acknowledged for one exact native-window binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoordinateSnapshot {
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
    content_bounds: PhysicalRect,
    outer_bounds: Option<PhysicalRect>,
    scale_factor: ScaleFactor,
    input_state: Option<WindowInputState>,
    presentation: Option<WindowPresentationState>,
    focused: Option<bool>,
}

impl CoordinateSnapshot {
    pub(crate) fn from_observation(
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
        observation: &ObservedWindow,
    ) -> Result<Self, CoordinateUnavailable> {
        if binding.token() != observation.token() {
            return Err(CoordinateUnavailable::TokenMismatch);
        }
        let content_bounds =
            require_fact(observation.content_bounds(), CoordinateFact::ContentBounds)?;
        let scale_factor = require_fact(observation.scale_factor(), CoordinateFact::ScaleFactor)?;
        Ok(Self {
            binding,
            coordinate_generation,
            content_bounds,
            outer_bounds: observation.outer_bounds().known().copied(),
            scale_factor,
            input_state: observation.input_state().known().copied(),
            presentation: observation.presentation().known().copied(),
            focused: observation.focused().known().copied(),
        })
    }

    pub(crate) const fn content_bounds(self) -> PhysicalRect {
        self.content_bounds
    }

    pub(crate) const fn outer_bounds(self) -> Option<PhysicalRect> {
        self.outer_bounds
    }

    pub(crate) const fn input_state(self) -> Option<WindowInputState> {
        self.input_state
    }

    pub(crate) const fn presentation(self) -> Option<WindowPresentationState> {
        self.presentation
    }

    pub(crate) const fn focused(self) -> Option<bool> {
        self.focused
    }

    pub(crate) const fn with_generation(mut self, generation: CoordinateGeneration) -> Self {
        self.coordinate_generation = generation;
        self
    }

    pub(crate) fn same_facts(self, other: Self) -> bool {
        self.binding == other.binding
            && self.content_bounds == other.content_bounds
            && self.outer_bounds == other.outer_bounds
            && self.scale_factor == other.scale_factor
            && self.input_state == other.input_state
            && self.presentation == other.presentation
            && self.focused == other.focused
    }

    pub(crate) fn same_placement_facts(self, other: Self) -> bool {
        self.binding == other.binding
            && self.content_bounds.min() == other.content_bounds.min()
            && self.scale_factor == other.scale_factor
    }

    pub(crate) fn desktop_to_surface(
        self,
        point: PhysicalPoint,
    ) -> Result<LogicalPoint, GeometryError> {
        point.to_target_logical(self.content_bounds.min(), self.scale_factor)
    }

    pub(crate) fn desktop_rect_to_surface(
        self,
        rect: PhysicalRect,
    ) -> Result<LogicalRect, GeometryError> {
        rect.to_target_logical(self.content_bounds.min(), self.scale_factor)
    }

    pub(crate) fn placement(
        self,
        logical_rect: LogicalRect,
        work_area: ObservedWorkArea,
        work_area_generation: WorkAreaGeneration,
    ) -> Result<ViewportPlacementProof, CoordinateUnavailable> {
        let physical_min = logical_rect
            .min()
            .to_desktop_physical(self.content_bounds.min(), self.scale_factor)
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

/// Opaque proof that one placement used current binding, scale, origin, and work-area facts.
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
    ScaleFactor,
}

/// Why an exact coordinate conversion or placement proof cannot be produced.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CoordinateUnavailable {
    #[error("window observation token does not match the registered binding")]
    TokenMismatch,
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
    use crate::ids::{SurfaceId, WorkspaceEpoch};
    use crate::platform::{ObservedWindow, ObservedWorkArea, WindowPresentationState};
    use crate::viewport::{WindowIncarnation, WindowToken, WorkAreaGeneration, WorkAreaToken};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
        PhysicalRect::new(x, y, width, height).expect("test physical rect must be valid")
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

    fn snapshot(scale: f64, origin_x: f64) -> CoordinateSnapshot {
        let token = WindowToken::new(7);
        let binding = ViewportBinding::new(
            WorkspaceEpoch::new(2),
            SurfaceId::new(3),
            token,
            WindowIncarnation::new(5),
        );
        let observation = ObservedWindow::new(token)
            .with_content_bounds(Authority::Known(rect(origin_x, -300.0, 1200.0, 900.0)))
            .with_outer_bounds(Authority::Known(rect(
                origin_x - 8.0,
                -330.0,
                1216.0,
                938.0,
            )))
            .with_scale_factor(Authority::Known(
                ScaleFactor::new(scale).expect("test scale must be valid"),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput));
        let observation =
            observation.with_presentation(Authority::Known(WindowPresentationState::Visible));
        CoordinateSnapshot::from_observation(binding, CoordinateGeneration::new(11), &observation)
            .expect("test observation must be route ready")
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
    fn missing_scale_fails_closed() {
        let token = WindowToken::new(1);
        let binding = ViewportBinding::new(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            WindowIncarnation::new(1),
        );
        let observation = ObservedWindow::new(token)
            .with_content_bounds(Authority::Known(rect(0.0, 0.0, 100.0, 100.0)));
        assert!(matches!(
            CoordinateSnapshot::from_observation(
                binding,
                CoordinateGeneration::new(1),
                &observation,
            ),
            Err(CoordinateUnavailable::FactUnavailable {
                fact: CoordinateFact::ScaleFactor,
                ..
            })
        ));
    }
}
