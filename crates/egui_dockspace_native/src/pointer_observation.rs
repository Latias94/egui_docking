//! Receipt-correlated native pointer observations frozen before ordered replay.

use std::time::Duration;

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerContinuation,
    NativePointerHover, NativePointerInput, NativePointerOwner, NativeSurfaceBinding,
    NativeWorkAreaBinding, PointerDeviceKind, PointerEventMetadata,
};
use eframe::{
    NativeViewportPointerObservation, NativeViewportPointerObservationCommandToken,
    NativeViewportPointerObservationResult, egui::ViewportId,
};
use winit::event::{DeviceId, PointerWindowRoute};
use winit::window::WindowId;

use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerObservationKey {
    pub(crate) token: NativeViewportPointerObservationCommandToken,
    pub(crate) viewport: ViewportId,
    pub(crate) window: WindowId,
    pub(crate) device: DeviceId,
}

impl NativePointerObservationKey {
    pub(crate) const fn from_result(result: NativeViewportPointerObservationResult) -> Self {
        Self {
            token: result.token(),
            viewport: result.viewport_id(),
            window: result.expected_window_id(),
            device: result.device_id(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NativePointerObservationRecord {
    key: NativePointerObservationKey,
    binding: NativeSurfaceBinding,
    sample: Option<NativePointerObservationSample>,
}

impl NativePointerObservationRecord {
    /// The matching command receipt supplies the source, even after route replacement.
    pub(crate) fn capture(
        result: NativeViewportPointerObservationResult,
        binding: NativeSurfaceBinding,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self {
            key: NativePointerObservationKey::from_result(result),
            binding,
            sample: NativePointerObservationSample::capture(result.observation(), viewports),
        }
    }

    pub(crate) const fn retired(
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            key,
            binding,
            sample: None,
        }
    }

    pub(crate) const fn key(self) -> NativePointerObservationKey {
        self.key
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        self.binding == binding
            || self.sample.is_some_and(|sample| {
                sample.hover == NativePointerHover::Dock(binding.window())
                    || sample.delivery == NativePointerOwner::Native(binding.window())
                    || sample.capture == NativePointerOwner::Native(binding.window())
            })
    }

    pub(crate) fn into_input_event(
        self,
        request: NativePointerContinuation,
        event: dockspace::runtime::NativePointerEvent,
        captured_at: Duration,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> Option<NativePointerInput> {
        if request.source_binding() != self.binding || request.local_coordinates().is_some() {
            return None;
        }
        let sample = self.sample?;
        Some(
            NativePointerInput::new(
                request.pointer(),
                event,
                sample.desktop_location(resolve_work_area),
                sample.delivery,
                sample.capture,
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Mouse,
                captured_at,
            )),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
        observation: NativeViewportPointerObservation,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self {
            key,
            binding,
            sample: NativePointerObservationSample::capture(observation, viewports),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NativePointerObservationSample {
    position: NativeDesktopPosition,
    hover: NativePointerHover,
    delivery: NativePointerOwner,
    capture: NativePointerOwner,
}

impl NativePointerObservationSample {
    fn capture(
        observation: NativeViewportPointerObservation,
        viewports: &NativeViewportMap,
    ) -> Option<Self> {
        let NativeViewportPointerObservation::Observed { facts, delivery } = observation else {
            return None;
        };
        let position = facts
            .desktop_position
            .and_then(|position| PhysicalPoint::new(position.x, position.y).ok())
            .map_or(NativeDesktopPosition::Unknown, NativeDesktopPosition::Exact);
        let hover = match from_winit(facts.hover, viewports) {
            NativePointerOwner::Unknown => NativePointerHover::Unknown,
            NativePointerOwner::None => NativePointerHover::OutsideAll,
            NativePointerOwner::Native(binding) => NativePointerHover::Dock(binding),
            NativePointerOwner::Foreign => NativePointerHover::Foreign,
        };
        Some(Self {
            position,
            hover,
            delivery: from_winit(delivery, viewports),
            capture: from_winit(facts.capture, viewports),
        })
    }

    fn desktop_location(
        self,
        mut resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativeDesktopPointerLocation {
        let work_area = match (self.position, self.hover) {
            (NativeDesktopPosition::Exact(point), NativePointerHover::OutsideAll) => {
                resolve_work_area(point)
            }
            _ => None,
        };
        NativeDesktopPointerLocation::new(self.position, self.hover, work_area)
    }
}

fn from_winit(route: PointerWindowRoute, viewports: &NativeViewportMap) -> NativePointerOwner {
    match route {
        PointerWindowRoute::Unknown => NativePointerOwner::Unknown,
        PointerWindowRoute::None => NativePointerOwner::None,
        PointerWindowRoute::Window(window) => viewports
            .pointer_binding_for_window(window)
            .map_or(NativePointerOwner::Unknown, NativePointerOwner::Native),
        PointerWindowRoute::Foreign => NativePointerOwner::Foreign,
    }
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
        SurfaceUnavailableReason,
    };
    use winit::dpi::PhysicalPosition;
    use winit::event::PointerEventFacts;

    use super::*;

    fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([1, 2].map(|id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(
                    RootId::new(id),
                    DockspaceNode::central_tabs([ItemId::new(id)]),
                ),
            )
        }))
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        for id in [1, 2] {
            session
                .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
                .unwrap();
        }
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        let mut bindings = report.inputs().iter().filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        });
        (bindings.next().unwrap(), bindings.next().unwrap())
    }

    fn key() -> NativePointerObservationKey {
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(70);
        let device = DeviceId::dummy();
        let prepared =
            eframe::prepare_native_viewport_pointer_observation(viewport, window, device, None);
        NativePointerObservationKey {
            token: prepared.token(),
            viewport,
            window,
            device,
        }
    }

    #[test]
    fn unknown_capture_and_delivery_never_borrow_source() {
        let (source, _) = bindings();
        let key = key();
        let mut viewports = NativeViewportMap::default();
        viewports.bind(key.viewport, key.window, source).unwrap();
        let record = NativePointerObservationRecord::for_test(
            key,
            source,
            NativeViewportPointerObservation::Observed {
                facts: PointerEventFacts {
                    surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                    desktop_position: Some(PhysicalPosition::new(-200.75, 300.5)),
                    hover: PointerWindowRoute::Window(key.window),
                    capture: PointerWindowRoute::Unknown,
                    ..Default::default()
                },
                delivery: PointerWindowRoute::Unknown,
            },
            &viewports,
        );

        assert_eq!(record.key(), key);
        assert_eq!(record.binding(), source);
        assert_eq!(
            record.sample,
            Some(NativePointerObservationSample {
                position: NativeDesktopPosition::Exact(PhysicalPoint::new(-200.75, 300.5).unwrap()),
                hover: NativePointerHover::Dock(source.window()),
                delivery: NativePointerOwner::Unknown,
                capture: NativePointerOwner::Unknown,
            }),
        );
    }

    #[test]
    fn frozen_routes_keep_receipt_binding_after_map_replacement() {
        let (source, target) = bindings();
        let (current_source, successor) = bindings();
        let key = key();
        let target_viewport = ViewportId::from_hash_of("observed-target");
        let target_window = WindowId::from(71);
        let observed = PointerWindowRoute::Window(target_window);
        for (hover, delivery, capture) in [
            (
                observed,
                PointerWindowRoute::Unknown,
                PointerWindowRoute::Unknown,
            ),
            (
                PointerWindowRoute::Unknown,
                observed,
                PointerWindowRoute::Unknown,
            ),
            (
                PointerWindowRoute::Unknown,
                PointerWindowRoute::Unknown,
                observed,
            ),
        ] {
            let mut viewports = NativeViewportMap::default();
            viewports
                .bind(key.viewport, key.window, current_source)
                .unwrap();
            viewports
                .bind(target_viewport, target_window, target)
                .unwrap();
            let observation = NativeViewportPointerObservation::Observed {
                facts: PointerEventFacts {
                    hover,
                    capture,
                    ..Default::default()
                },
                delivery,
            };
            let record =
                NativePointerObservationRecord::for_test(key, source, observation, &viewports);
            let frozen = record.sample;
            assert_eq!(record.binding(), source);
            assert_eq!(
                frozen,
                Some(NativePointerObservationSample {
                    position: NativeDesktopPosition::Unknown,
                    hover: if hover == observed {
                        NativePointerHover::Dock(target.window())
                    } else {
                        NativePointerHover::Unknown
                    },
                    delivery: if delivery == observed {
                        NativePointerOwner::Native(target.window())
                    } else {
                        NativePointerOwner::Unknown
                    },
                    capture: if capture == observed {
                        NativePointerOwner::Native(target.window())
                    } else {
                        NativePointerOwner::Unknown
                    },
                }),
            );

            assert!(
                viewports
                    .replace(target_viewport, target, target_window, successor)
                    .unwrap()
                    .is_none()
            );

            assert_eq!(
                viewports.pointer_binding_for_window(target_window),
                Some(successor.window()),
            );
            assert_eq!(record.sample, frozen);
            assert!(record.references_binding(source));
            assert!(record.references_binding(target));
            assert!(!record.references_binding(current_source));
            assert!(!record.references_binding(successor));
            assert_ne!(
                record.sample,
                NativePointerObservationSample::capture(observation, &viewports),
            );
        }
    }

    #[test]
    fn unmapped_routes_stay_unknown_after_window_attachment() {
        let (source, _) = bindings();
        let key = key();
        let mut viewports = NativeViewportMap::default();
        let observation = NativeViewportPointerObservation::Observed {
            facts: PointerEventFacts {
                surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                hover: PointerWindowRoute::Window(key.window),
                capture: PointerWindowRoute::Window(key.window),
                ..Default::default()
            },
            delivery: PointerWindowRoute::Window(key.window),
        };
        let record = NativePointerObservationRecord::for_test(key, source, observation, &viewports);
        let unknown = Some(NativePointerObservationSample {
            position: NativeDesktopPosition::Unknown,
            hover: NativePointerHover::Unknown,
            delivery: NativePointerOwner::Unknown,
            capture: NativePointerOwner::Unknown,
        });
        assert_eq!(record.sample, unknown);

        viewports.bind(key.viewport, key.window, source).unwrap();

        assert_eq!(record.sample, unknown);
        assert_ne!(
            record.sample,
            NativePointerObservationSample::capture(observation, &viewports),
        );
    }

    #[test]
    fn unavailable_unsupported_and_retired_have_no_sample() {
        let (source, other) = bindings();
        let key = key();
        let mut viewports = NativeViewportMap::default();
        viewports.bind(key.viewport, key.window, source).unwrap();
        for observation in [
            NativeViewportPointerObservation::Unavailable,
            NativeViewportPointerObservation::Unsupported,
        ] {
            let record =
                NativePointerObservationRecord::for_test(key, source, observation, &viewports);
            assert_eq!(record.key(), key);
            assert_eq!(record.binding(), source);
            assert_eq!(record.sample, None);
            assert!(record.references_binding(source));
            assert!(!record.references_binding(other));
        }
        let retired = NativePointerObservationRecord::retired(key, source);
        assert_eq!(retired.key(), key);
        assert_eq!(retired.binding(), source);
        assert_eq!(retired.sample, None);
        assert!(retired.references_binding(source));
        assert!(!retired.references_binding(other));
    }

    #[test]
    fn work_area_resolution_requires_outside_all_and_exact_position() {
        let (source, _) = bindings();
        let key = key();
        let mut viewports = NativeViewportMap::default();
        viewports.bind(key.viewport, key.window, source).unwrap();
        let exact = Some(PhysicalPosition::new(-200.75, 300.5));
        let point = PhysicalPoint::new(-200.75, 300.5).unwrap();
        for (desktop_position, hover, should_resolve) in [
            (exact, PointerWindowRoute::None, true),
            (exact, PointerWindowRoute::Unknown, false),
            (exact, PointerWindowRoute::Foreign, false),
            (exact, PointerWindowRoute::Window(key.window), false),
            (None, PointerWindowRoute::None, false),
            (
                Some(PhysicalPosition::new(f64::NAN, 300.5)),
                PointerWindowRoute::None,
                false,
            ),
            (
                Some(PhysicalPosition::new(-200.75, f64::INFINITY)),
                PointerWindowRoute::None,
                false,
            ),
        ] {
            let record = NativePointerObservationRecord::for_test(
                key,
                source,
                NativeViewportPointerObservation::Observed {
                    facts: PointerEventFacts {
                        desktop_position,
                        hover,
                        capture: PointerWindowRoute::Foreign,
                        ..Default::default()
                    },
                    delivery: PointerWindowRoute::None,
                },
                &viewports,
            );
            let sample = record.sample.unwrap();
            assert_eq!(sample.delivery, NativePointerOwner::None);
            assert_eq!(sample.capture, NativePointerOwner::Foreign);
            assert_eq!(
                sample.position,
                if desktop_position == exact {
                    NativeDesktopPosition::Exact(point)
                } else {
                    NativeDesktopPosition::Unknown
                },
            );
            let mut resolved = 0;
            let location = sample.desktop_location(|position| {
                assert_eq!(position, point);
                resolved += 1;
                None
            });
            assert_eq!(resolved, i32::from(should_resolve));
            assert_eq!(
                location,
                NativeDesktopPointerLocation::new(sample.position, sample.hover, None),
            );
        }
    }
}
