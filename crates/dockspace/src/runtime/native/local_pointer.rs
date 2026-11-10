//! Event-time native client coordinates authorized by committed local geometry.

use thiserror::Error;

use super::NativeSurfaceBinding;
use crate::backend_ingress::BackendIngressLease;
use crate::coordinates::{CoordinateSnapshot, SurfaceClientGeometry};
use crate::geometry::{GeometryError, LogicalPoint, PhysicalPoint, PhysicalSize, ScaleFactor};
use crate::viewport::CoordinateGeneration;

/// Native client geometry observed at one callback boundary.
///
/// These are ordinary host facts. Constructing them grants no input authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalGeometry {
    content_size: PhysicalSize,
    native_scale_factor: ScaleFactor,
    presentation_scale_factor: ScaleFactor,
}

impl NativeLocalGeometry {
    /// Creates the complete local geometry observed by the host callback.
    #[must_use]
    pub const fn new(
        content_size: PhysicalSize,
        native_scale_factor: ScaleFactor,
        presentation_scale_factor: ScaleFactor,
    ) -> Self {
        Self {
            content_size,
            native_scale_factor,
            presentation_scale_factor,
        }
    }

    /// Returns the physical client size.
    #[must_use]
    pub const fn content_size(self) -> PhysicalSize {
        self.content_size
    }

    /// Returns the native scale under which physical client points are generated.
    #[must_use]
    pub const fn native_scale_factor(self) -> ScaleFactor {
        self.native_scale_factor
    }

    /// Returns the physical-pixels-per-logical-unit presentation scale.
    #[must_use]
    pub const fn presentation_scale_factor(self) -> ScaleFactor {
        self.presentation_scale_factor
    }
}

/// Physical client coordinates and the geometry used to generate one event.
///
/// Hosts must freeze the size and native scale where the physical point is
/// generated. A later callback must not substitute its current geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalPointerFact {
    client_position: PhysicalPoint,
    content_size: PhysicalSize,
    native_scale_factor: ScaleFactor,
}

impl NativeLocalPointerFact {
    /// Creates one event-time fact without granting coordinate authority.
    #[must_use]
    pub const fn new(
        client_position: PhysicalPoint,
        content_size: PhysicalSize,
        native_scale_factor: ScaleFactor,
    ) -> Self {
        Self {
            client_position,
            content_size,
            native_scale_factor,
        }
    }

    /// Returns the event's physical position relative to the client area.
    #[must_use]
    pub const fn client_position(self) -> PhysicalPoint {
        self.client_position
    }

    /// Returns the physical client size frozen when the point was generated.
    #[must_use]
    pub const fn content_size(self) -> PhysicalSize {
        self.content_size
    }

    /// Returns the native scale frozen when the point was generated.
    #[must_use]
    pub const fn native_scale_factor(self) -> ScaleFactor {
        self.native_scale_factor
    }
}

/// Opaque permission to capture native client points against committed geometry.
///
/// Obtain this value from
/// [`DockspaceSession::native_local_coordinates`](crate::runtime::DockspaceSession::native_local_coordinates).
/// It binds one joined backend, native-window incarnation, and core coordinate
/// generation. It proves neither desktop position nor global native hover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalCoordinates {
    backend: BackendIngressLease,
    binding: NativeSurfaceBinding,
    generation: CoordinateGeneration,
    content_size: PhysicalSize,
    native_scale_factor: ScaleFactor,
    surface_client: SurfaceClientGeometry,
}

impl NativeLocalCoordinates {
    pub(super) const fn new(
        backend: BackendIngressLease,
        binding: NativeSurfaceBinding,
        snapshot: CoordinateSnapshot,
    ) -> Self {
        Self {
            backend,
            binding,
            generation: snapshot.coordinate_generation(),
            content_size: snapshot.content_size(),
            native_scale_factor: snapshot.native_scale_factor(),
            surface_client: snapshot.surface_client_geometry(),
        }
    }

    /// Returns the exact native-window incarnation authorized by this capability.
    #[must_use]
    pub const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    /// Returns the committed local geometry required at capture time.
    #[must_use]
    pub const fn geometry(self) -> NativeLocalGeometry {
        NativeLocalGeometry::new(
            self.content_size,
            self.native_scale_factor,
            self.surface_client.scale_factor(),
        )
    }

    /// Captures one event using its original physical point and frozen geometry.
    ///
    /// Event and callback size/native scale must match exactly, and the callback
    /// must match this capability's complete local geometry. Conversion uses the
    /// frozen surface-to-client mapping once. Finite points outside its clip or
    /// the client area are preserved without clamping and grant no desktop
    /// outside-all authority.
    /// The reducer validates current coordinate authority again at its ordinal.
    ///
    /// # Errors
    ///
    /// Returns an error for a different binding, mismatched event or callback
    /// geometry, or an overflowing logical coordinate conversion.
    pub fn capture(
        self,
        binding: NativeSurfaceBinding,
        event: NativeLocalPointerFact,
        callback: NativeLocalGeometry,
    ) -> Result<NativeLocalPointerSample, NativeLocalCoordinateError> {
        if binding != self.binding {
            return Err(NativeLocalCoordinateError::BindingMismatch {
                expected: self.binding,
                observed: binding,
            });
        }
        if event.content_size != callback.content_size
            || event.native_scale_factor != callback.native_scale_factor
        {
            return Err(NativeLocalCoordinateError::EventGeometryMismatch);
        }
        if callback != self.geometry() {
            return Err(NativeLocalCoordinateError::CallbackGeometryMismatch);
        }
        let surface_position = self
            .surface_client
            .client_to_surface(event.client_position)?;
        Ok(NativeLocalPointerSample {
            coordinates: self,
            client_position: event.client_position,
            surface_position,
        })
    }

    pub(super) const fn backend(self) -> BackendIngressLease {
        self.backend
    }

    pub(super) const fn coordinate_generation(self) -> CoordinateGeneration {
        self.generation
    }
}

/// A native client point captured once under an opaque coordinate capability.
///
/// The original physical point and logical conversion travel together. Delayed
/// translation must retain this sample instead of applying a newer scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalPointerSample {
    coordinates: NativeLocalCoordinates,
    client_position: PhysicalPoint,
    surface_position: LogicalPoint,
}

impl NativeLocalPointerSample {
    /// Returns whether two captured samples use the same exact coordinate authority.
    ///
    /// Positions are deliberately excluded. Hosts may use this comparison to
    /// compact idle motion without joining different bindings or generations.
    #[must_use]
    pub fn same_coordinates(self, other: Self) -> bool {
        self.coordinates == other.coordinates
    }

    /// Returns the original event-time client point in physical pixels.
    #[must_use]
    pub const fn client_position(self) -> PhysicalPoint {
        self.client_position
    }

    /// Returns the logical point computed using the captured surface-to-client mapping.
    #[must_use]
    pub const fn surface_position(self) -> LogicalPoint {
        self.surface_position
    }

    pub(super) const fn coordinates(self) -> NativeLocalCoordinates {
        self.coordinates
    }
}

/// Event-time local coordinate authority for one native pointer edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeLocalPointerLocation {
    /// An immutable sample captured against exact committed local geometry.
    Exact(NativeLocalPointerSample),
    /// Local coordinates could not be proven; delivery and capture remain independent.
    Unknown,
}

/// Why an event could not be captured under a native local coordinate capability.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NativeLocalCoordinateError {
    /// The supplied native-window incarnation differs from the capability.
    #[error("native local pointer binding differs from its coordinate capability")]
    BindingMismatch {
        /// Exact native-window incarnation authorized by the capability.
        expected: NativeSurfaceBinding,
        /// Exact native-window incarnation supplied by the callback.
        observed: NativeSurfaceBinding,
    },
    /// The event's size or native scale differs from the callback's facts.
    #[error("native local pointer event geometry differs from callback geometry")]
    EventGeometryMismatch,
    /// The callback's local geometry differs from the committed capability.
    #[error("native local pointer callback geometry differs from committed geometry")]
    CallbackGeometryMismatch,
    /// Mapping a finite physical point overflowed the logical coordinate range.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PhysicalRect;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::intent::{Authority, AuthorityUnavailableReason};
    use crate::platform::WindowCoordinateObservation;
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::pointer_journal::{PointerInputLease, PointerProviderScope};
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{
        CoordinateGeneration, CoordinateObservationGeneration, ViewportBinding, WindowIncarnation,
        WindowToken,
    };

    fn size(width: f64, height: f64) -> PhysicalSize {
        PhysicalSize::new(width, height).expect("test client size is valid")
    }

    fn scale(value: f64) -> ScaleFactor {
        ScaleFactor::new(value).expect("test scale is valid")
    }

    fn point(x: f64, y: f64) -> PhysicalPoint {
        PhysicalPoint::new(x, y).expect("test client point is finite")
    }

    fn coordinates(native_scale: f64, presentation_scale: f64) -> NativeLocalCoordinates {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let platform = PlatformObservationAuthority::new(domain)
            .create()
            .expect("test platform provider enrolls");
        let host = PresentationLedger::new(domain)
            .create_host()
            .expect("test presentation host enrolls");
        let pointer = PointerInputLease::new(domain, 1, PointerProviderScope::NativeJoined(host));
        let backend = BackendIngressLease::new(platform, pointer, host)
            .expect("test joined backend has one authority domain and host");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let unknown = Authority::Unknown(AuthorityUnavailableReason::NotReported);
        let observation = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(3),
            unknown,
            unknown,
            Authority::Known(scale(native_scale)),
            Authority::Known(scale(presentation_scale)),
        )
        .with_content_size(Authority::Known(size(800.0, 600.0)));
        let snapshot = CoordinateSnapshot::from_observation(
            binding,
            CoordinateGeneration::new(7),
            observation,
        )
        .expect("test local geometry needs no desktop origin");
        NativeLocalCoordinates::new(
            backend,
            NativeSurfaceBinding::from_binding(platform, binding),
            snapshot,
        )
    }

    fn event(geometry: NativeLocalGeometry, position: PhysicalPoint) -> NativeLocalPointerFact {
        NativeLocalPointerFact::new(
            position,
            geometry.content_size(),
            geometry.native_scale_factor(),
        )
    }

    #[test]
    fn event_geometry_mismatch_matrix_is_rejected() {
        let coordinates = coordinates(2.0, 3.0);
        let callback = coordinates.geometry();
        for event in [
            NativeLocalPointerFact::new(point(12.0, 6.0), size(801.0, 600.0), scale(2.0)),
            NativeLocalPointerFact::new(point(12.0, 6.0), size(800.0, 601.0), scale(2.0)),
            NativeLocalPointerFact::new(point(12.0, 6.0), size(800.0, 600.0), scale(1.0)),
        ] {
            assert_eq!(
                coordinates.capture(coordinates.binding(), event, callback),
                Err(NativeLocalCoordinateError::EventGeometryMismatch),
            );
        }
    }

    #[test]
    fn callback_geometry_mismatch_matrix_is_rejected_even_when_event_matches() {
        let coordinates = coordinates(2.0, 3.0);
        for callback in [
            NativeLocalGeometry::new(size(801.0, 600.0), scale(2.0), scale(3.0)),
            NativeLocalGeometry::new(size(800.0, 601.0), scale(2.0), scale(3.0)),
            NativeLocalGeometry::new(size(800.0, 600.0), scale(1.0), scale(3.0)),
            NativeLocalGeometry::new(size(800.0, 600.0), scale(2.0), scale(4.0)),
        ] {
            assert_eq!(
                coordinates.capture(
                    coordinates.binding(),
                    event(callback, point(12.0, 6.0)),
                    callback,
                ),
                Err(NativeLocalCoordinateError::CallbackGeometryMismatch),
            );
        }
    }

    #[test]
    fn queued_old_scale_event_cannot_be_reinterpreted_by_a_new_callback_or_capability() {
        let old = coordinates(1.0, 1.0);
        let current = coordinates(2.0, 2.0);
        let old_event = event(old.geometry(), point(120.0, 60.0));
        assert_eq!(
            current.capture(current.binding(), old_event, current.geometry()),
            Err(NativeLocalCoordinateError::EventGeometryMismatch),
        );
        assert_eq!(
            old.capture(old.binding(), old_event, current.geometry()),
            Err(NativeLocalCoordinateError::EventGeometryMismatch),
        );
    }

    #[test]
    fn finite_outside_client_points_are_preserved_without_clamping() {
        let coordinates = coordinates(2.0, 2.0);
        let position = point(-32.0, 900.0);
        let sample = coordinates
            .capture(
                coordinates.binding(),
                event(coordinates.geometry(), position),
                coordinates.geometry(),
            )
            .expect("finite points outside the client area remain valid samples");
        assert_eq!(sample.client_position(), position);
        assert_eq!(
            sample.surface_position(),
            LogicalPoint::new(-16.0, 450.0).unwrap()
        );
        assert_eq!(sample.coordinates(), coordinates);
        assert_eq!(sample.coordinates().geometry(), coordinates.geometry());
    }

    #[test]
    fn capture_preserves_source_motion_outside_an_offset_surface_clip() {
        let coordinates = NativeLocalCoordinates {
            surface_client: SurfaceClientGeometry::new(
                point(120.0, 60.0),
                scale(3.0),
                PhysicalRect::new(120.0, 60.0, 300.0, 240.0).unwrap(),
            ),
            ..coordinates(2.0, 3.0)
        };
        let position = point(90.0, 420.0);
        assert!(!coordinates.surface_client.clip().contains(position));
        let sample = coordinates
            .capture(
                coordinates.binding(),
                event(coordinates.geometry(), position),
                coordinates.geometry(),
            )
            .expect("capture preserves finite source motion outside the hover clip");
        assert_eq!(sample.client_position(), position);
        assert_eq!(
            sample.surface_position(),
            LogicalPoint::new(-10.0, 120.0).unwrap()
        );
        assert_eq!(sample.coordinates(), coordinates);
        assert_eq!(sample.coordinates().geometry(), coordinates.geometry());
    }

    #[test]
    fn sample_compaction_does_not_cross_a_changed_client_mapping() {
        let previous = coordinates(2.0, 3.0);
        let moved = NativeLocalCoordinates {
            surface_client: SurfaceClientGeometry::new(
                point(1.0, 0.0),
                scale(3.0),
                previous.surface_client.clip(),
            ),
            ..previous
        };
        let clipped = NativeLocalCoordinates {
            surface_client: SurfaceClientGeometry::new(
                point(0.0, 0.0),
                scale(3.0),
                PhysicalRect::new(20.0, 0.0, 780.0, 600.0).unwrap(),
            ),
            ..previous
        };
        let capture = |coordinates: NativeLocalCoordinates, position| {
            coordinates
                .capture(
                    coordinates.binding(),
                    event(coordinates.geometry(), position),
                    coordinates.geometry(),
                )
                .unwrap()
        };
        let old_sample = capture(previous, point(120.0, 60.0));
        assert!(old_sample.same_coordinates(capture(previous, point(240.0, 60.0))));
        assert!(!old_sample.same_coordinates(capture(moved, point(120.0, 60.0))));
        assert!(!old_sample.same_coordinates(capture(clipped, point(120.0, 60.0))));
    }

    #[test]
    fn physical_point_is_divided_only_by_the_frozen_presentation_scale() {
        let coordinates = coordinates(2.0, 3.0);
        let sample = coordinates
            .capture(
                coordinates.binding(),
                event(coordinates.geometry(), point(120.0, 60.0)),
                coordinates.geometry(),
            )
            .expect("different native and presentation scales are valid");
        assert_eq!(
            sample.surface_position(),
            LogicalPoint::new(40.0, 20.0).unwrap()
        );
        assert_eq!(sample.client_position(), point(120.0, 60.0));
    }

    #[test]
    fn finite_physical_point_whose_logical_conversion_overflows_is_rejected() {
        let coordinates = coordinates(1.0, f64::MIN_POSITIVE);
        let error = coordinates
            .capture(
                coordinates.binding(),
                event(coordinates.geometry(), point(f64::MAX, 0.0)),
                coordinates.geometry(),
            )
            .expect_err("a finite physical point cannot produce an infinite logical point");
        assert!(matches!(
            error,
            NativeLocalCoordinateError::Geometry(GeometryError::NonFinite { .. }),
        ));
    }

    #[test]
    fn matching_geometry_cannot_rebind_a_capability_to_another_native_incarnation() {
        let coordinates = coordinates(2.0, 3.0);
        let binding = coordinates.binding();
        let foreign = NativeSurfaceBinding::from_binding(
            binding.provider,
            ViewportBinding::new(
                binding.binding.authority_domain(),
                binding.binding.epoch(),
                binding.binding.surface(),
                binding.binding.token(),
                WindowIncarnation::new(binding.binding.incarnation().get() + 1),
            ),
        );
        assert_eq!(
            coordinates.capture(
                foreign,
                event(coordinates.geometry(), point(12.0, 6.0)),
                coordinates.geometry(),
            ),
            Err(NativeLocalCoordinateError::BindingMismatch {
                expected: binding,
                observed: foreign,
            }),
        );
    }
}
