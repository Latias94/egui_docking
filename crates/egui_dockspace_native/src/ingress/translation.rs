//! Typed translation from fork-owned native facts into core protocol values.

use super::*;

pub(super) fn accepted_work_area_authority(
    dockspace: &Dockspace,
    commit: &NativeWorkAreaCommit,
) -> Option<AcceptedWorkAreaAuthority> {
    let expected = commit.roster.as_ref()?;
    let viewport = dockspace.engine().viewport();
    if !viewport.capabilities().work_area().is_supported() {
        return None;
    }
    let mut actual = viewport
        .work_areas()
        .map(|(_, work_area)| work_area)
        .collect::<Vec<_>>();
    actual.sort_by_key(|work_area| work_area.token());
    if &actual != expected {
        return None;
    }
    Some(AcceptedWorkAreaAuthority {
        native_generation: commit.native_generation,
        provider: dockspace.engine().platform_provider()?,
        generation: viewport.work_area_generation(),
        tokens: actual.into_iter().map(ObservedWorkArea::token).collect(),
    })
}

pub(super) fn translate_work_area_roster(
    observation: &NativeWorkAreaRosterObservation,
    core_observation_generation: u64,
) -> Result<(WorkAreaRosterObservation, NativeWorkAreaCommit), NativeRuntimeError> {
    let native_generation = observation.generation().get();
    let Some(native_roster) = observation.roster().value() else {
        let reason = map_unavailable(
            observation
                .roster()
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        );
        return Ok((
            WorkAreaRosterObservation::unknown(
                WorkAreaObservationGeneration::new(core_observation_generation),
                reason,
            ),
            NativeWorkAreaCommit {
                native_generation,
                roster: None,
            },
        ));
    };

    let mut roster = Vec::with_capacity(native_roster.len());
    for native in native_roster {
        roster.push(ObservedWorkArea::new(
            WorkAreaToken::new(native.token().get()),
            translate_rect(&native.bounds())?,
            ScaleFactor::new(native.scale_factor())?,
        ));
    }
    roster.sort_by_key(|work_area| work_area.token());
    let translated = WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(core_observation_generation),
        Authority::Known(roster.clone()),
    )?;
    Ok((
        translated,
        NativeWorkAreaCommit {
            native_generation,
            roster: Some(roster),
        },
    ))
}

pub(super) fn work_area_capability(
    observation: &NativeWorkAreaRosterObservation,
) -> PlatformCapability {
    if observation.roster().value().is_some() {
        return PlatformCapability::Supported;
    }
    let reason = observation
        .roster()
        .unavailable_reason()
        .unwrap_or(NativeUnavailableReason::NotObserved);
    match reason {
        NativeUnavailableReason::Unsupported => PlatformCapability::unsupported(
            PlatformRequirement::WorkArea,
            PlatformCapabilityReason::BackendUnsupported,
        ),
        NativeUnavailableReason::NotObserved => PlatformCapability::unknown(
            PlatformRequirement::WorkArea,
            PlatformCapabilityReason::NotReported,
        ),
        NativeUnavailableReason::StaleSource | NativeUnavailableReason::Retired => {
            PlatformCapability::unknown(
                PlatformRequirement::WorkArea,
                PlatformCapabilityReason::EnvironmentUnavailable,
            )
        }
    }
}

pub(super) fn native_window_lifecycle_capability(
    capabilities: NativeBackendCapabilities,
) -> PlatformCapability {
    translate_backend_capability(
        intersect_backend_capability(
            capabilities.window_lifecycle(),
            capabilities.window_visibility(),
        ),
        PlatformRequirement::NativeWindowLifecycle,
    )
}

pub(super) fn translate_capability_roster(
    backend: NativeBackendCapabilities,
    work_area: PlatformCapability,
) -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(native_window_lifecycle_capability(backend));
    capabilities.set_authoritative_inventory(translate_backend_capability(
        backend.authoritative_inventory(),
        PlatformRequirement::AuthoritativeInventory,
    ));
    capabilities.set_hovered_window(translate_backend_capability(
        backend.hovered_window(),
        PlatformRequirement::HoveredWindow,
    ));
    capabilities.set_desktop_pointer_position(translate_backend_capability(
        backend.desktop_pointer_position(),
        PlatformRequirement::DesktopPointerPosition,
    ));
    capabilities.set_authoritative_button_state(translate_backend_capability(
        backend.authoritative_button_state(),
        PlatformRequirement::AuthoritativeButtonState,
    ));
    capabilities.set_global_window_placement(translate_backend_capability(
        backend.global_window_placement(),
        PlatformRequirement::GlobalWindowPlacement,
    ));
    capabilities.set_work_area(work_area);
    capabilities.set_pointer_hit_test_observation(translate_backend_capability(
        backend.pointer_hit_test_observation(),
        PlatformRequirement::PointerHitTestObservation,
    ));
    capabilities.set_pointer_hit_test_control(translate_backend_capability(
        backend.pointer_hit_test_control(),
        PlatformRequirement::PointerHitTestControl,
    ));
    capabilities.set_global_focus_observation(translate_backend_capability(
        backend.global_focus_observation(),
        PlatformRequirement::GlobalFocusObservation,
    ));
    capabilities.set_window_activation_control(translate_backend_capability(
        backend.window_activation_control(),
        PlatformRequirement::WindowActivationControl,
    ));
    capabilities.set_close_cancellation(translate_backend_capability(
        backend.close_cancellation(),
        PlatformRequirement::CloseCancellation,
    ));
    capabilities
}

pub(super) const fn intersect_backend_capability(
    left: NativeBackendCapability,
    right: NativeBackendCapability,
) -> NativeBackendCapability {
    match (left, right) {
        (NativeBackendCapability::Unsupported, _) | (_, NativeBackendCapability::Unsupported) => {
            NativeBackendCapability::Unsupported
        }
        (NativeBackendCapability::Unknown, _) | (_, NativeBackendCapability::Unknown) => {
            NativeBackendCapability::Unknown
        }
        (NativeBackendCapability::Supported, NativeBackendCapability::Supported) => {
            NativeBackendCapability::Supported
        }
    }
}

pub(super) fn translate_backend_capability(
    capability: NativeBackendCapability,
    requirement: PlatformRequirement,
) -> PlatformCapability {
    match capability {
        NativeBackendCapability::Supported => PlatformCapability::Supported,
        NativeBackendCapability::Unsupported => PlatformCapability::unsupported(
            requirement,
            PlatformCapabilityReason::BackendUnsupported,
        ),
        NativeBackendCapability::Unknown => PlatformCapability::unknown(
            requirement,
            PlatformCapabilityReason::EnvironmentUnavailable,
        ),
    }
}

pub(super) fn translate_work_area_route(
    route: &NativeAuthority<NativeWorkAreaRoute>,
    accepted: Option<&AcceptedWorkAreaAuthority>,
) -> Authority<DesktopWorkAreaRoute> {
    let Some(route) = route.value().copied() else {
        return Authority::Unknown(map_unavailable(
            route
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        ));
    };
    let Some(accepted) = accepted else {
        return Authority::Unknown(AuthorityUnavailableReason::NotReported);
    };
    let token = WorkAreaToken::new(route.token().get());
    if !native_work_area_route_is_current(
        route.generation().get(),
        token,
        accepted.native_generation,
        &accepted.tokens,
    ) {
        return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
    }
    Authority::Known(DesktopWorkAreaRoute::new(
        accepted.provider,
        accepted.generation,
        token,
    ))
}

pub(super) fn native_work_area_route_is_current(
    native_generation: u64,
    token: WorkAreaToken,
    accepted_generation: u64,
    accepted_tokens: &BTreeSet<WorkAreaToken>,
) -> bool {
    native_generation == accepted_generation && accepted_tokens.contains(&token)
}

pub(super) fn empty_pointer_interval(
    submitted_pointer_segment: bool,
    watermark: PointerEdgeSequence,
) -> Result<Option<PointerEdgeJournal>, dockspace::pointer_journal::PointerJournalError> {
    if submitted_pointer_segment {
        return Ok(None);
    }
    Ok(Some(PointerEdgeJournal::new(
        watermark,
        watermark,
        Vec::new(),
    )?))
}

pub(super) fn hovered_route(
    edge: &NativePointerEdge,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Option<BoundNativeRoute> {
    let NativeHoveredWindow::Viewport(binding) = edge.hovered().value()? else {
        return None;
    };
    routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding == *binding)
}

pub(super) fn delivery_route(
    edge: &NativePointerEdge,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Option<BoundNativeRoute> {
    let NativePointerDeliveryOwner::Viewport(binding) = edge.delivery_owner().value()? else {
        return None;
    };
    routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding == *binding)
}

pub(super) fn translate_scroll_edge(
    host: dockspace::presentation_observation::PresentationHostLease,
    scroll: NativeScrollEdge,
    native_delivery: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    presented: Option<&PresentedNativePointerGraph>,
    physical_coordinates: Authority<PhysicalScrollCoordinates>,
) -> Result<ScrollEdge, NativeRuntimeError> {
    let delivery = translate_scroll_delivery(host, native_delivery, routes, presented)?;
    let delta = scroll
        .delta()
        .map(|delta| translate_scroll_delta(delta, physical_coordinates))
        .transpose()?;
    Ok(ScrollEdge::new(
        ScrollDeviceId::new(scroll.device().get()),
        scroll
            .sequence()
            .map(|sequence| ScrollSequenceToken::new(sequence.get())),
        match scroll.phase() {
            NativeScrollPhase::Discrete => ScrollPhase::Discrete,
            NativeScrollPhase::Begin => ScrollPhase::Begin,
            NativeScrollPhase::Update => ScrollPhase::Update,
            NativeScrollPhase::End => ScrollPhase::End,
            NativeScrollPhase::Cancel(reason) => ScrollPhase::Cancel(match reason {
                NativeScrollCancelReason::PlatformCancelled => {
                    ScrollCancelReason::PlatformCancelled
                }
                NativeScrollCancelReason::BindingRetired => ScrollCancelReason::BindingRetired,
                NativeScrollCancelReason::DeviceRemoved => ScrollCancelReason::DeviceRemoved,
            }),
        },
        delta,
        translate_authority(scroll.momentum(), |momentum| match momentum {
            NativeScrollMomentum::Direct => ScrollMomentum::Direct,
            NativeScrollMomentum::Momentum => ScrollMomentum::Momentum,
        }),
        translate_authority(scroll.modifiers(), |modifiers| {
            ScrollModifiers::new(
                modifiers.shift(),
                modifiers.control(),
                modifiers.alt(),
                modifiers.command(),
            )
        }),
        delivery,
    )?)
}

pub(super) fn translate_scroll_delivery(
    host: dockspace::presentation_observation::PresentationHostLease,
    native: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    presented: Option<&PresentedNativePointerGraph>,
) -> Result<Authority<ScrollDeliveryEndpoint>, NativeRuntimeError> {
    let Some(NativePointerDeliveryOwner::Viewport(binding)) = native.value() else {
        return Ok(Authority::Unknown(map_unavailable(
            native
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        )));
    };
    let Some(route) = routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding() == *binding)
    else {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    };
    let Some(presented) = presented else {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    };
    if presented.native() != route.exact() || presented.scene().surface() != route.surface() {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    }
    Ok(Authority::Known(ScrollDeliveryEndpoint::new(
        host,
        route.surface(),
        Some(route.core()),
        presented.coordinate_generation(),
    )?))
}

pub(super) fn translate_scroll_delta(
    delta: NativeScrollDelta,
    physical_coordinates: Authority<PhysicalScrollCoordinates>,
) -> Result<ScrollDelta, NativeRuntimeError> {
    let native = delta.vector();
    let vector = FiniteScrollVector::new(native.x(), native.y())?;
    match delta {
        NativeScrollDelta::Lines(_) => Ok(ScrollDelta::Lines(vector)),
        NativeScrollDelta::PhysicalPixels(_) => Ok(ScrollDelta::PhysicalPixels {
            delta: vector,
            coordinates: physical_coordinates,
        }),
    }
}

pub(super) fn event_time_physical_scroll_coordinates(
    retained: &RetainedPointerEdge,
) -> Authority<PhysicalScrollCoordinates> {
    let edge = retained.edge();
    let Some(capture) = edge.delivery_coordinates().value().copied() else {
        return Authority::Unknown(map_unavailable(
            edge.delivery_coordinates()
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        ));
    };
    let (Some(route), Some(graph)) = (retained.delivery_route(), retained.graphs().delivery())
    else {
        return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
    };
    if capture.binding() != route.native_binding()
        || graph.native() != route.exact()
        || graph.scene().surface() != route.surface()
        || capture.native_scale_factor() as f32 != graph.graph().native_pixels_per_point()
        || capture.presentation_scale_factor() as f32 != graph.graph().pixels_per_point()
    {
        return Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable);
    }
    Authority::Known(PhysicalScrollCoordinates::new(
        route.core(),
        graph.coordinate_generation(),
    ))
}

pub(super) fn translate_delivery_owner(
    owner: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Authority<PointerEventDeliveryOwner> {
    translate_delivery_owner_fact(owner.value(), owner.unavailable_reason(), routes)
}

pub(super) fn translate_delivery_owner_fact(
    owner: Option<&NativePointerDeliveryOwner>,
    unavailable: Option<NativeUnavailableReason>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Authority<PointerEventDeliveryOwner> {
    match owner {
        Some(NativePointerDeliveryOwner::Viewport(binding)) => routes
            .get(&binding.viewport_id())
            .copied()
            .filter(|route| route.native_binding == *binding)
            .map_or(
                Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
                |route| Authority::Known(PointerEventDeliveryOwner::Native(route.core)),
            ),
        Some(NativePointerDeliveryOwner::Foreign) => {
            Authority::Known(PointerEventDeliveryOwner::Foreign)
        }
        Some(NativePointerDeliveryOwner::None) => Authority::Known(PointerEventDeliveryOwner::None),
        None => Authority::Unknown(map_unavailable(
            unavailable.unwrap_or(NativeUnavailableReason::NotObserved),
        )),
    }
}

pub(super) fn exact_native(binding: NativeViewportBinding) -> ExactNativeViewport {
    ExactNativeViewport::new(
        binding.viewport_id(),
        NativeViewportIncarnation::new(binding.incarnation().get()),
    )
}

pub(super) fn resolve_semantic_receiver_event(
    dockspace: &Dockspace,
    presentations: &NativePresentationLedger,
    route: BoundNativeRoute,
    graph: &egui::PointerHitGraphSnapshot,
    receiver: egui::WidgetReceiver,
    action: SemanticReceiverAction,
) -> Option<SemanticReceiverEvent> {
    if !receiver.enabled {
        return None;
    }
    let presented = presentations.semantic_graph(route.native_binding(), graph)?;
    let fingerprint = PaintReceiverFingerprint::new(
        receiver.id,
        receiver.layer_id,
        receiver.interact_rect,
        receiver.sense,
        receiver.enabled,
    );
    let PaintReceiverLookup::Dock(target) = dockspace.resolve_retained_receiver(
        presented.scene(),
        presented.emission(),
        route.exact().viewport(),
        graph.widget_pass_nr(),
        fingerprint,
    ) else {
        return None;
    };
    if !dockspace.retained_semantic_receiver_supports(
        presented.scene(),
        presented.emission(),
        target,
        action,
    ) {
        return None;
    }
    Some(SemanticReceiverEvent::new(
        presented.scene(),
        presented.emission(),
        SemanticDelivery::Native(route.core()),
        target,
        action,
    ))
}

pub(super) fn translate_coordinates(
    binding: ViewportBinding,
    generation: u64,
    geometry: &NativeAuthority<eframe::NativeWindowGeometry>,
) -> Result<WindowCoordinateObservation, NativeRuntimeError> {
    let unavailable = geometry
        .unavailable_reason()
        .map(map_unavailable)
        .unwrap_or(AuthorityUnavailableReason::NotReported);
    let (content, outer, native_scale, presentation_scale) = geometry.value().map_or_else(
        || {
            (
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
            )
        },
        |geometry| {
            (
                translate_authority(geometry.content_rect(), translate_rect),
                translate_authority(geometry.outer_rect(), translate_rect),
                translate_authority(geometry.native_scale_factor(), |scale| {
                    ScaleFactor::new(*scale)
                }),
                translate_authority(geometry.presentation_scale_factor(), |scale| {
                    ScaleFactor::new(*scale)
                }),
            )
        },
    );
    Ok(WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(generation),
        transpose_geometry(content)?,
        transpose_geometry(outer)?,
        transpose_geometry(native_scale)?,
        transpose_geometry(presentation_scale)?,
    ))
}

pub(super) fn translate_input_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> InputEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::PointerInput,
    ) {
        return InputEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || InputEffectAcknowledgement::known(None),
        |reason| InputEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

pub(super) fn translate_presentation_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> PresentationEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Presentation,
    ) {
        return PresentationEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || PresentationEffectAcknowledgement::known(None),
        |reason| PresentationEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

pub(super) fn translate_close_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> CloseEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Close,
    ) {
        return CloseEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || CloseEffectAcknowledgement::known(None),
        |reason| CloseEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

pub(super) fn translate_lifecycle_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> CloseEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Lifecycle,
    ) {
        return CloseEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || CloseEffectAcknowledgement::known(None),
        |reason| CloseEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

pub(super) fn transpose_geometry<T>(
    authority: Authority<Result<T, dockspace::geometry::GeometryError>>,
) -> Result<Authority<T>, NativeRuntimeError> {
    match authority {
        Authority::Known(value) => Ok(Authority::Known(value?)),
        Authority::Unknown(reason) => Ok(Authority::Unknown(reason)),
    }
}

pub(super) fn translate_focus(
    focused: &NativeAuthority<NativeFocusedWindow>,
    routes: &BTreeMap<NativeViewportBinding, BoundNativeRoute>,
    focus_routes: &BTreeSet<NativeViewportBinding>,
) -> Authority<GlobalFocusedWindow> {
    match focused.value() {
        Some(NativeFocusedWindow::Viewport(binding)) => routes
            .get(binding)
            .filter(|route| route.native_binding() == *binding && focus_routes.contains(binding))
            .map_or(
                Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
                |route| Authority::Known(GlobalFocusedWindow::Dock(route.core)),
            ),
        Some(NativeFocusedWindow::Foreign) => Authority::Known(GlobalFocusedWindow::Foreign),
        Some(NativeFocusedWindow::None) => Authority::Known(GlobalFocusedWindow::None),
        None => Authority::Unknown(map_unavailable(
            focused
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        )),
    }
}

pub(super) fn translate_focus_acknowledgement(
    effects: &NativeEffectDriver,
    focused: &NativeAuthority<NativeFocusedWindow>,
    windows: &[NativeWindowSnapshot],
    focus_routes: &BTreeSet<NativeViewportBinding>,
) -> Authority<Option<EffectId>> {
    let expected = match focused.value() {
        Some(NativeFocusedWindow::Viewport(binding)) if focus_routes.contains(binding) => {
            Some(*binding)
        }
        Some(NativeFocusedWindow::Viewport(_)) => {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        }
        Some(NativeFocusedWindow::Foreign | NativeFocusedWindow::None) => None,
        None => {
            return Authority::Unknown(map_unavailable(
                focused
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ));
        }
    };

    let mut acknowledged = None;
    for window in windows
        .iter()
        .filter(|window| focus_routes.contains(&window.binding()))
    {
        let observation = window.focus();
        let Some(observed_focused) = observation.value().value().copied() else {
            return Authority::Unknown(map_unavailable(
                observation
                    .value()
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ));
        };
        let acknowledgement = observation.acknowledgement();
        if let Some(reason) = acknowledgement.unavailable_reason() {
            return Authority::Unknown(map_unavailable(reason));
        }
        let Some(correlation) = acknowledgement.correlation() else {
            continue;
        };
        if expected != Some(window.binding()) || !observed_focused || acknowledged.is_some() {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        }
        let Some(effect) = effects.acknowledged_effect(
            Some(correlation),
            window.binding(),
            NativeEffectProperty::Focus,
        ) else {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        };
        acknowledged = Some(effect);
    }
    Authority::Known(acknowledged)
}

pub(super) fn translate_authority<T, U>(
    authority: &NativeAuthority<T>,
    map: impl FnOnce(&T) -> U,
) -> Authority<U> {
    authority.value().map_or_else(
        || {
            Authority::Unknown(map_unavailable(
                authority
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ))
        },
        |value| Authority::Known(map(value)),
    )
}

pub(super) const fn map_unavailable(reason: NativeUnavailableReason) -> AuthorityUnavailableReason {
    match reason {
        NativeUnavailableReason::NotObserved => AuthorityUnavailableReason::NotReported,
        NativeUnavailableReason::Unsupported => AuthorityUnavailableReason::ProviderUnavailable,
        NativeUnavailableReason::StaleSource | NativeUnavailableReason::Retired => {
            AuthorityUnavailableReason::SurfaceUnavailable
        }
    }
}

pub(super) fn translate_point(
    point: &NativePhysicalPoint,
) -> Result<PhysicalPoint, dockspace::geometry::GeometryError> {
    PhysicalPoint::new(f64::from(point.x()), f64::from(point.y()))
}

pub(super) fn translate_rect(
    rect: &NativePhysicalRect,
) -> Result<PhysicalRect, dockspace::geometry::GeometryError> {
    PhysicalRect::from_min_max(translate_point(&rect.min())?, translate_point(&rect.max())?)
}

pub(super) const fn translate_button(button: NativePointerButton) -> PointerButton {
    match button {
        NativePointerButton::Primary => PointerButton::Primary,
        NativePointerButton::Secondary => PointerButton::Secondary,
        NativePointerButton::Middle => PointerButton::Middle,
        NativePointerButton::Back => PointerButton::Other(4),
        NativePointerButton::Forward => PointerButton::Other(5),
        NativePointerButton::Other(button) => PointerButton::Other(button),
    }
}

pub(super) const fn translate_pointer_cancel_reason(
    reason: NativePointerStreamCancelReason,
) -> PointerStreamCancelReason {
    match reason {
        NativePointerStreamCancelReason::PlatformCancelled => {
            PointerStreamCancelReason::ExplicitPlatformCancellation
        }
        NativePointerStreamCancelReason::DeviceRemoved => PointerStreamCancelReason::DeviceRemoved,
        NativePointerStreamCancelReason::BindingRetired => {
            PointerStreamCancelReason::BindingRetired
        }
    }
}

pub(super) const fn semantic_key(key: NativeKey) -> Option<SemanticKey> {
    match key {
        NativeKey::Escape => None,
        NativeKey::ArrowLeft => Some(SemanticKey::ArrowLeft),
        NativeKey::ArrowRight => Some(SemanticKey::ArrowRight),
        NativeKey::ArrowUp => Some(SemanticKey::ArrowUp),
        NativeKey::ArrowDown => Some(SemanticKey::ArrowDown),
        NativeKey::Home => Some(SemanticKey::Home),
        NativeKey::End => Some(SemanticKey::End),
        NativeKey::Enter => Some(SemanticKey::Enter),
        NativeKey::Space => Some(SemanticKey::Space),
    }
}
