//! Correlation and dispatch for core effects and restored child materialization.

mod cleanup;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dockspace::backend::effect::{
    CleanupObservationToken, DispatchFailureReason, EffectDispatchResult, EffectId,
    EffectIndeterminateReason, EffectResult, EffectUnsupportedReason, NativeCloseResolution,
    PlatformEffect, PlatformEffectEmission,
};
use dockspace::backend::ingress::BackendIngressRecorder;
use dockspace::geometry::PhysicalRect;
use dockspace::ids::{SurfaceId, WorkspaceEpoch};
use dockspace::viewport::{ViewportBinding, ViewportRole};
use eframe::{
    NativeEffectCorrelation, NativeEffectDispatchOutcome, NativeEffectProperty, NativeEffectResult,
    NativeEffectSink, NativeEffectSubmitError, NativePhysicalPoint, NativePhysicalRect,
    NativePlatformError, NativeViewportBinding, NativeViewportCreateCorrelation,
    NativeViewportCreateDispatchOutcome, NativeViewportCreateRequestId, NativeViewportCreateResult,
    NativeViewportCreateSink, NativeViewportCreateSubmitError, NativeWindowEffect,
};
use egui::{UserData, ViewportId};
use egui_dockspace::backend::{
    BackendEffectReceipt, ExactNativeViewport, NativeViewportIncarnation,
};

use crate::NativeRuntimeError;
use crate::ingress::BoundNativeRoute;
use crate::viewport::NativeViewportRoster;

use self::cleanup::{
    CleanupDelivery, CleanupDisposition, CleanupObservation, CleanupRendezvous,
    CleanupRendezvousError, PendingCleanupResult,
};

static NEXT_EFFECT_RUNTIME: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
struct CoreEffectToken {
    runtime: u64,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    provider_incarnation: u64,
    binding: ViewportBinding,
    destructive_cleanup: bool,
    native: Option<ExactNativeViewport>,
    property: NativeEffectProperty,
}

#[derive(Clone, Copy, Debug)]
struct CoreCreateToken {
    runtime: u64,
    effect: EffectId,
    epoch: WorkspaceEpoch,
    provider_incarnation: u64,
    core: ViewportBinding,
    viewport: ViewportId,
}

type NativePendingCleanupResult = PendingCleanupResult<ViewportBinding>;
type NativeCleanupRendezvous = CleanupRendezvous<CleanupObservationToken, u64, ViewportBinding>;
type NativeCleanupDelivery = CleanupDelivery<CleanupObservationToken, u64, ViewportBinding>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum DeferredEffectResult {
    Ordinary(EffectResult),
    Destructive {
        predecessor: EffectId,
        provider_incarnation: u64,
        pending: NativePendingCleanupResult,
    },
    Cleanup(NativeCleanupDelivery),
}

impl DeferredEffectResult {
    fn local_destructive(
        predecessor: EffectId,
        epoch: WorkspaceEpoch,
        provider_incarnation: u64,
        binding: ViewportBinding,
        result: EffectDispatchResult,
    ) -> Self {
        Self::Destructive {
            predecessor,
            provider_incarnation,
            pending: PendingCleanupResult::local(epoch, result, binding),
        }
    }

    #[cfg(test)]
    pub(crate) fn local_destructive_for_test(
        predecessor: EffectId,
        epoch: WorkspaceEpoch,
        provider_incarnation: u64,
        binding: ViewportBinding,
        result: EffectDispatchResult,
    ) -> Self {
        Self::local_destructive(predecessor, epoch, provider_incarnation, binding, result)
    }
}

#[derive(Clone, Copy, Debug)]
struct InitializationReceiptTombstone {
    effect: EffectId,
    epoch: WorkspaceEpoch,
}

impl InitializationReceiptTombstone {
    fn matches(self, token: CoreEffectToken) -> bool {
        self.effect == token.effect
            && self.epoch == token.epoch
            && matches!(
                token.property,
                NativeEffectProperty::Geometry | NativeEffectProperty::Presentation
            )
    }
}

#[derive(Clone, Debug)]
struct PendingNativeCreate {
    token: CoreCreateToken,
    origin: PendingNativeCreateOrigin,
    placement: PhysicalRect,
    bound: Option<ExactNativeViewport>,
    initialization: NativeCreateInitialization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeCreateInitialization {
    Unsubmitted,
    AwaitingDispatch {
        geometry: InitializationLane,
        presentation: InitializationLane,
    },
    Failed,
}

impl NativeCreateInitialization {
    const fn is_publishable(self) -> bool {
        matches!(
            self,
            Self::AwaitingDispatch {
                geometry,
                presentation,
            } if geometry.has_dispatch_proof() && presentation.has_dispatch_proof()
        )
    }

    const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }

    fn observe(
        &mut self,
        property: NativeEffectProperty,
        outcome: NativeEffectDispatchOutcome,
    ) -> Result<InitializationReceiptDisposition, NativeRuntimeError> {
        let Self::AwaitingDispatch {
            geometry,
            presentation,
        } = self
        else {
            return match *self {
                Self::Failed
                    if matches!(
                        property,
                        NativeEffectProperty::Geometry | NativeEffectProperty::Presentation
                    ) =>
                {
                    Ok(InitializationReceiptDisposition::Consumed)
                }
                Self::Unsubmitted => Err(NativeRuntimeError::HostedProtocol(
                    "native initialization result preceded its transactional submission".into(),
                )),
                Self::Failed => Ok(InitializationReceiptDisposition::NotInitialization),
                Self::AwaitingDispatch { .. } => Err(NativeRuntimeError::HostedProtocol(
                    "native initialization state could not be borrowed consistently".into(),
                )),
            };
        };
        let disposition = match property {
            NativeEffectProperty::Geometry => geometry.observe(outcome),
            NativeEffectProperty::Presentation => presentation.observe(outcome),
            _ => return Ok(InitializationReceiptDisposition::NotInitialization),
        };
        if matches!(
            disposition,
            InitializationReceiptDisposition::Report(
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
            )
        ) {
            *self = Self::Failed;
        }
        Ok(disposition)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InitializationLane {
    #[default]
    Awaiting,
    Indeterminate,
    Dispatched,
    DispatchedIndeterminate,
}

impl InitializationLane {
    const fn has_dispatch_proof(self) -> bool {
        matches!(self, Self::Dispatched | Self::DispatchedIndeterminate)
    }

    fn observe(
        &mut self,
        outcome: NativeEffectDispatchOutcome,
    ) -> InitializationReceiptDisposition {
        match outcome {
            NativeEffectDispatchOutcome::Dispatched => {
                *self = match *self {
                    Self::Awaiting | Self::Indeterminate => Self::Dispatched,
                    Self::Dispatched | Self::DispatchedIndeterminate => *self,
                };
                InitializationReceiptDisposition::Consumed
            }
            NativeEffectDispatchOutcome::Indeterminate => match *self {
                Self::Awaiting => {
                    *self = Self::Indeterminate;
                    InitializationReceiptDisposition::Report(EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ))
                }
                Self::Dispatched => {
                    *self = Self::DispatchedIndeterminate;
                    InitializationReceiptDisposition::Report(EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ))
                }
                Self::Indeterminate | Self::DispatchedIndeterminate => {
                    InitializationReceiptDisposition::Consumed
                }
            },
            NativeEffectDispatchOutcome::Rejected => InitializationReceiptDisposition::Report(
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
            NativeEffectDispatchOutcome::Unsupported => InitializationReceiptDisposition::Report(
                EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InitializationReceiptDisposition {
    NotInitialization,
    Consumed,
    Report(EffectDispatchResult),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingEffectRoute {
    surface: SurfaceId,
    core: ViewportBinding,
    initialization: NativeCreateInitialization,
}

impl PendingEffectRoute {
    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn core(self) -> ViewportBinding {
        self.core
    }

    pub(crate) const fn is_publishable(self) -> bool {
        self.initialization.is_publishable()
    }

    pub(crate) const fn is_failed(self) -> bool {
        self.initialization.is_failed()
    }
}

#[derive(Clone, Debug)]
enum PendingNativeCreateOrigin {
    Requested {
        request: NativeViewportCreateRequestId,
        parent: NativeViewportBinding,
        materialized: bool,
    },
    Adopted {
        native: ExactNativeViewport,
    },
}

impl PendingNativeCreateOrigin {
    const fn is_materialized(&self) -> bool {
        match self {
            Self::Requested { materialized, .. } => *materialized,
            Self::Adopted { .. } => true,
        }
    }

    const fn adopted_native(&self) -> Option<ExactNativeViewport> {
        match self {
            Self::Requested { .. } => None,
            Self::Adopted { native } => Some(*native),
        }
    }
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
    Failed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoredCreateTransition {
    Materialized,
    Retain(RestoredCreatePhase),
    Retry,
}

/// Owns the only correlation namespace between core effects and fork requests.
#[derive(Clone)]
pub(crate) struct NativeEffectDriver {
    runtime: u64,
    creates: BTreeMap<ViewportId, PendingNativeCreate>,
    initialization_receipt_tombstones:
        BTreeMap<ExactNativeViewport, InitializationReceiptTombstone>,
    restored_creates: BTreeMap<ViewportId, PendingRestoredCreate>,
    materialized_restored_viewports: BTreeMap<ViewportId, SurfaceId>,
    cleanup: NativeCleanupRendezvous,
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
            initialization_receipt_tombstones: BTreeMap::new(),
            restored_creates: BTreeMap::new(),
            materialized_restored_viewports: BTreeMap::new(),
            cleanup: NativeCleanupRendezvous::default(),
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
            if self.restored_creates.contains_key(viewport)
                || self.materialized_restored_viewports.contains_key(viewport)
            {
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
        self.materialized_restored_viewports
            .retain(|viewport, _| !routes.contains_key(viewport));
    }

    pub(crate) fn restored_create_is_materialized(&self, viewport: ViewportId) -> bool {
        self.materialized_restored_viewports.contains_key(&viewport)
    }

    pub(crate) fn retire_materialized_restored_viewport(
        &mut self,
        viewport: ViewportId,
        surface: SurfaceId,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(materialized_surface) =
            self.materialized_restored_viewports.get(&viewport).copied()
        else {
            return Ok(false);
        };
        if materialized_surface != surface {
            return Err(NativeRuntimeError::IngressUnavailable(
                "materialized restored viewport retirement changed its configured surface",
            ));
        }
        self.materialized_restored_viewports.remove(&viewport);
        Ok(true)
    }

    pub(crate) fn restored_create_terminal_error(&self) -> Option<NativeRuntimeError> {
        self.restored_creates
            .values()
            .find_map(|pending| restored_create_terminal_error(pending.token, pending.phase))
    }

    pub(crate) fn route_for_new_binding(
        &mut self,
        native: NativeViewportBinding,
    ) -> Option<PendingEffectRoute> {
        let pending = self.creates.get_mut(&native.viewport_id())?;
        if !pending.origin.is_materialized() {
            return None;
        }
        let exact = exact_native(native);
        if pending
            .origin
            .adopted_native()
            .is_some_and(|expected| expected != exact)
        {
            return None;
        }
        match pending.bound {
            Some(bound) if bound != exact => return None,
            Some(_) => {}
            None => pending.bound = Some(exact),
        }
        Some(PendingEffectRoute {
            surface: pending.token.core.surface(),
            core: pending.token.core,
            initialization: pending.initialization,
        })
    }

    pub(crate) fn route_for_provisional_retirement(
        &mut self,
        native: ExactNativeViewport,
    ) -> Option<PendingEffectRoute> {
        let pending = self.creates.get_mut(&native.viewport())?;
        if !pending.origin.is_materialized()
            || pending
                .origin
                .adopted_native()
                .is_some_and(|expected| expected != native)
        {
            return None;
        }
        match pending.bound {
            Some(bound) if bound != native => return None,
            Some(_) => {}
            None => pending.bound = Some(native),
        }
        Some(PendingEffectRoute {
            surface: pending.token.core.surface(),
            core: pending.token.core,
            initialization: pending.initialization,
        })
    }

    pub(crate) fn publish_route(&mut self, native: ExactNativeViewport) -> bool {
        let publishable = self.creates.get(&native.viewport()).is_some_and(|pending| {
            pending.bound == Some(native) && pending.initialization.is_publishable()
        });
        if publishable {
            self.creates.remove(&native.viewport());
        }
        publishable
    }

    pub(crate) fn has_provisional_native(&self, native: ExactNativeViewport) -> bool {
        self.creates
            .get(&native.viewport())
            .is_some_and(|pending| pending.bound == Some(native))
    }

    pub(crate) fn forget_pending_native(&mut self, native: ExactNativeViewport) {
        self.creates.retain(|_, pending| {
            pending.bound != Some(native) && pending.origin.adopted_native() != Some(native)
        });
        self.cleanup.forget_native(native);
    }

    /// Retires duplicate-detection state after the fork proves exact ingress quiescence.
    pub(crate) fn retire_cleanup_native(&mut self, native: ExactNativeViewport) {
        self.cleanup.retire_native(native);
    }

    /// Retires all cleanup correlation for one exact core/native binding pair.
    pub(crate) fn retire_cleanup_binding(
        &mut self,
        binding: ViewportBinding,
        native: ExactNativeViewport,
    ) {
        self.cleanup.retire_binding(binding, native);
    }

    pub(crate) fn retain_initialization_receipts(
        &mut self,
        native: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        if self.initialization_receipt_tombstones.contains_key(&native) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native initialization repeated one receipt tombstone",
            ));
        }
        let pending = self.creates.remove(&native.viewport()).ok_or(
            NativeRuntimeError::IngressUnavailable(
                "native initialization receipt retention lost its pending create",
            ),
        )?;
        if pending.bound != Some(native) {
            self.creates.insert(native.viewport(), pending);
            return Err(NativeRuntimeError::IngressUnavailable(
                "native initialization receipt retention changed its exact lifetime",
            ));
        }
        let tombstone = InitializationReceiptTombstone {
            effect: pending.token.effect,
            epoch: pending.token.epoch,
        };
        let previous = self
            .initialization_receipt_tombstones
            .insert(native, tombstone);
        debug_assert!(previous.is_none());
        Ok(())
    }

    pub(crate) fn retire_initialization_receipts(&mut self, native: ExactNativeViewport) {
        self.initialization_receipt_tombstones.remove(&native);
    }

    #[cfg(test)]
    pub(crate) fn has_initialization_receipt_tombstone(&self, native: ExactNativeViewport) -> bool {
        self.initialization_receipt_tombstones.contains_key(&native)
    }

    #[cfg(test)]
    pub(crate) fn cleanup_retained_counts(&self) -> (usize, usize, usize, usize) {
        self.cleanup.retained_counts()
    }

    #[cfg(test)]
    pub(crate) fn install_provisional_native_for_test(
        &mut self,
        native: ExactNativeViewport,
        core: ViewportBinding,
        failed: bool,
    ) {
        self.creates.insert(
            native.viewport(),
            PendingNativeCreate {
                token: CoreCreateToken {
                    runtime: self.runtime,
                    effect: EffectId::new(1),
                    epoch: WorkspaceEpoch::new(1),
                    provider_incarnation: 1,
                    core,
                    viewport: native.viewport(),
                },
                origin: PendingNativeCreateOrigin::Adopted { native },
                placement: PhysicalRect::new(0.0, 0.0, 320.0, 240.0)
                    .expect("test placement is valid"),
                bound: Some(native),
                initialization: if failed {
                    NativeCreateInitialization::Failed
                } else {
                    NativeCreateInitialization::AwaitingDispatch {
                        geometry: InitializationLane::Awaiting,
                        presentation: InitializationLane::Awaiting,
                    }
                },
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn install_materialized_restored_viewport_for_test(
        &mut self,
        viewport: ViewportId,
        surface: SurfaceId,
    ) {
        self.materialized_restored_viewports
            .insert(viewport, surface);
    }

    pub(crate) fn retire_restored_viewport(
        &mut self,
        viewport: ViewportId,
        surface: dockspace::ids::SurfaceId,
    ) -> Result<(), NativeRuntimeError> {
        if self
            .restored_creates
            .get(&viewport)
            .is_some_and(|pending| pending.token.surface != surface)
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "restored viewport retirement changed its configured surface",
            ));
        }
        self.restored_creates.remove(&viewport);
        Ok(())
    }

    pub(crate) fn consume_effect_result(
        &mut self,
        result: &NativeEffectResult,
        current_epoch: WorkspaceEpoch,
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
        if self
            .initialization_receipt_tombstones
            .get(&exact)
            .copied()
            .is_some_and(|tombstone| tombstone.matches(token))
        {
            return Ok(());
        }
        match self.consume_initialization_result(token, exact, result.outcome())? {
            InitializationReceiptDisposition::Consumed => return Ok(()),
            InitializationReceiptDisposition::Report(dispatch) => {
                recorder.record_platform_effect_result(EffectResult::new(
                    token.effect,
                    token.epoch,
                    dispatch,
                ))?;
                return Ok(());
            }
            InitializationReceiptDisposition::NotInitialization => {}
        }
        if let Some(dispatch) = translate_dispatch_outcome(result.outcome()) {
            if token.destructive_cleanup {
                if token.epoch == current_epoch
                    && token.provider_incarnation == recorder.lease().platform_incarnation()
                {
                    recorder.record_platform_effect_result(EffectResult::new(
                        token.effect,
                        token.epoch,
                        dispatch,
                    ))?;
                } else {
                    let pending =
                        PendingCleanupResult::native(token.epoch, dispatch, token.binding, exact);
                    match self
                        .cleanup
                        .record_result(
                            token.effect,
                            pending,
                            current_epoch,
                            recorder.lease().platform_incarnation(),
                        )
                        .map_err(cleanup_rendezvous_error)?
                    {
                        CleanupDisposition::Deliver(delivery) => {
                            self.record_cleanup_delivery(recorder, delivery)?;
                        }
                        CleanupDisposition::Buffered | CleanupDisposition::SettledDuplicate => {}
                    }
                }
            } else {
                recorder.record_platform_effect_result(EffectResult::new(
                    token.effect,
                    token.epoch,
                    dispatch,
                ))?;
            }
        }
        Ok(())
    }

    pub(crate) fn record_deferred_effect_result(
        &mut self,
        recorder: &mut BackendIngressRecorder,
        current_epoch: WorkspaceEpoch,
        result: DeferredEffectResult,
    ) -> Result<(), NativeRuntimeError> {
        match result {
            DeferredEffectResult::Ordinary(result) => {
                recorder.record_platform_effect_result(result)?;
                Ok(())
            }
            DeferredEffectResult::Destructive {
                predecessor,
                provider_incarnation,
                pending,
            } => {
                if pending.epoch() == current_epoch
                    && provider_incarnation == recorder.lease().platform_incarnation()
                {
                    recorder.record_platform_effect_result(EffectResult::new(
                        predecessor,
                        pending.epoch(),
                        pending.result(),
                    ))?;
                    return Ok(());
                }
                match self
                    .cleanup
                    .record_result(
                        predecessor,
                        pending,
                        current_epoch,
                        recorder.lease().platform_incarnation(),
                    )
                    .map_err(cleanup_rendezvous_error)?
                {
                    CleanupDisposition::Deliver(delivery) => {
                        self.record_cleanup_delivery(recorder, delivery)
                    }
                    CleanupDisposition::Buffered | CleanupDisposition::SettledDuplicate => Ok(()),
                }
            }
            DeferredEffectResult::Cleanup(delivery) => {
                self.record_cleanup_delivery(recorder, delivery)
            }
        }
    }

    pub(crate) fn settle_effect_results(&mut self, receipts: &[BackendEffectReceipt]) {
        for receipt in receipts {
            let Some(delivery) = self.cleanup.take_inflight(receipt.ordinal().get()) else {
                continue;
            };
            if receipt.accepted_effect() == Some(delivery.predecessor()) {
                self.cleanup.accept_delivery(delivery);
            } else {
                self.cleanup.reject_delivery(delivery);
            }
        }
    }

    fn record_cleanup_delivery(
        &mut self,
        recorder: &mut BackendIngressRecorder,
        delivery: NativeCleanupDelivery,
    ) -> Result<(), NativeRuntimeError> {
        if !self
            .cleanup
            .delivery_is_current(delivery, recorder.lease().platform_incarnation())
        {
            self.cleanup.reject_delivery(delivery);
            return Ok(());
        }
        let pending = delivery.pending();
        let result =
            EffectResult::observed_via_cleanup(delivery.token(), pending.epoch(), pending.result());
        debug_assert_eq!(result.receipt_epoch(), delivery.receipt_epoch());
        let ordinal = recorder.record_platform_effect_result(result)?;
        self.cleanup
            .mark_inflight(ordinal.get(), delivery)
            .map_err(cleanup_rendezvous_error)
    }

    fn consume_initialization_result(
        &mut self,
        token: CoreEffectToken,
        exact: ExactNativeViewport,
        outcome: NativeEffectDispatchOutcome,
    ) -> Result<InitializationReceiptDisposition, NativeRuntimeError> {
        let Some(pending) = self.creates.get_mut(&exact.viewport()) else {
            return Ok(InitializationReceiptDisposition::NotInitialization);
        };
        if pending.token.effect != token.effect || pending.bound != Some(exact) {
            return Ok(InitializationReceiptDisposition::NotInitialization);
        }
        pending.initialization.observe(token.property, outcome)
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
        let PendingNativeCreateOrigin::Requested {
            request,
            parent,
            materialized,
        } = &mut pending.origin
        else {
            return Err(NativeRuntimeError::HostedProtocol(
                "viewport-create result named an adopted replacement without a create request"
                    .into(),
            ));
        };
        if pending.token.effect != token.effect
            || *request != result.request_id()
            || *parent != result.parent()
            || result.viewport_id() != token.viewport
        {
            return Err(NativeRuntimeError::HostedProtocol(
                "viewport-create result changed its affine request identity".into(),
            ));
        }
        match result.outcome() {
            NativeViewportCreateDispatchOutcome::Materialized => {
                *materialized = true;
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
        let pending = self.restored_creates.get(&token.viewport).ok_or_else(|| {
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
            RestoredCreateTransition::Materialized => {
                if self
                    .materialized_restored_viewports
                    .contains_key(&token.viewport)
                {
                    return Err(NativeRuntimeError::HostedProtocol(
                        "restored viewport-create repeated one materialized lifetime".into(),
                    ));
                }
                self.restored_creates.remove(&token.viewport);
                let previous = self
                    .materialized_restored_viewports
                    .insert(token.viewport, token.surface);
                debug_assert!(previous.is_none());
                Ok(())
            }
            RestoredCreateTransition::Retain(phase) => {
                self.restored_creates
                    .get_mut(&token.viewport)
                    .expect("the validated restored create remains pending")
                    .phase = phase;
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
        deferred_replacements: &BTreeMap<SurfaceId, ExactNativeViewport>,
        adopted_replacements: &mut BTreeMap<ExactNativeViewport, ViewportBinding>,
        catalog: &NativeViewportRoster,
        effect_sink: &NativeEffectSink,
        create_sink: &NativeViewportCreateSink,
    ) -> Result<Vec<DeferredEffectResult>, NativeRuntimeError> {
        self.initialize_bound_creates(routes, effect_sink)?;
        let mut terminal = Vec::new();
        for emission in effects {
            let dispatch = match emission.effect() {
                PlatformEffect::CreateWindow {
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
                PlatformEffect::RequestReplacement {
                    binding,
                    placement,
                    role,
                } => {
                    if let Some(native) = deferred_replacements.get(&binding.surface()).copied() {
                        if adopted_replacements.contains_key(&native) {
                            Err(EffectDispatchResult::DispatchFailed(
                                DispatchFailureReason::AdapterRejected,
                            ))
                        } else {
                            let adoption = self.adopt_existing_replacement(
                                emission, *binding, *placement, *role, native, routes, catalog,
                            );
                            if adoption.is_ok() {
                                adopted_replacements.insert(native, *binding);
                            }
                            adoption
                        }
                    } else {
                        self.dispatch_create(
                            emission,
                            *binding,
                            *placement,
                            *role,
                            routes,
                            catalog,
                            create_sink,
                        )
                    }
                }
                PlatformEffect::RetainChild { .. } => {
                    // These are observation/ownership lanes. Keeping the live
                    // viewport in the runtime roster is the concrete action;
                    // no synthetic platform acknowledgement is emitted.
                    Ok(())
                }
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    ..
                } => match self.register_cleanup_observation(
                    emission,
                    *binding,
                    *predecessor,
                    routes,
                ) {
                    Ok(Some(delivery)) => {
                        terminal.push(DeferredEffectResult::Cleanup(delivery));
                        Ok(())
                    }
                    Ok(None) => Ok(()),
                    Err(result) => Err(result),
                },
                effect => self.dispatch_effect(emission, effect, routes, effect_sink),
            };

            // Sink submission is part of the host's provisional prepare phase. A local
            // rejection is retained beside the effect candidate and becomes an ordered
            // terminal result only if the enclosing core transaction commits.
            if let Err(result) = dispatch {
                if emission.effect().is_destructive_cleanup() {
                    terminal.push(DeferredEffectResult::local_destructive(
                        emission.id(),
                        emission.epoch(),
                        emission.provider().incarnation(),
                        emission.effect().binding(),
                        result,
                    ));
                } else {
                    terminal.push(DeferredEffectResult::Ordinary(dispatch_result(
                        emission.id(),
                        emission.epoch(),
                        result,
                    )));
                }
            }
        }
        Ok(terminal)
    }

    fn register_cleanup_observation(
        &mut self,
        emission: &PlatformEffectEmission,
        binding: ViewportBinding,
        predecessor: EffectId,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<Option<NativeCleanupDelivery>, EffectDispatchResult> {
        let token =
            emission
                .cleanup_observation_token()
                .ok_or(EffectDispatchResult::DispatchFailed(
                    DispatchFailureReason::AdapterRejected,
                ))?;
        let observation = route_for_core(routes, binding).map_or_else(
            || {
                CleanupObservation::local(
                    token,
                    binding,
                    emission.epoch(),
                    emission.provider().incarnation(),
                    emission.provider().incarnation(),
                )
            },
            |route| {
                CleanupObservation::new(
                    token,
                    binding,
                    route.exact(),
                    emission.epoch(),
                    emission.provider().incarnation(),
                    emission.provider().incarnation(),
                )
            },
        );
        match self
            .cleanup
            .register_observation(predecessor, observation)
            .map_err(cleanup_dispatch_error)?
        {
            CleanupDisposition::Deliver(delivery) => Ok(Some(delivery)),
            CleanupDisposition::Buffered | CleanupDisposition::SettledDuplicate => Ok(None),
        }
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
            provider_incarnation: emission.provider().incarnation(),
            binding,
            destructive_cleanup: effect.is_destructive_cleanup(),
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
            provider_incarnation: emission.provider().incarnation(),
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
                origin: PendingNativeCreateOrigin::Requested {
                    request,
                    parent: parent.native_binding(),
                    materialized: false,
                },
                placement,
                bound: None,
                initialization: NativeCreateInitialization::Unsubmitted,
            },
        );
        Ok(())
    }

    fn adopt_existing_replacement(
        &mut self,
        emission: &PlatformEffectEmission,
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
        native: ExactNativeViewport,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
    ) -> Result<(), EffectDispatchResult> {
        if role != ViewportRole::Child {
            return Err(EffectDispatchResult::Unsupported(
                EffectUnsupportedReason::BackendUnsupported,
            ));
        }
        let parent = routes
            .get(&ViewportId::ROOT)
            .ok_or(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::WindowUnavailable,
            ))?;
        if binding.surface() == parent.surface() {
            return Err(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::AdapterRejected,
            ));
        }
        let spec = catalog.child_for_binding(binding);
        if spec.viewport() != native.viewport()
            || routes.contains_key(&native.viewport())
            || self.creates.contains_key(&native.viewport())
        {
            return Err(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::AdapterRejected,
            ));
        }
        let token = CoreCreateToken {
            runtime: self.runtime,
            effect: emission.id(),
            epoch: emission.epoch(),
            provider_incarnation: emission.provider().incarnation(),
            core: binding,
            viewport: native.viewport(),
        };
        self.creates.insert(
            native.viewport(),
            PendingNativeCreate {
                token,
                origin: PendingNativeCreateOrigin::Adopted { native },
                placement,
                bound: Some(native),
                initialization: NativeCreateInitialization::Unsubmitted,
            },
        );
        Ok(())
    }

    fn initialize_bound_creates(
        &mut self,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        effect_sink: &NativeEffectSink,
    ) -> Result<(), NativeRuntimeError> {
        let candidates = self
            .creates
            .iter()
            .filter(|(_, pending)| {
                pending.bound.is_some()
                    && pending.initialization == NativeCreateInitialization::Unsubmitted
            })
            .map(|(viewport, _)| *viewport)
            .collect::<Vec<_>>();
        for viewport in candidates {
            self.initialize_bound_create(viewport, routes, effect_sink)
                .map_err(|result| {
                    NativeRuntimeError::HostedProtocol(format!(
                        "native replacement initialization was not accepted atomically: {result:?}"
                    ))
                })?;
        }
        Ok(())
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
            provider_incarnation: pending.token.provider_incarnation,
            binding: pending.token.core,
            destructive_cleanup: false,
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
        pending.initialization = NativeCreateInitialization::AwaitingDispatch {
            geometry: InitializationLane::Awaiting,
            presentation: InitializationLane::Awaiting,
        };
        Ok(())
    }
}

fn restored_create_transition(
    outcome: NativeViewportCreateDispatchOutcome,
) -> RestoredCreateTransition {
    match outcome {
        NativeViewportCreateDispatchOutcome::Materialized => RestoredCreateTransition::Materialized,
        NativeViewportCreateDispatchOutcome::Rejected => RestoredCreateTransition::Retry,
        NativeViewportCreateDispatchOutcome::Failed => {
            RestoredCreateTransition::Retain(RestoredCreatePhase::Failed)
        }
        NativeViewportCreateDispatchOutcome::Unsupported => {
            RestoredCreateTransition::Retain(RestoredCreatePhase::Unsupported)
        }
    }
}

fn restored_create_terminal_error(
    token: RestoredCreateToken,
    phase: RestoredCreatePhase,
) -> Option<NativeRuntimeError> {
    match phase {
        RestoredCreatePhase::Failed => Some(NativeRuntimeError::RestoredViewportCreateFailed {
            surface: token.surface,
            viewport: token.viewport,
        }),
        RestoredCreatePhase::Unsupported => {
            Some(NativeRuntimeError::RestoredViewportCreateUnsupported {
                surface: token.surface,
                viewport: token.viewport,
            })
        }
        RestoredCreatePhase::AwaitingResult => None,
    }
}

fn dispatch_result(
    effect: EffectId,
    epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
) -> EffectResult {
    EffectResult::new(effect, epoch, result)
}

fn cleanup_rendezvous_error(error: CleanupRendezvousError) -> NativeRuntimeError {
    let detail = match error {
        CleanupRendezvousError::NativeMismatch => {
            "cleanup result changed its exact native viewport lifetime"
        }
        CleanupRendezvousError::LocalResultUnavailable => {
            "route-less cleanup observation did not own a matching local result"
        }
        CleanupRendezvousError::StaleObservation => {
            "cleanup observation replayed an older provider authority"
        }
        CleanupRendezvousError::ConflictingObservation => {
            "cleanup observation reused one authority frontier with different correlation"
        }
        CleanupRendezvousError::ConflictingResult => {
            "one delayed cleanup effect produced conflicting results"
        }
        CleanupRendezvousError::DuplicateIngressOrdinal => {
            "one backend ingress ordinal carried multiple cleanup results"
        }
    };
    NativeRuntimeError::HostedProtocol(detail.into())
}

fn cleanup_dispatch_error(error: CleanupRendezvousError) -> EffectDispatchResult {
    match error {
        CleanupRendezvousError::NativeMismatch | CleanupRendezvousError::LocalResultUnavailable => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable)
        }
        CleanupRendezvousError::StaleObservation
        | CleanupRendezvousError::ConflictingObservation
        | CleanupRendezvousError::ConflictingResult
        | CleanupRendezvousError::DuplicateIngressOrdinal => {
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected)
        }
    }
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
        | NativePlatformError::HostIngressSettlementMismatch
        | NativePlatformError::BindingIngressNotQuiescent
        | NativePlatformError::BindingIngressAlreadyQuiesced => {
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
        NativeViewportIncarnation::new(binding.incarnation().get()),
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

    fn pending_initialization() -> NativeCreateInitialization {
        NativeCreateInitialization::AwaitingDispatch {
            geometry: InitializationLane::Awaiting,
            presentation: InitializationLane::Awaiting,
        }
    }

    #[test]
    fn native_initialization_requires_dispatch_proof_for_both_property_lanes() {
        let mut initialization = pending_initialization();
        assert!(!initialization.is_publishable());

        assert_eq!(
            initialization
                .observe(
                    NativeEffectProperty::Geometry,
                    NativeEffectDispatchOutcome::Dispatched,
                )
                .expect("the geometry receipt is valid"),
            InitializationReceiptDisposition::Consumed,
        );
        assert_eq!(
            initialization
                .observe(
                    NativeEffectProperty::Presentation,
                    NativeEffectDispatchOutcome::Indeterminate,
                )
                .expect("the indeterminate receipt is retained"),
            InitializationReceiptDisposition::Report(EffectDispatchResult::Indeterminate(
                EffectIndeterminateReason::AcknowledgementLost,
            )),
        );
        assert!(!initialization.is_publishable());

        assert_eq!(
            initialization
                .observe(
                    NativeEffectProperty::Presentation,
                    NativeEffectDispatchOutcome::Dispatched,
                )
                .expect("the later dispatch proof is valid"),
            InitializationReceiptDisposition::Consumed,
        );
        assert!(initialization.is_publishable());
    }

    #[test]
    fn native_initialization_reports_only_the_first_terminal_failure() {
        let mut initialization = pending_initialization();
        assert_eq!(
            initialization
                .observe(
                    NativeEffectProperty::Geometry,
                    NativeEffectDispatchOutcome::Rejected,
                )
                .expect("the rejection is valid"),
            InitializationReceiptDisposition::Report(EffectDispatchResult::DispatchFailed(
                DispatchFailureReason::AdapterRejected,
            )),
        );
        assert_eq!(
            initialization
                .observe(
                    NativeEffectProperty::Presentation,
                    NativeEffectDispatchOutcome::Unsupported,
                )
                .expect("the second terminal result is consumed"),
            InitializationReceiptDisposition::Consumed,
        );
        assert!(initialization.is_failed());
        assert!(!initialization.is_publishable());
    }

    #[test]
    fn restored_create_outcomes_distinguish_retry_materialization_and_fatal_failure() {
        assert_eq!(
            restored_create_transition(NativeViewportCreateDispatchOutcome::Materialized),
            RestoredCreateTransition::Materialized
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

        let token = RestoredCreateToken {
            runtime: 7,
            surface: dockspace::ids::SurfaceId::new(11),
            viewport: ViewportId::from_hash_of("restored-create-terminal"),
        };
        assert!(
            restored_create_terminal_error(token, RestoredCreatePhase::AwaitingResult).is_none()
        );
        assert!(matches!(
            restored_create_terminal_error(token, RestoredCreatePhase::Failed),
            Some(NativeRuntimeError::RestoredViewportCreateFailed {
                surface,
                viewport,
            }) if surface == token.surface && viewport == token.viewport
        ));
        assert!(matches!(
            restored_create_terminal_error(token, RestoredCreatePhase::Unsupported),
            Some(NativeRuntimeError::RestoredViewportCreateUnsupported {
                surface,
                viewport,
            }) if surface == token.surface && viewport == token.viewport
        ));
    }
}
