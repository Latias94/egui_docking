//! Window hit facts are selected independently from the retained source placement.

use crate::engine::{CoreHostFrame, DockEngine};
use crate::geometry::PhysicalPoint;
use crate::pointer_journal::NativeSurfacePointerRoute;
use crate::pointer_receiver::{
    NativeWindowHitReceipt, PointerReceiverCandidate, PointerReceiverProbeRequest,
};
use crate::runtime::{PointerEventMetadata, SurfacePresentationArtifact};

use super::NativeWindowBinding;

/// The independent question which requires a framework window hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeWindowHitPurpose {
    /// Select the placement receiving a semantic delivery lane.
    Delivery,
    /// Select the independently exposed drop target placement.
    Hover,
}

/// One physical point in an exact native window at the current input ordinal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeWindowHitQuery {
    window: NativeWindowBinding,
    point: PhysicalPoint,
    purpose: NativeWindowHitPurpose,
    metadata: PointerEventMetadata,
}

impl NativeWindowHitQuery {
    /// Returns the exact physical window whose framework hit graph must answer.
    #[must_use]
    pub const fn window(self) -> NativeWindowBinding {
        self.window
    }
    /// Returns the original window-client point in physical pixels.
    #[must_use]
    pub const fn point(self) -> PhysicalPoint {
        self.point
    }
    /// Returns whether selection serves delivery or independent hover.
    #[must_use]
    pub const fn purpose(self) -> NativeWindowHitPurpose {
        self.purpose
    }
    /// Returns the immutable event-time device and observation time.
    #[must_use]
    pub const fn metadata(self) -> PointerEventMetadata {
        self.metadata
    }

    pub(super) fn from_candidate(
        frame: &CoreHostFrame,
        candidate: &PointerReceiverCandidate,
    ) -> Option<Self> {
        let probe = candidate.native_window()?;
        let provider = frame.backend_ingress_provider()?.platform_provider();
        Some(Self {
            window: NativeWindowBinding::from_binding(provider, probe.window()),
            point: probe.client_position(),
            purpose: if candidate.probes() == PointerReceiverProbeRequest::HoverHit {
                NativeWindowHitPurpose::Hover
            } else {
                NativeWindowHitPurpose::Delivery
            },
            metadata: candidate.metadata(),
        })
    }
}

/// Framework visibility at a physical point, independent of docking geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeWindowHit {
    /// The framework proves this exact presented placement is exposed.
    Surface(SurfacePresentationArtifact),
    /// The point reaches window background, with no placement underneath.
    WindowBackground,
    /// A framework overlay or undeclared receiver blocks the point.
    Blocked,
    /// The framework cannot prove the current exposed placement.
    Unknown,
}

pub(super) fn validate_window_hit(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    hit: NativeWindowHit,
) -> NativeWindowHitReceipt {
    let Some(probe) = candidate.native_window() else {
        return NativeWindowHitReceipt::Unknown;
    };
    let surface = match hit {
        NativeWindowHit::Surface(surface) => surface,
        NativeWindowHit::WindowBackground => return NativeWindowHitReceipt::WindowBackground,
        NativeWindowHit::Blocked => return NativeWindowHitReceipt::Blocked,
        NativeWindowHit::Unknown => return NativeWindowHitReceipt::Unknown,
    };
    let view = frame.view();
    let Some(projection) = view.interaction_projection(surface.output().surface()) else {
        return NativeWindowHitReceipt::Unknown;
    };
    if !crate::runtime::interaction::PresentedDockspaceSurface::from_projection(projection)
        .matches_presentation_artifact(surface)
    {
        return NativeWindowHitReceipt::Unknown;
    }
    let authority = projection.authority();
    let Some(binding) = authority
        .binding()
        .filter(|binding| binding.window() == probe.window())
    else {
        return NativeWindowHitReceipt::Unknown;
    };
    let Some(record) = view
        .viewport()
        .registry()
        .placement_record(binding)
        .filter(|record| record.is_routeable())
    else {
        return NativeWindowHitReceipt::Unknown;
    };
    let Some(coordinates) = record.coordinates() else {
        return NativeWindowHitReceipt::Unknown;
    };
    if coordinates.window_coordinates().coordinate_generation() != probe.coordinate_generation()
        || authority.coordinate_generation() != coordinates.coordinate_generation()
        || !coordinates
            .surface_client_geometry()
            .clip()
            .contains(probe.client_position())
    {
        return NativeWindowHitReceipt::Unknown;
    }
    let Ok(point) = coordinates.client_to_surface(probe.client_position()) else {
        return NativeWindowHitReceipt::Unknown;
    };
    if !DockEngine::native_surface_hit_contains(projection, point) {
        return NativeWindowHitReceipt::Unknown;
    }
    NativeWindowHitReceipt::Surface {
        route: NativeSurfacePointerRoute::new(
            binding,
            coordinates.coordinate_generation(),
            probe.client_position(),
            point,
        ),
        output: projection.output_ticket(),
        authority,
    }
}
