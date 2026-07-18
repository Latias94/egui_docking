//! Authoritative cross-surface pointer routing with opaque generation proofs.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::coordinates::CoordinateUnavailable;
use crate::ids::SurfaceId;
use crate::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
    SurfacePointer,
};
use crate::platform::{
    ButtonObservation, PlatformCapability, PlatformCapabilityIssue, PlatformSnapshot,
    PointerObservation, PointerWindow, WindowInputState,
};
use crate::viewport::{
    CapabilityGeneration, InventoryGeneration, RouteGeneration, ViewportBinding,
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
    UnknownWindowToken,
    TargetNotReady { surface: SurfaceId },
    SourceBindingStale,
    SourcePassthroughNotObserved,
    Coordinate(CoordinateUnavailable),
}

/// Opaque route proof produced only by the core coordinator.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRouteProof {
    pointer: PointerId,
    stamp: ViewportRouteStamp,
    target: Authority<Option<SurfacePointer>>,
    target_binding: Option<ViewportBinding>,
    buttons: Authority<Vec<ButtonObservation>>,
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
        drag_sources: &BTreeMap<PointerId, ViewportBinding>,
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
    source: Option<ViewportBinding>,
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
        target,
        target_binding,
        buttons,
    }
}

fn resolve_target(
    snapshot: &PlatformSnapshot,
    registry: &ViewportRegistry,
    observation: &PointerObservation,
    source: Option<ViewportBinding>,
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
        PointerWindow::Foreign | PointerWindow::None => Ok((None, None)),
        PointerWindow::Dock(token) => {
            let binding = registry
                .binding_for_token(token)
                .ok_or(ViewportRouteUnavailable::UnknownWindowToken)?;
            let record = registry
                .record(binding.surface())
                .ok_or(ViewportRouteUnavailable::UnknownWindowToken)?;
            if !record.is_ready() {
                return Err(ViewportRouteUnavailable::TargetNotReady {
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
    source: Option<ViewportBinding>,
) -> Result<(), ViewportRouteUnavailable> {
    let Some(source) = source else {
        return Ok(());
    };
    let record = registry
        .record(source.surface())
        .filter(|record| record.binding() == source)
        .ok_or(ViewportRouteUnavailable::SourceBindingStale)?;
    if !record.is_ready() {
        return Err(ViewportRouteUnavailable::SourcePassthroughNotObserved);
    }
    let coordinates = record
        .coordinates()
        .ok_or(ViewportRouteUnavailable::SourcePassthroughNotObserved)?;
    if coordinates.input_state() != Some(WindowInputState::PassThrough) {
        return Err(ViewportRouteUnavailable::SourcePassthroughNotObserved);
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
        | ViewportRouteUnavailable::HoverAuthority(reason) => *reason,
        ViewportRouteUnavailable::Coordinate(_) => {
            AuthorityUnavailableReason::CoordinateUnavailable
        }
        ViewportRouteUnavailable::UnknownWindowToken
        | ViewportRouteUnavailable::TargetNotReady { .. }
        | ViewportRouteUnavailable::SourceBindingStale
        | ViewportRouteUnavailable::SourcePassthroughNotObserved => {
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
        PlatformCapabilities, PlatformCapability, PlatformRequirement, WindowInputState,
    };
    use crate::viewport::{ViewportRole, WindowToken};

    fn capabilities() -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_authoritative_button_state(PlatformCapability::Supported);
        capabilities.set_pointer_passthrough(PlatformCapability::Supported);
        capabilities
    }

    fn window(
        token: WindowToken,
        x: f64,
        scale: f64,
        input: WindowInputState,
    ) -> crate::platform::ObservedWindow {
        crate::platform::ObservedWindow::new(token)
            .with_content_bounds(Authority::Known(
                PhysicalRect::new(x, 0.0, 600.0, 400.0).expect("test bounds must be valid"),
            ))
            .with_scale_factor(Authority::Known(
                ScaleFactor::new(scale).expect("test scale must be valid"),
            ))
            .with_input_state(Authority::Known(input))
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
    fn target_scale_is_used_once_and_foreign_is_authoritative_none() {
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
        registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(2),
                target_token,
                ViewportRole::Child,
            )
            .expect("target must register");
        let facts = PlatformSnapshot::new(
            capabilities(),
            vec![
                window(source_token, 0.0, 1.0, WindowInputState::PassThrough),
                window(target_token, 1000.0, 2.0, WindowInputState::ReceivesInput),
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
                &BTreeMap::from([(PointerId::new(1), source)]),
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
            vec![
                window(source_token, 0.0, 1.0, WindowInputState::PassThrough),
                window(target_token, 1000.0, 2.0, WindowInputState::ReceivesInput),
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
                &BTreeMap::from([(PointerId::new(1), source)]),
            )
            .expect("foreign route must publish");
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
            vec![window(token, 0.0, 1.0, WindowInputState::ReceivesInput)],
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
                &BTreeMap::from([(PointerId::new(1), source)]),
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
    fn route_generation_exhaustion_is_atomic() {
        let mut routes = ViewportRouteState::default();
        routes.exhaust_generation();
        let before = routes.clone();
        let snapshot = PlatformSnapshot::new(
            PlatformCapabilities::default(),
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
