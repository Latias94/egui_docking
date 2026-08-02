//! Correlation and dispatch for core effects and restored child materialization.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dockspace::backend_ingress::BackendIngressRecorder;
use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectIndeterminateReason, EffectResult,
    EffectUnsupportedReason, NativeCloseResolution, PlatformEffect, PlatformEffectEmission,
};
use dockspace::geometry::PhysicalRect;
use dockspace::ids::WorkspaceEpoch;
use dockspace::viewport::{ViewportBinding, ViewportRole};
use eframe::{
    NativeEffectCorrelation, NativeEffectDispatchOutcome, NativeEffectProperty, NativeEffectResult,
    NativeEffectSink, NativeEffectSubmitError, NativePhysicalPoint, NativePhysicalRect,
    NativePlatformError, NativeViewportBinding, NativeViewportCreateCorrelation,
    NativeViewportCreateDispatchOutcome, NativeViewportCreateRequestId, NativeViewportCreateResult,
    NativeViewportCreateSink, NativeViewportCreateSubmitError, NativeWindowEffect,
};
use egui::{UserData, ViewportId};
use egui_dockspace::ExactNativeViewport;

use crate::NativeRuntimeError;
use crate::ingress::BoundNativeRoute;
use crate::viewport::NativeViewportRoster;

static NEXT_EFFECT_RUNTIME: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
struct CoreEffectToken {
    runtime: u64,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    native: Option<ExactNativeViewport>,
    property: NativeEffectProperty,
}

#[derive(Clone, Copy, Debug)]
struct CoreCreateToken {
    runtime: u64,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    core: ViewportBinding,
    viewport: ViewportId,
}

#[derive(Clone, Debug)]
struct PendingNativeCreate {
    token: CoreCreateToken,
    request: NativeViewportCreateRequestId,
    parent: NativeViewportBinding,
    placement: PhysicalRect,
    materialized: bool,
    bound: Option<ExactNativeViewport>,
    initialized: bool,
}

#[derive(Clone, Copy, Debug)]
struct RestoredCreateToken {
    runtime: u64,
    surface: dockspace::ids::SurfaceId,
    viewport: ViewportId,
}

#[derive(Clone, Debug)]
struct PendingRestoredCreate {
    token: RestoredCreateToken,
    request: NativeViewportCreateRequestId,
    parent: NativeViewportBinding,
    phase: RestoredCreatePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoredCreatePhase {
    AwaitingResult,
    Materialized,
    Failed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoredCreateTransition {
    Retain(RestoredCreatePhase),
    Retry,
}

/// Owns the only correlation namespace between core effects and fork requests.
#[derive(Clone)]
pub(crate) struct NativeEffectDriver {
    runtime: u64,
    creates: BTreeMap<ViewportId, PendingNativeCreate>,
    restored_creates: BTreeMap<ViewportId, PendingRestoredCreate>,
}

impl NativeEffectDriver {
    pub(crate) fn new() -> Result<Self, NativeRuntimeError> {
        let runtime = NEXT_EFFECT_RUNTIME
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| NativeRuntimeError::IdentityExhausted)?;
        Ok(Self {
            runtime,
            creates: BTreeMap::new(),
            restored_creates: BTreeMap::new(),
        })
    }

    pub(crate) fn schedule_restored_creates(
        &mut self,
        pending_viewports: &std::collections::BTreeSet<ViewportId>,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
        create_sink: &NativeViewportCreateSink,
    ) -> Result<(), NativeRuntimeError> {
        let Some(parent) = routes.get(&ViewportId::ROOT).copied() else {
            return Ok(());
        };
        for viewport in pending_viewports {
            if self.restored_creates.contains_key(viewport) {
                continue;
            }
            let spec = catalog
                .get(*viewport)
                .filter(|spec| {
                    spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some()
                })
                .ok_or(NativeRuntimeError::UnconfiguredViewport {
                    viewport: *viewport,
                })?;
            let token = RestoredCreateToken {
                runtime: self.runtime,
                surface: spec.surface(),
                viewport: *viewport,
            };
            let callback: Arc<egui::DeferredViewportUiCallback> = Arc::new(|_ui| {});
            let request = create_sink
                .submit(
                    parent.native_binding(),
                    *viewport,
                    catalog
                        .restored_staging_builder(*viewport)
                        .expect("validated restored viewport must retain its staging builder"),
                    callback,
                    NativeViewportCreateCorrelation::new(UserData::new(token)),
                )
                .map_err(|source| NativeRuntimeError::RestoredViewportCreateSubmit {
                    surface: spec.surface(),
                    viewport: *viewport,
                    source,
                })?;
            self.restored_creates.insert(
                *viewport,
                PendingRestoredCreate {
                    token,
                    request,
                    parent: parent.native_binding(),
                    phase: RestoredCreatePhase::AwaitingResult,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn reconcile_restored_routes(
        &mut self,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    ) {
        self.restored_creates.retain(|viewport, pending| {
            pending.phase != RestoredCreatePhase::Materialized || !routes.contains_key(viewport)
        });
    }

    pub(crate) fn restored_create_is_materialized(&self, viewport: ViewportId) -> bool {
        self.restored_creates
            .get(&viewport)
            .is_some_and(|pending| pending.phase == RestoredCreatePhase::Materialized)
    }

    pub(crate) fn restored_create_is_terminal(&self, viewport: ViewportId) -> bool {
        self.restored_creates.get(&viewport).is_some_and(|pending| {
            matches!(
                pending.phase,
                RestoredCreatePhase::Failed | RestoredCreatePhase::Unsupported
            )
        })
    }

    pub(crate) fn route_for_new_binding(
        &mut self,
        native: NativeViewportBinding,
    ) -> Option<(dockspace::ids::SurfaceId, ViewportBinding)> {
        let pending = self.creates.get_mut(&native.viewport_id())?;
        if !pending.materialized || pending.bound.is_some() {
            return None;
        }
        let exact = exact_native(native);
        pending.bound = Some(exact);
        Some((pending.token.core.surface(), pending.token.core))
    }

    pub(crate) fn forget_native(&mut self, native: ExactNativeViewport) {
        self.creates
            .retain(|_, pending| pending.bound != Some(native));
    }

    pub(crate) fn consume_effect_result(
        &self,
        result: &NativeEffectResult,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<(), NativeRuntimeError> {
        let Some(token) = result
            .correlation()
            .user_data()
            .downcast_ref::<CoreEffectToken>()
            .copied()
        else {
            return Ok(());
        };
        if token.runtime != self.runtime {
            return Ok(());
        }
        let exact = exact_native(result.binding());
        if token.native != Some(exact) {
            return Err(NativeRuntimeError::HostedProtocol(
                "native effect result changed its exact viewport lifetime".into(),
            ));
        }
        if let Some(dispatch) = translate_dispatch_outcome(result.outcome()) {
            recorder.record_platform_effect_result(EffectResult::new(
                token.effect,
                token.epoch,
                dispatch,
            ))?;
        }
        Ok(())
    }

    pub(crate) fn consume_create_result(
        &mut self,
        result: &NativeViewportCreateResult,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<(), NativeRuntimeError> {
        if let Some(token) = result
            .correlation()
            .user_data()
            .downcast_ref::<RestoredCreateToken>()
            .copied()
        {
            return self.consume_restored_create_result(token, result);
        }
        let Some(token) = result
            .correlation()
            .user_data()
            .downcast_ref::<CoreCreateToken>()
            .copied()
        else {
            return Ok(());
        };
        if token.runtime != self.runtime {
            return Ok(());
        }
        let pending = self.creates.get_mut(&token.viewport).ok_or_else(|| {
            NativeRuntimeError::HostedProtocol(
                "viewport-create result named no pending core request".into(),
            )
        })?;
        if pending.token.effect != token.effect
            || pending.request != result.request_id()
            || pending.parent != result.parent()
            || result.viewport_id() != token.viewport
        {
            return Err(NativeRuntimeError::HostedProtocol(
                "viewport-create result changed its affine request identity".into(),
            ));
        }
        match result.outcome() {
            NativeViewportCreateDispatchOutcome::Materialized => {
                pending.materialized = true;
            }
            NativeViewportCreateDispatchOutcome::Rejected => {
                recorder.record_platform_effect_result(EffectResult::new(
                    token.effect,
                    token.epoch,
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
                ))?;
                self.creates.remove(&token.viewport);
            }
            NativeViewportCreateDispatchOutcome::Failed => {
                recorder.record_platform_effect_result(EffectResult::new(
                    token.effect,
                    token.epoch,
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
                ))?;
                self.creates.remove(&token.viewport);
            }
            NativeViewportCreateDispatchOutcome::Unsupported => {
                recorder.record_platform_effect_result(EffectResult::new(
                    token.effect,
                    token.epoch,
                    EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
                ))?;
                self.creates.remove(&token.viewport);
            }
        }
        Ok(())
    }

    fn consume_restored_create_result(
        &mut self,
        token: RestoredCreateToken,
        result: &NativeViewportCreateResult,
    ) -> Result<(), NativeRuntimeError> {
        if token.runtime != self.runtime {
            return Ok(());
        }
        let pending = self
            .restored_creates
            .get_mut(&token.viewport)
            .ok_or_else(|| {
                NativeRuntimeError::HostedProtocol(
                    "restored viewport-create result named no pending request".into(),
                )
            })?;
        if pending.token.surface != token.surface
            || pending.request != result.request_id()
            || pending.parent != result.parent()
            || result.viewport_id() != token.viewport
        {
            return Err(NativeRuntimeError::HostedProtocol(
                "restored viewport-create result changed its affine request identity".into(),
            ));
        }
        if pending.phase != RestoredCreatePhase::AwaitingResult {
            return Err(NativeRuntimeError::HostedProtocol(
                "restored viewport-create request received more than one terminal result".into(),
            ));
        }
        match restored_create_transition(result.outcome()) {
            RestoredCreateTransition::Retain(phase) => {
                pending.phase = phase;
                Ok(())
            }
            RestoredCreateTransition::Retry => {
                self.restored_creates.remove(&token.viewport);
                Ok(())
            }
        }
    }

    pub(crate) fn acknowledged_effect(
        &self,
        correlation: Option<&NativeEffectCorrelation>,
        native: NativeViewportBinding,
        property: NativeEffectProperty,
    ) -> Option<EffectId> {
        let token = correlation?.user_data().downcast_ref::<CoreEffectToken>()?;
        let exact = exact_native(native);
        (token.runtime == self.runtime && token.native == Some(exact) && token.property == property)
            .then_some(token.effect)
    }

    pub(crate) fn dispatch(
        &mut self,
        effects: &[PlatformEffectEmission],
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
        effect_sink: &NativeEffectSink,
        create_sink: &NativeViewportCreateSink,
    ) -> Vec<EffectResult> {
        let mut terminal = self.initialize_bound_creates(routes, effect_sink);
        for emission in effects {
            let dispatch = match emission.effect() {
                PlatformEffect::CreateWindow {
                    binding,
                    placement,
                    role,
                }
                | PlatformEffect::RequestReplacement {
                    binding,
                    placement,
                    role,
                } => self.dispatch_create(
                    emission,
                    *binding,
                    *placement,
                    *role,
                    routes,
                    catalog,
                    create_sink,
                ),
                PlatformEffect::RetainChild { .. } | PlatformEffect::ContinueCleanup { .. } => {
                    // These are observation/ownership lanes. Keeping the live
                    // viewport in the runtime roster is the concrete action;
                    // no synthetic platform acknowledgement is emitted.
                    Ok(())
                }
                effect => self.dispatch_effect(emission, effect, routes, effect_sink),
            };

            // Sink submission is part of the host's provisional prepare phase. A local
            // rejection is retained beside the effect candidate and becomes an ordered
            // terminal result only if the enclosing core transaction commits.
            if let Err(result) = dispatch {
                terminal.push(dispatch_result(emission.id(), emission.epoch(), result));
            }
        }
        terminal
    }

    fn dispatch_effect(
        &self,
        emission: &PlatformEffectEmission,
        effect: &PlatformEffect,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        effect_sink: &NativeEffectSink,
    ) -> Result<(), EffectDispatchResult> {
        let binding = effect.binding();
        let route = route_for_core(routes, binding).ok_or(EffectDispatchResult::DispatchFailed(
            DispatchFailureReason::WindowUnavailable,
        ))?;
        let native_effect = translate_effect(effect).ok_or(EffectDispatchResult::Unsupported(
            EffectUnsupportedReason::BackendUnsupported,
        ))?;
        let token = CoreEffectToken {
            runtime: self.runtime,
            effect: emission.id(),
            epoch: emission.epoch(),
            native: Some(route.exact()),
            property: native_effect_property(&native_effect),
        };
        effect_sink
            .submit(
                route.native_binding(),
                native_effect,
                NativeEffectCorrelation::new(UserData::new(token)),
            )
            .map(|_| ())
            .map_err(translate_effect_submit_error)
    }

    fn dispatch_create(
        &mut self,
        emission: &PlatformEffectEmission,
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
        create_sink: &NativeViewportCreateSink,
    ) -> Result<(), EffectDispatchResult> {
        if role != ViewportRole::Child {
            return Err(EffectDispatchResult::Unsupported(
                EffectUnsupportedReason::BackendUnsupported,
            ));
        }
        let parent =
            routes
                .get(&ViewportId::ROOT)
                .copied()
                .ok_or(EffectDispatchResult::DispatchFailed(
                    DispatchFailureReason::WindowUnavailable,
                ))?;
        let spec = catalog.child_for_binding(binding);
        if self.creates.contains_key(&spec.viewport()) {
            return Err(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::AdapterRejected,
            ));
        }
        let token = CoreCreateToken {
            runtime: self.runtime,
            effect: emission.id(),
            epoch: emission.epoch(),
            core: binding,
            viewport: spec.viewport(),
        };
        let callback: Arc<egui::DeferredViewportUiCallback> = Arc::new(|_ui| {});
        let request = create_sink
            .submit(
                parent.native_binding(),
                spec.viewport(),
                spec.builder(),
                callback,
                NativeViewportCreateCorrelation::new(UserData::new(token)),
            )
            .map_err(translate_create_submit_error)?;
        self.creates.insert(
            spec.viewport(),
            PendingNativeCreate {
                token,
                request,
                parent: parent.native_binding(),
                placement,
                materialized: false,
                bound: None,
                initialized: false,
            },
        );
        Ok(())
    }

    fn initialize_bound_creates(
        &mut self,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        effect_sink: &NativeEffectSink,
    ) -> Vec<EffectResult> {
        let mut terminal = Vec::new();
        let candidates = self
            .creates
            .iter()
            .filter(|(_, pending)| pending.bound.is_some() && !pending.initialized)
            .map(|(viewport, pending)| (*viewport, pending.token))
            .collect::<Vec<_>>();
        for (viewport, token) in candidates {
            if let Err(result) = self.initialize_bound_create(viewport, routes, effect_sink) {
                terminal.push(dispatch_result(token.effect, token.epoch, result));
                self.creates.remove(&viewport);
            }
        }
        terminal
    }

    fn initialize_bound_create(
        &mut self,
        viewport: ViewportId,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        effect_sink: &NativeEffectSink,
    ) -> Result<(), EffectDispatchResult> {
        let pending =
            self.creates
                .get_mut(&viewport)
                .ok_or(EffectDispatchResult::DispatchFailed(
                    DispatchFailureReason::AdapterRejected,
                ))?;
        let native = pending.bound.ok_or(EffectDispatchResult::DispatchFailed(
            DispatchFailureReason::WindowUnavailable,
        ))?;
        let route = routes
            .get(&viewport)
            .copied()
            .ok_or(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::WindowUnavailable,
            ))?;
        if route.exact() != native || route.core() != pending.token.core {
            return Err(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::WindowUnavailable,
            ));
        }
        let token = CoreEffectToken {
            runtime: self.runtime,
            effect: pending.token.effect,
            epoch: pending.token.epoch,
            native: Some(native),
            property: NativeEffectProperty::Geometry,
        };
        effect_sink
            .submit(
                route.native_binding(),
                NativeWindowEffect::SetOuterRect(to_native_rect(pending.placement).map_err(
                    |_| {
                        EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected)
                    },
                )?),
                NativeEffectCorrelation::new(UserData::new(token)),
            )
            .map_err(translate_effect_submit_error)?;
        let presentation_token = CoreEffectToken {
            property: NativeEffectProperty::Presentation,
            ..token
        };
        effect_sink
            .submit(
                route.native_binding(),
                NativeWindowEffect::SetVisible(false),
                NativeEffectCorrelation::new(UserData::new(presentation_token)),
            )
            .map_err(translate_effect_submit_error)?;
        pending.initialized = true;
        Ok(())
    }
}

fn restored_create_transition(
    outcome: NativeViewportCreateDispatchOutcome,
) -> RestoredCreateTransition {
    match outcome {
        NativeViewportCreateDispatchOutcome::Materialized => {
            RestoredCreateTransition::Retain(RestoredCreatePhase::Materialized)
        }
        NativeViewportCreateDispatchOutcome::Rejected => RestoredCreateTransition::Retry,
        NativeViewportCreateDispatchOutcome::Failed => {
            RestoredCreateTransition::Retain(RestoredCreatePhase::Failed)
        }
        NativeViewportCreateDispatchOutcome::Unsupported => {
            RestoredCreateTransition::Retain(RestoredCreatePhase::Unsupported)
        }
    }
}

fn dispatch_result(
    effect: EffectId,
    epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
) -> EffectResult {
    EffectResult::new(effect, epoch, result)
}

fn translate_effect_submit_error(error: NativeEffectSubmitError) -> EffectDispatchResult {
    match error {
        NativeEffectSubmitError::CycleClosed => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped)
        }
        NativeEffectSubmitError::BindingUnavailable { .. } => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable)
        }
        NativeEffectSubmitError::Platform(error) => translate_platform_error(error),
    }
}

fn translate_create_submit_error(error: NativeViewportCreateSubmitError) -> EffectDispatchResult {
    match error {
        NativeViewportCreateSubmitError::CycleClosed => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped)
        }
        NativeViewportCreateSubmitError::ParentBindingUnavailable { .. } => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable)
        }
        NativeViewportCreateSubmitError::ViewportAlreadyBound { .. } => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected)
        }
        NativeViewportCreateSubmitError::Platform(error) => translate_platform_error(error),
    }
}

fn translate_platform_error(error: NativePlatformError) -> EffectDispatchResult {
    match error {
        NativePlatformError::UnknownBinding | NativePlatformError::RetiredBinding => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable)
        }
        NativePlatformError::CounterExhausted
        | NativePlatformError::ViewportAlreadyRegistered
        | NativePlatformError::UnknownEffectRequest
        | NativePlatformError::EffectRequestMismatch
        | NativePlatformError::EffectNotDispatched
        | NativePlatformError::EffectDispatchAlreadyRecorded
        | NativePlatformError::ViewportCreateLaneBusy
        | NativePlatformError::UnknownViewportCreateRequest
        | NativePlatformError::ViewportCreateRequestMismatch
        | NativePlatformError::PresentationViewportMismatch
        | NativePlatformError::UnknownPresentationTicket
        | NativePlatformError::WindowSnapshotMismatch
        | NativePlatformError::IncompletePlatformRoster
        | NativePlatformError::HostIngressInFlight
        | NativePlatformError::HostIngressPoisoned
        | NativePlatformError::HostIngressSettlementMismatch => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected)
        }
    }
}

fn route_for_core(
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    binding: ViewportBinding,
) -> Option<BoundNativeRoute> {
    routes
        .values()
        .copied()
        .find(|route| route.core() == binding)
}

fn exact_native(binding: NativeViewportBinding) -> ExactNativeViewport {
    ExactNativeViewport::new(
        binding.viewport_id(),
        egui_dockspace::NativeViewportIncarnation::new(binding.incarnation().get()),
    )
}

fn translate_dispatch_outcome(
    outcome: NativeEffectDispatchOutcome,
) -> Option<EffectDispatchResult> {
    match outcome {
        NativeEffectDispatchOutcome::Dispatched => None,
        NativeEffectDispatchOutcome::Rejected => Some(EffectDispatchResult::DispatchFailed(
            DispatchFailureReason::AdapterRejected,
        )),
        NativeEffectDispatchOutcome::Unsupported => Some(EffectDispatchResult::Unsupported(
            EffectUnsupportedReason::BackendUnsupported,
        )),
        NativeEffectDispatchOutcome::Indeterminate => Some(EffectDispatchResult::Indeterminate(
            EffectIndeterminateReason::AcknowledgementLost,
        )),
    }
}

fn translate_effect(effect: &PlatformEffect) -> Option<NativeWindowEffect> {
    match effect {
        PlatformEffect::ShowWindow { .. } => Some(NativeWindowEffect::SetVisible(true)),
        PlatformEffect::CompensatingClose { .. }
        | PlatformEffect::ReleaseChild { .. }
        | PlatformEffect::RequestRootClose { .. } => Some(NativeWindowEffect::Destroy),
        PlatformEffect::CancelRootClose { .. } => Some(NativeWindowEffect::CancelClose),
        PlatformEffect::SetPointerPassthrough { enabled, .. } => {
            Some(NativeWindowEffect::SetPointerPassThrough(*enabled))
        }
        PlatformEffect::RequestFocus { .. } => Some(NativeWindowEffect::RequestFocus),
        PlatformEffect::ResolveNativeClose { resolution, .. } => match resolution {
            NativeCloseResolution::Accept => Some(NativeWindowEffect::Destroy),
            NativeCloseResolution::Cancel => Some(NativeWindowEffect::CancelClose),
        },
        PlatformEffect::CreateWindow { .. }
        | PlatformEffect::RetainChild { .. }
        | PlatformEffect::ContinueCleanup { .. }
        | PlatformEffect::RequestReplacement { .. } => None,
    }
}

fn native_effect_property(effect: &NativeWindowEffect) -> NativeEffectProperty {
    match effect {
        NativeWindowEffect::SetVisible(_) => NativeEffectProperty::Presentation,
        NativeWindowEffect::SetOuterRect(_) => NativeEffectProperty::Geometry,
        NativeWindowEffect::RequestFocus => NativeEffectProperty::Focus,
        NativeWindowEffect::SetPointerPassThrough(_) => NativeEffectProperty::PointerInput,
        NativeWindowEffect::CancelClose => NativeEffectProperty::Close,
        NativeWindowEffect::Destroy => NativeEffectProperty::Lifecycle,
    }
}

fn to_native_rect(rect: PhysicalRect) -> Result<NativePhysicalRect, NativeRuntimeError> {
    let point = |value: dockspace::geometry::PhysicalPoint| -> Result<_, NativeRuntimeError> {
        let x = rounded_i32(value.x())?;
        let y = rounded_i32(value.y())?;
        Ok(NativePhysicalPoint::new(x, y))
    };
    Ok(NativePhysicalRect::new(
        point(rect.min())?,
        point(rect.max())?,
    ))
}

fn rounded_i32(value: f64) -> Result<i32, NativeRuntimeError> {
    let rounded = value.round();
    if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return Err(NativeRuntimeError::HostedProtocol(
            "physical placement exceeds the native coordinate range".into(),
        ));
    }
    Ok(rounded as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_create_failures_are_precise_terminals_without_becoming_runtime_fatal() {
        assert_eq!(
            restored_create_transition(NativeViewportCreateDispatchOutcome::Materialized),
            RestoredCreateTransition::Retain(RestoredCreatePhase::Materialized)
        );
        assert_eq!(
            restored_create_transition(NativeViewportCreateDispatchOutcome::Rejected),
            RestoredCreateTransition::Retry
        );
        assert_eq!(
            restored_create_transition(NativeViewportCreateDispatchOutcome::Failed),
            RestoredCreateTransition::Retain(RestoredCreatePhase::Failed)
        );
        assert_eq!(
            restored_create_transition(NativeViewportCreateDispatchOutcome::Unsupported),
            RestoredCreateTransition::Retain(RestoredCreatePhase::Unsupported)
        );
    }
}
