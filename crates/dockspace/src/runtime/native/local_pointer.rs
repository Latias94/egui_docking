//! Event-time native client coordinates authorized by committed local geometry.

use thiserror::Error;

use super::NativeWindowBinding;
use crate::backend_ingress::BackendIngressLease;
use crate::coordinates::WindowCoordinateSnapshot;
use crate::geometry::{GeometryError, PhysicalPoint, PhysicalSize, ScaleFactor};
use crate::viewport::CoordinateGeneration;

/// Native client geometry observed at one callback boundary.
///
/// These are ordinary host facts. Constructing them grants no input authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalGeometry {
    content_size: PhysicalSize,
    native_scale_factor: ScaleFactor,
}

impl NativeLocalGeometry {
    /// Creates the complete local geometry observed by the host callback.
    #[must_use]
    pub const fn new(content_size: PhysicalSize, native_scale_factor: ScaleFactor) -> Self {
        Self {
            content_size,
            native_scale_factor,
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
    binding: NativeWindowBinding,
    generation: CoordinateGeneration,
    content_size: PhysicalSize,
    native_scale_factor: ScaleFactor,
}

impl NativeLocalCoordinates {
    pub(super) const fn new(
        backend: BackendIngressLease,
        binding: NativeWindowBinding,
        snapshot: WindowCoordinateSnapshot,
    ) -> Self {
        Self {
            backend,
            binding,
            generation: snapshot.coordinate_generation(),
            content_size: snapshot.content_size(),
            native_scale_factor: snapshot.native_scale_factor(),
        }
    }

    /// Returns the exact native-window incarnation authorized by this capability.
    #[must_use]
    pub const fn binding(self) -> NativeWindowBinding {
        self.binding
    }

    /// Returns the committed local geometry required at capture time.
    #[must_use]
    pub const fn geometry(self) -> NativeLocalGeometry {
        NativeLocalGeometry::new(self.content_size, self.native_scale_factor)
    }

    /// Captures one event using its original physical point and frozen geometry.
    ///
    /// Event and callback size/native scale must match exactly, and the callback
    /// must match this capability's complete local geometry. The original
    /// physical client point is preserved, including finite points outside
    /// the client area, without clamping or granting desktop outside-all authority.
    /// The reducer validates current coordinate authority again at its ordinal.
    ///
    /// # Errors
    ///
    /// Returns an error for a different binding, mismatched event or callback
    /// geometry.
    pub fn capture(
        self,
        binding: NativeWindowBinding,
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
        Ok(NativeLocalPointerSample {
            coordinates: self,
            client_position: event.client_position,
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
/// The physical point and its window geometry travel together. Surface placement
/// selection and logical conversion belong to the reducer ordinal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalPointerSample {
    coordinates: NativeLocalCoordinates,
    client_position: PhysicalPoint,
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
        expected: NativeWindowBinding,
        /// Exact native-window incarnation supplied by the callback.
        observed: NativeWindowBinding,
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
    use crate::coordinates::CoordinateSnapshot;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::intent::{Authority, AuthorityUnavailableReason};
    use crate::platform::WindowCoordinateObservation;
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::pointer_journal::{PointerInputLease, PointerProviderScope};
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{
        CoordinateObservationGeneration, ViewportBinding, WindowIncarnation, WindowToken,
    };

    fn size(width: f64, height: f64) -> PhysicalSize {
        PhysicalSize::new(width, height).unwrap()
    }
    fn scale(value: f64) -> ScaleFactor {
        ScaleFactor::new(value).unwrap()
    }
    fn point(x: f64, y: f64) -> PhysicalPoint {
        PhysicalPoint::new(x, y).unwrap()
    }

    fn coordinates(native_scale: f64, generation: u64) -> NativeLocalCoordinates {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let platform = PlatformObservationAuthority::new(domain).create().unwrap();
        let host = PresentationLedger::new(domain).create_host().unwrap();
        let pointer = PointerInputLease::new(domain, 1, PointerProviderScope::NativeJoined(host));
        let backend = BackendIngressLease::new(platform, pointer, host).unwrap();
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
            Authority::Known(scale(3.0)),
        )
        .with_content_size(Authority::Known(size(800.0, 600.0)));
        let snapshot = CoordinateSnapshot::from_observation(
            binding,
            CoordinateGeneration::new(generation),
            observation,
        )
        .unwrap();
        NativeLocalCoordinates::new(
            backend,
            NativeWindowBinding::from_binding(platform, binding.window()),
            snapshot.window_coordinates(),
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
        let coordinates = coordinates(2.0, 7);
        let callback = coordinates.geometry();
        for event in [
            NativeLocalPointerFact::new(point(12.0, 6.0), size(801.0, 600.0), scale(2.0)),
            NativeLocalPointerFact::new(point(12.0, 6.0), size(800.0, 601.0), scale(2.0)),
            NativeLocalPointerFact::new(point(12.0, 6.0), size(800.0, 600.0), scale(1.0)),
        ] {
            assert_eq!(
                coordinates.capture(coordinates.binding(), event, callback),
                Err(NativeLocalCoordinateError::EventGeometryMismatch)
            );
        }
    }

    #[test]
    fn callback_geometry_mismatch_is_rejected_even_when_event_matches() {
        let coordinates = coordinates(2.0, 7);
        for callback in [
            NativeLocalGeometry::new(size(801.0, 600.0), scale(2.0)),
            NativeLocalGeometry::new(size(800.0, 601.0), scale(2.0)),
            NativeLocalGeometry::new(size(800.0, 600.0), scale(1.0)),
        ] {
            assert_eq!(
                coordinates.capture(
                    coordinates.binding(),
                    event(callback, point(12.0, 6.0)),
                    callback
                ),
                Err(NativeLocalCoordinateError::CallbackGeometryMismatch)
            );
        }
    }

    #[test]
    fn queued_old_scale_event_cannot_be_reinterpreted_by_a_new_callback() {
        let old = coordinates(1.0, 7);
        let current = coordinates(2.0, 8);
        let old_event = event(old.geometry(), point(120.0, 60.0));
        assert_eq!(
            current.capture(current.binding(), old_event, current.geometry()),
            Err(NativeLocalCoordinateError::EventGeometryMismatch)
        );
        assert_eq!(
            old.capture(old.binding(), old_event, current.geometry()),
            Err(NativeLocalCoordinateError::EventGeometryMismatch)
        );
    }

    #[test]
    fn outside_and_extreme_physical_points_remain_unclamped_window_facts() {
        let coordinates = coordinates(2.0, 7);
        for point in [point(-32.0, 900.0), point(f64::MAX, -f64::MAX)] {
            let sample = coordinates
                .capture(
                    coordinates.binding(),
                    event(coordinates.geometry(), point),
                    coordinates.geometry(),
                )
                .unwrap();
            assert_eq!(sample.client_position(), point);
            assert_eq!(sample.coordinates(), coordinates);
        }
    }

    #[test]
    fn same_geometry_cannot_rebind_a_sample_to_another_window_incarnation() {
        let coordinates = coordinates(2.0, 7);
        let binding = coordinates.binding();
        let foreign = NativeWindowBinding::from_binding(
            binding.provider,
            crate::viewport::WindowBinding::new(
                binding.window.authority_domain(),
                binding.window.epoch(),
                binding.window.token(),
                WindowIncarnation::new(2),
            ),
        );
        assert_eq!(
            coordinates.capture(
                foreign,
                event(coordinates.geometry(), point(12.0, 6.0)),
                coordinates.geometry()
            ),
            Err(NativeLocalCoordinateError::BindingMismatch {
                expected: binding,
                observed: foreign
            })
        );
    }

    #[test]
    fn motion_compaction_keeps_window_coordinate_generations_separate() {
        let old = coordinates(2.0, 7);
        let current = NativeLocalCoordinates {
            generation: CoordinateGeneration::new(8),
            ..old
        };
        let capture = |cap: NativeLocalCoordinates, position| {
            cap.capture(
                cap.binding(),
                event(cap.geometry(), position),
                cap.geometry(),
            )
            .unwrap()
        };
        let sample = capture(old, point(12.0, 6.0));
        assert!(sample.same_coordinates(capture(old, point(24.0, 12.0))));
        assert!(!sample.same_coordinates(capture(current, point(12.0, 6.0))));
    }
}
