//! Authoritative cross-surface pointer routing with opaque generation proofs.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::coordinates::CoordinateUnavailable;
use crate::effect::EffectId;
use crate::ids::SurfaceId;
use crate::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
    SurfacePointer,
};
use crate::platform::{
    ButtonObservation, InputEffectAcknowledgement, PlatformCapability, PlatformCapabilityIssue,
    PlatformSnapshot, PointerObservation, PointerWindow, WindowInputState,
};
use crate::viewport::{
    CapabilityGeneration, InputObservationGeneration, InventoryGeneration, RouteGeneration,
    ViewportBinding,
};
use crate::viewport_registry::ViewportRegistry;

/// Exact generation tuple which owns a route proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewportRouteStamp {
    capability: CapabilityGeneration,
    inventory: InventoryGeneration,
    route: RouteGeneration,
}

impl ViewportRouteStamp {
    #[must_use]
    pub const fn capability_generation(self) -> CapabilityGeneration {
        self.capability
    }

    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory
    }

    #[must_use]
    pub const fn route_generation(self) -> RouteGeneration {
        self.route
    }
}

/// Why a platform pointer observation could not produce an authoritative route.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewportRouteUnavailable {
    Capability(PlatformCapabilityIssue),
    HoverAuthority(AuthorityUnavailableReason),
    PointerAuthority(AuthorityUnavailableReason),
    OpaqueWindowBlocker,
    UnknownWindowToken,
    TargetNotReady { surface: SurfaceId },
    TargetNotRouteable { surface: SurfaceId },
    SourceBindingStale,
    SourcePassthroughNotObserved,
    SourceInputAuthorityUnavailable(AuthorityUnavailableReason),
    SourceNotRouteable,
    Coordinate(CoordinateUnavailable),
}

/// Opaque route proof produced only by the core coordinator.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRouteProof {
    pointer: PointerId,
    stamp: ViewportRouteStamp,
    desktop_position: Authority<crate::geometry::PhysicalPoint>,
    target: Authority<Option<SurfacePointer>>,
    target_binding: Option<ViewportBinding>,
    buttons: Authority<Vec<ButtonObservation>>,
}

/// Core-owned proof requirement for one active drag source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewportRouteSource {
    binding: ViewportBinding,
    passthrough: SourcePassthroughAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourcePassthroughAuthority {
    Preexisting,
    CoreEffect {
        effect: EffectId,
        issued_after: InputObservationGeneration,
    },
    Unavailable,
}

impl ViewportRouteSource {
    pub(crate) const fn preexisting(binding: ViewportBinding) -> Self {
        Self {
            binding,
            passthrough: SourcePassthroughAuthority::Preexisting,
        }
    }

    pub(crate) const fn core_effect(
        binding: ViewportBinding,
        effect: EffectId,
        issued_after: InputObservationGeneration,
    ) -> Self {
        Self {
            binding,
            passthrough: SourcePassthroughAuthority::CoreEffect {
                effect,
                issued_after,
            },
        }
    }

    pub(crate) const fn unavailable(binding: ViewportBinding) -> Self {
        Self {
            binding,
            passthrough: SourcePassthroughAuthority::Unavailable,
        }
    }
}

impl ViewportRouteProof {
    #[must_use]
    pub const fn pointer(&self) -> PointerId {
        self.pointer
    }

    #[must_use]
    pub const fn stamp(&self) -> ViewportRouteStamp {
        self.stamp
    }

    #[must_use]
    pub const fn desktop_position(&self) -> &Authority<crate::geometry::PhysicalPoint> {
        &self.desktop_position
    }

    #[must_use]
    pub const fn target(&self) -> &Authority<Option<SurfacePointer>> {
        &self.target
    }

    #[must_use]
    pub const fn target_binding(&self) -> Option<ViewportBinding> {
        self.target_binding
    }

    #[must_use]
    pub fn button_state(&self, button: PointerButton) -> Authority<PointerButtonState> {
        match &self.buttons {
            Authority::Unknown(reason) => Authority::Unknown(*reason),
            Authority::Known(buttons) => buttons
                .iter()
                .find(|observation| observation.button() == button)
                .map_or(
                    Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    |observation| Authority::Known(observation.state()),
                ),
        }
    }
}

/// Published route proof set for the latest accepted platform snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewportRouteState {
    last_generation: RouteGeneration,
    proofs: BTreeMap<PointerId, ViewportRouteProof>,
}

impl ViewportRouteState {
    /// Replaces all routes from one complete snapshot.
    pub(crate) fn publish(
        &mut self,
        snapshot: &PlatformSnapshot,
        registry: &ViewportRegistry,
        capability_generation: CapabilityGeneration,
        drag_sources: &BTreeMap<PointerId, ViewportRouteSource>,
    ) -> Result<RouteGeneration, ViewportRouteError> {
        let generation = self
            .last_generation
            .checked_next()
            .ok_or(ViewportRouteError::RouteGenerationExhausted)?;
        let stamp = ViewportRouteStamp {
            capability: capability_generation,
            inventory: registry.inventory_generation(),
            route: generation,
        };
        let proofs = snapshot
            .pointers()
            .iter()
            .map(|observation| {
                let source = drag_sources.get(&observation.pointer()).copied();
                let proof = resolve_observation(snapshot, registry, stamp, observation, source);
                (observation.pointer(), proof)
            })
            .collect();
        self.last_generation = generation;
        self.proofs = proofs;
        Ok(generation)
    }

    #[must_use]
    pub const fn generation(&self) -> RouteGeneration {
        self.last_generation
    }

    #[must_use]
    pub fn proof(&self, pointer: PointerId) -> Option<&ViewportRouteProof> {
        self.proofs.get(&pointer)
    }

    #[must_use]
    pub fn is_current(&self, proof: &ViewportRouteProof) -> bool {
        self.proofs
            .get(&proof.pointer)
            .is_some_and(|current| current == proof)
    }

    pub(crate) fn clear(&mut self) {
        self.proofs.clear();
    }

    #[cfg(test)]
    pub(crate) fn exhaust_generation(&mut self) {
        self.last_generation = RouteGeneration::new(u64::MAX);
    }
}

fn resolve_observation(
    snapshot: &PlatformSnapshot,
    registry: &ViewportRegistry,
    stamp: ViewportRouteStamp,
    observation: &PointerObservation,
    source: Option<ViewportRouteSource>,
) -> ViewportRouteProof {
    let routed = resolve_target(snapshot, registry, observation, source);
    let (target, target_binding) = match routed {
        Ok((target, binding)) => (Authority::Known(target), binding),
        Err(reason) => (Authority::Unknown(authority_reason(&reason)), None),
    };
    let buttons = match snapshot.capabilities().authoritative_release() {
        PlatformCapability::Supported => observation.button_states().clone(),
        PlatformCapability::Unsupported(issue) | PlatformCapability::Unknown(issue) => {
            Authority::Unknown(authority_reason(&ViewportRouteUnavailable::Capability(
                issue,
            )))
        }
    };
    ViewportRouteProof {
        pointer: observation.pointer(),
        stamp,
        desktop_position: *observation.desktop_position(),
        target,
        target_binding,
        buttons,
    }
}

fn resolve_target(
    snapshot: &PlatformSnapshot,
    registry: &ViewportRegistry,
    observation: &PointerObservation,
    source: Option<ViewportRouteSource>,
) -> Result<(Option<SurfacePointer>, Option<ViewportBinding>), ViewportRouteUnavailable> {
    if let PlatformCapability::Unsupported(issue) | PlatformCapability::Unknown(issue) =
        snapshot.capabilities().cross_surface_routing()
    {
        return Err(ViewportRouteUnavailable::Capability(issue));
    }
    require_source_passthrough(registry, source)?;
    let hovered = match observation.hovered() {
        Authority::Known(hovered) => *hovered,
        Authority::Unknown(reason) => {
            return Err(ViewportRouteUnavailable::HoverAuthority(*reason));
        }
    };
    match hovered {
        PointerWindow::Foreign => Err(ViewportRouteUnavailable::OpaqueWindowBlocker),
        PointerWindow::None => Ok((None, None)),
        PointerWindow::Dock(token) => {
            let binding = registry
                .binding_for_token(token)
                .ok_or(ViewportRouteUnavailable::UnknownWindowToken)?;
            let record = registry
                .record(binding.surface())
                .ok_or(ViewportRouteUnavailable::UnknownWindowToken)?;
            if !record.is_routeable() {
                return Err(ViewportRouteUnavailable::TargetNotRouteable {
                    surface: binding.surface(),
                });
            }
            let coordinates =
                record
                    .coordinates()
                    .ok_or(ViewportRouteUnavailable::TargetNotReady {
                        surface: binding.surface(),
                    })?;
            let point = match observation.desktop_position() {
                Authority::Known(point) => *point,
                Authority::Unknown(reason) => {
                    return Err(ViewportRouteUnavailable::PointerAuthority(*reason));
                }
            };
            let logical = coordinates.desktop_to_surface(point).map_err(|source| {
                ViewportRouteUnavailable::Coordinate(CoordinateUnavailable::Geometry(source))
            })?;
            Ok((
                Some(SurfacePointer::new(binding.surface(), logical)),
                Some(binding),
            ))
        }
    }
}

fn require_source_passthrough(
    registry: &ViewportRegistry,
    source: Option<ViewportRouteSource>,
) -> Result<(), ViewportRouteUnavailable> {
    let Some(source) = source else {
        return Ok(());
    };
    let binding = source.binding;
    let record = registry
        .record(binding.surface())
        .filter(|record| record.binding() == binding)
        .ok_or(ViewportRouteUnavailable::SourceBindingStale)?;
    if !record.is_routeable() {
        return Err(ViewportRouteUnavailable::SourceNotRouteable);
    }
    record
        .coordinates()
        .ok_or(ViewportRouteUnavailable::SourcePassthroughNotObserved)?;
    let observation = record
        .input_observation()
        .ok_or(ViewportRouteUnavailable::SourcePassthroughNotObserved)?;
    if observation.known_state() != Some(WindowInputState::PassThrough) {
        return Err(ViewportRouteUnavailable::SourcePassthroughNotObserved);
    }
    match source.passthrough {
        SourcePassthroughAuthority::Preexisting => {}
        SourcePassthroughAuthority::CoreEffect {
            effect,
            issued_after,
        } if observation.generation() > issued_after || observation.acknowledges(effect) => {}
        SourcePassthroughAuthority::CoreEffect { .. } => {
            if let InputEffectAcknowledgement::Unknown(reason) = observation.acknowledged_effect() {
                return Err(ViewportRouteUnavailable::SourceInputAuthorityUnavailable(
                    reason,
                ));
            }
            return Err(ViewportRouteUnavailable::SourcePassthroughNotObserved);
        }
        SourcePassthroughAuthority::Unavailable => {
            return Err(ViewportRouteUnavailable::SourcePassthroughNotObserved);
        }
    }
    Ok(())
}

const fn authority_reason(reason: &ViewportRouteUnavailable) -> AuthorityUnavailableReason {
    match reason {
        ViewportRouteUnavailable::Capability(issue) => match issue.reason() {
            crate::platform::PlatformCapabilityReason::PermissionDenied => {
                AuthorityUnavailableReason::PermissionDenied
            }
            crate::platform::PlatformCapabilityReason::BackendUnsupported
            | crate::platform::PlatformCapabilityReason::NotReported
            | crate::platform::PlatformCapabilityReason::EnvironmentUnavailable => {
                AuthorityUnavailableReason::ProviderUnavailable
            }
        },
        ViewportRouteUnavailable::PointerAuthority(reason)
        | ViewportRouteUnavailable::HoverAuthority(reason)
        | ViewportRouteUnavailable::SourceInputAuthorityUnavailable(reason) => *reason,
        ViewportRouteUnavailable::Coordinate(_) => {
            AuthorityUnavailableReason::CoordinateUnavailable
        }
        ViewportRouteUnavailable::UnknownWindowToken
        | ViewportRouteUnavailable::OpaqueWindowBlocker
        | ViewportRouteUnavailable::TargetNotReady { .. }
        | ViewportRouteUnavailable::TargetNotRouteable { .. }
        | ViewportRouteUnavailable::SourceBindingStale
        | ViewportRouteUnavailable::SourcePassthroughNotObserved
        | ViewportRouteUnavailable::SourceNotRouteable => {
            AuthorityUnavailableReason::SurfaceUnavailable
        }
    }
}

/// Fatal route-generation allocation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ViewportRouteError {
    #[error("viewport route generation is exhausted")]
    RouteGenerationExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
    use crate::ids::{SurfaceId, WorkspaceEpoch};
    use crate::platform::{
        InputEffectAcknowledgement, PlatformCapabilities, PlatformCapability, PlatformRequirement,
        WindowInputObservation, WindowInputState, WindowPresentationState,
    };
    use crate::viewport::{InputObservationGeneration, ViewportRole, WindowToken};
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    fn unknown_focus(generation: u64) -> crate::viewport_focus::FocusObservationEnvelope {
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        )
    }

    fn capabilities() -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_authoritative_button_state(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
        capabilities
    }

    fn window(
        binding: ViewportBinding,
        generation: u64,
        x: f64,
        scale: f64,
        input: WindowInputState,
    ) -> crate::platform::ObservedWindow {
        window_with_ack(
            binding,
            generation,
            x,
            scale,
            input,
            InputEffectAcknowledgement::known(None),
        )
    }

    fn window_with_ack(
        binding: ViewportBinding,
        generation: u64,
        x: f64,
        scale: f64,
        input: WindowInputState,
        acknowledgement: InputEffectAcknowledgement,
    ) -> crate::platform::ObservedWindow {
        crate::platform::ObservedWindow::new(binding.token())
            .with_content_bounds(Authority::Known(
                PhysicalRect::new(x, 0.0, 600.0, 400.0).expect("test bounds must be valid"),
            ))
            .with_scale_factor(Authority::Known(
                ScaleFactor::new(scale).expect("test scale must be valid"),
            ))
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(generation),
                Authority::Known(input),
                acknowledgement,
            ))
            .with_presentation(Authority::Known(WindowPresentationState::Visible))
            .with_close_requested(Authority::Known(false))
    }

    fn pointer(hovered: PointerWindow, x: f64) -> PointerObservation {
        PointerObservation::new(
            PointerId::new(1),
            Authority::Known(hovered),
            Authority::Known(PhysicalPoint::new(x, 150.0).expect("test point must be valid")),
            Authority::Known(vec![ButtonObservation::new(
                PointerButton::Primary,
                PointerButtonState::Released,
            )]),
        )
        .expect("test pointer must be valid")
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn target_scale_is_used_once_and_opaque_foreign_windows_fail_closed() {
        let mut registry = ViewportRegistry::default();
        let source_token = WindowToken::new(10);
        let target_token = WindowToken::new(20);
        let source = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                source_token,
                ViewportRole::Root,
            )
            .expect("source must register");
        let target_binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(2),
                target_token,
                ViewportRole::Child,
            )
            .expect("target must register");
        let facts = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(1),
            vec![
                window(source, 1, 0.0, 1.0, WindowInputState::PassThrough),
                window(
                    target_binding,
                    1,
                    1000.0,
                    2.0,
                    WindowInputState::ReceivesInput,
                ),
            ],
            vec![pointer(PointerWindow::Dock(target_token), 1200.0)],
            Vec::new(),
        )
        .expect("test facts must be valid");
        registry
            .apply_snapshot(&facts)
            .expect("test inventory must apply");
        let mut routes = ViewportRouteState::default();
        routes
            .publish(
                &facts,
                &registry,
                CapabilityGeneration::new(1),
                &BTreeMap::from([(PointerId::new(1), ViewportRouteSource::preexisting(source))]),
            )
            .expect("route must publish");
        let proof = routes.proof(PointerId::new(1)).expect("route must exist");
        let Authority::Known(Some(target)) = proof.target() else {
            panic!("target must be known");
        };
        assert!((target.position().x() - 100.0).abs() < f64::EPSILON);
        assert!((target.position().y() - 75.0).abs() < f64::EPSILON);

        let foreign = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(2),
            vec![
                window(source, 2, 0.0, 1.0, WindowInputState::PassThrough),
                window(
                    target_binding,
                    2,
                    1000.0,
                    2.0,
                    WindowInputState::ReceivesInput,
                ),
            ],
            vec![pointer(PointerWindow::Foreign, 1200.0)],
            Vec::new(),
        )
        .expect("foreign facts must be valid");
        registry
            .apply_snapshot(&foreign)
            .expect("foreign inventory must apply");
        routes
            .publish(
                &foreign,
                &registry,
                CapabilityGeneration::new(2),
                &BTreeMap::from([(PointerId::new(1), ViewportRouteSource::preexisting(source))]),
            )
            .expect("foreign route must publish");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
        ));

        let outside = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(3),
            vec![
                window(source, 3, 0.0, 1.0, WindowInputState::PassThrough),
                window(
                    target_binding,
                    3,
                    1000.0,
                    2.0,
                    WindowInputState::ReceivesInput,
                ),
            ],
            vec![pointer(PointerWindow::None, 1200.0)],
            Vec::new(),
        )
        .expect("trusted outside facts must be valid");
        registry
            .apply_snapshot(&outside)
            .expect("trusted outside inventory must apply");
        routes
            .publish(
                &outside,
                &registry,
                CapabilityGeneration::new(3),
                &BTreeMap::from([(PointerId::new(1), ViewportRouteSource::preexisting(source))]),
            )
            .expect("trusted outside route must publish");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Known(None)
        ));
    }

    #[test]
    fn requested_but_unobserved_passthrough_fails_closed() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(1);
        let source = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Root,
            )
            .expect("source must register");
        let facts = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(1),
            vec![window(source, 1, 0.0, 1.0, WindowInputState::ReceivesInput)],
            vec![pointer(PointerWindow::None, 700.0)],
            Vec::new(),
        )
        .expect("facts must be valid");
        registry
            .apply_snapshot(&facts)
            .expect("inventory must apply");
        let mut routes = ViewportRouteState::default();
        routes
            .publish(
                &facts,
                &registry,
                CapabilityGeneration::new(1),
                &BTreeMap::from([(PointerId::new(1), ViewportRouteSource::preexisting(source))]),
            )
            .expect("unavailable proof still publishes");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
        ));
    }

    #[test]
    fn preexisting_passthrough_does_not_require_effect_acknowledgement_authority() {
        let mut registry = ViewportRegistry::default();
        let source = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(1),
                ViewportRole::Root,
            )
            .expect("source must register");
        let source_window = window(source, 1, 0.0, 1.0, WindowInputState::PassThrough)
            .with_input_observation(WindowInputObservation::new(
                source,
                InputObservationGeneration::new(1),
                Authority::Known(WindowInputState::PassThrough),
                InputEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
            ));
        let facts = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(1),
            vec![source_window],
            vec![pointer(PointerWindow::None, 700.0)],
            Vec::new(),
        )
        .expect("facts must be valid");
        registry
            .apply_snapshot(&facts)
            .expect("inventory must apply");
        let mut routes = ViewportRouteState::default();
        routes
            .publish(
                &facts,
                &registry,
                CapabilityGeneration::new(1),
                &BTreeMap::from([(PointerId::new(1), ViewportRouteSource::preexisting(source))]),
            )
            .expect("preexisting passthrough proof must publish");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Known(None)
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn core_effect_route_requires_post_baseline_capture_or_exact_acknowledgement() {
        let mut registry = ViewportRegistry::default();
        let source = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(1),
                ViewportRole::Root,
            )
            .expect("source must register");
        let exact_registry_seed = registry.clone();
        let enable = EffectId::new(70);
        let issued_after = InputObservationGeneration::new(4);
        let stale_wrong = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(1),
            vec![window_with_ack(
                source,
                4,
                0.0,
                1.0,
                WindowInputState::PassThrough,
                InputEffectAcknowledgement::known(Some(EffectId::new(71))),
            )],
            vec![pointer(PointerWindow::None, 700.0)],
            Vec::new(),
        )
        .expect("stale wrong acknowledgement must remain representable");
        registry
            .apply_snapshot(&stale_wrong)
            .expect("stale facts must apply");
        let source_authority = BTreeMap::from([(
            PointerId::new(1),
            ViewportRouteSource::core_effect(source, enable, issued_after),
        )]);
        let mut routes = ViewportRouteState::default();
        routes
            .publish(
                &stale_wrong,
                &registry,
                CapabilityGeneration::new(1),
                &source_authority,
            )
            .expect("failed authority must still publish a proof");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
        ));

        let exact_acknowledgement = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(2),
            vec![window_with_ack(
                source,
                4,
                0.0,
                1.0,
                WindowInputState::PassThrough,
                InputEffectAcknowledgement::known(Some(enable)),
            )],
            vec![pointer(PointerWindow::None, 700.0)],
            Vec::new(),
        )
        .expect("exact acknowledgement must remain representable");
        let mut exact_registry = exact_registry_seed;
        exact_registry
            .apply_snapshot(&exact_acknowledgement)
            .expect("exact acknowledgement facts must apply");
        let mut exact_routes = ViewportRouteState::default();
        exact_routes
            .publish(
                &exact_acknowledgement,
                &exact_registry,
                CapabilityGeneration::new(1),
                &source_authority,
            )
            .expect("exact acknowledgement authority must publish");
        assert!(matches!(
            exact_routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Known(None)
        ));

        let causally_newer = PlatformSnapshot::new(
            capabilities(),
            unknown_focus(3),
            vec![window_with_ack(
                source,
                5,
                0.0,
                1.0,
                WindowInputState::PassThrough,
                InputEffectAcknowledgement::unknown(
                    AuthorityUnavailableReason::ProviderUnavailable,
                ),
            )],
            vec![pointer(PointerWindow::None, 700.0)],
            Vec::new(),
        )
        .expect("newer capture must remain representable");
        registry
            .apply_snapshot(&causally_newer)
            .expect("newer facts must apply");
        routes
            .publish(
                &causally_newer,
                &registry,
                CapabilityGeneration::new(1),
                &source_authority,
            )
            .expect("causally newer authority must publish");
        assert!(matches!(
            routes
                .proof(PointerId::new(1))
                .expect("route must exist")
                .target(),
            Authority::Known(None)
        ));
    }

    #[test]
    fn route_generation_exhaustion_is_atomic() {
        let mut routes = ViewportRouteState::default();
        routes.exhaust_generation();
        let before = routes.clone();
        let snapshot = PlatformSnapshot::new(
            PlatformCapabilities::default(),
            unknown_focus(1),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("empty snapshot must be valid");
        assert_eq!(
            routes.publish(
                &snapshot,
                &ViewportRegistry::default(),
                CapabilityGeneration::new(1),
                &BTreeMap::new(),
            ),
            Err(ViewportRouteError::RouteGenerationExhausted)
        );
        assert_eq!(routes, before);
    }

    #[test]
    fn default_capabilities_do_not_infer_a_route() {
        let mut routes = ViewportRouteState::default();
        let snapshot = PlatformSnapshot::new(
            PlatformCapabilities::default(),
            unknown_focus(1),
            Vec::new(),
            vec![pointer(PointerWindow::None, 0.0)],
            Vec::new(),
        )
        .expect("snapshot must be valid");
        routes
            .publish(
                &snapshot,
                &ViewportRegistry::default(),
                CapabilityGeneration::new(1),
                &BTreeMap::new(),
            )
            .expect("unknown proof still publishes");
        let proof = routes.proof(PointerId::new(1)).expect("proof must exist");
        assert!(matches!(proof.target(), Authority::Unknown(_)));
        assert_eq!(
            snapshot.capabilities().cross_surface_routing(),
            PlatformCapability::Unknown(crate::platform::PlatformCapabilityIssue::new(
                PlatformRequirement::AuthoritativeInventory,
                crate::platform::PlatformCapabilityReason::NotReported,
            ))
        );
    }

    #[test]
    fn non_authoritative_button_capability_overrides_reported_release_value() {
        let mut capabilities = capabilities();
        capabilities.set_authoritative_button_state(PlatformCapability::unknown(
            PlatformRequirement::AuthoritativeButtonState,
            crate::platform::PlatformCapabilityReason::NotReported,
        ));
        let snapshot = PlatformSnapshot::new(
            capabilities,
            unknown_focus(1),
            Vec::new(),
            vec![pointer(PointerWindow::None, 0.0)],
            Vec::new(),
        )
        .expect("snapshot must be valid");
        let mut routes = ViewportRouteState::default();
        routes
            .publish(
                &snapshot,
                &ViewportRegistry::default(),
                CapabilityGeneration::new(1),
                &BTreeMap::new(),
            )
            .expect("route publication must remain total");

        assert_eq!(
            routes
                .proof(PointerId::new(1))
                .expect("proof must exist")
                .button_state(PointerButton::Primary),
            Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable)
        );
    }
}
